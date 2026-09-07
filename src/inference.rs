//! Resident, offline Parakeet inference. Construct and use an Engine in the
//! inference worker, keeping capture and IPC independent of model execution.

use std::fs::File;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use hound::{SampleFormat, WavReader};
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};

const SAMPLE_RATE: u32 = 16_000;
const MIN_SAMPLES: usize = SAMPLE_RATE as usize / 10;
// Capture stops at 120 seconds. Allow one second for the stop command and WAV
// finalization so a boundary recording retains its final spoken samples.
const MAX_SAMPLES: usize = SAMPLE_RATE as usize * 121;
const MODEL_FILES: [&str; 4] = [
    "encoder.int8.onnx",
    "decoder.int8.onnx",
    "joiner.int8.onnx",
    "tokens.txt",
];

pub struct Engine {
    recognizer: OfflineRecognizer,
}

impl Engine {
    /// Load the three ONNX graphs once. The default static sherpa-onnx build is
    /// CPU-only, so expose no provider switch that would silently fall back.
    pub fn load(model_dir: &Path, num_threads: usize) -> Result<Self> {
        validate_model(model_dir)?;
        ensure!(
            (1..=64).contains(&num_threads),
            "inference thread count must be between 1 and 64"
        );

        let model_path = |name: &str| -> Result<String> {
            let path = model_dir.join(name);
            let path = path.to_str().context("model path must be valid UTF-8")?;
            ensure!(!path.contains('\0'), "model path contains a NUL byte");
            Ok(path.to_owned())
        };
        let mut config = OfflineRecognizerConfig::default();
        config.model_config.transducer = OfflineTransducerModelConfig {
            encoder: Some(model_path("encoder.int8.onnx")?),
            decoder: Some(model_path("decoder.int8.onnx")?),
            joiner: Some(model_path("joiner.int8.onnx")?),
        };
        config.model_config.tokens = Some(model_path("tokens.txt")?);
        config.model_config.model_type = Some("nemo_transducer".into());
        config.model_config.provider = Some("cpu".into());
        config.model_config.num_threads = num_threads as i32;
        config.decoding_method = Some("greedy_search".into());

        let recognizer = OfflineRecognizer::create(&config).with_context(|| {
            format!(
                "cannot load Parakeet from {}; verify the model download and available memory",
                model_dir.display()
            )
        })?;
        Ok(Self { recognizer })
    }

    /// Each utterance gets a fresh stream; the expensive model stays resident.
    /// The caller may discard a result after cancellation while native decoding
    /// completes on this worker thread.
    pub fn transcribe(&mut self, wav: &Path) -> Result<String> {
        let samples = read_audio(wav)?;
        if samples.is_empty() {
            return Ok(String::new());
        }
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(SAMPLE_RATE as i32, &samples);
        self.recognizer.decode(&stream);
        let result = stream
            .get_result()
            .context("sherpa-onnx did not return a recognition result")?;
        Ok(result.text.trim().to_owned())
    }
}

/// Fast startup check, not a replacement for archive SHA256 verification in
/// download-model.sh. ONNX graph validity is checked when the engine loads.
pub fn validate_model(dir: &Path) -> Result<()> {
    ensure!(
        dir.is_dir(),
        "model directory does not exist: {}; run `just-speak model download` first",
        dir.display()
    );
    for name in MODEL_FILES {
        let path = dir.join(name);
        let file = File::open(&path)
            .with_context(|| format!("cannot read model file {}", path.display()))?;
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_file() && metadata.len() > 0,
            "model file is empty or is not a regular file: {}",
            path.display()
        );
    }
    Ok(())
}

fn read_audio(path: &Path) -> Result<Vec<f32>> {
    let mut reader = WavReader::open(path)
        .with_context(|| format!("cannot open WAV recording {}", path.display()))?;
    let spec = reader.spec();
    ensure!(spec.channels == 1, "WAV recording must be mono");
    ensure!(
        spec.sample_rate == SAMPLE_RATE,
        "WAV recording must use a 16000 Hz sample rate"
    );
    ensure!(
        spec.sample_format == SampleFormat::Int && spec.bits_per_sample == 16,
        "WAV recording must use signed 16-bit PCM samples"
    );
    ensure!(
        reader.len() as usize <= MAX_SAMPLES,
        "WAV recording exceeds the 121-second input limit"
    );
    let mut samples = reader
        .samples::<i16>()
        .map(|sample| sample.map(|value| value as f32 / 32768.0))
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("WAV recording has incomplete or invalid sample data")?;

    // Reject accidental taps and exact digital silence before inference. Do
    // not use an amplitude threshold: quiet speech must remain transcribable.
    if samples.len() < MIN_SAMPLES || samples.iter().all(|sample| *sample == 0.0) {
        return Ok(Vec::new());
    }
    // A short word is valid input. Padding avoids undersized tensors in the
    // encoder while preserving all recorded samples.
    if samples.len() < SAMPLE_RATE as usize {
        samples.resize(SAMPLE_RATE as usize, 0.0);
    }
    Ok(samples)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{WavSpec, WavWriter};
    use tempfile::{NamedTempFile, tempdir};

    fn wav_file(channels: u16, sample_rate: u32, samples: &[i16]) -> NamedTempFile {
        let file = NamedTempFile::new().unwrap();
        let spec = WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(file.path(), spec).unwrap();
        for sample in samples {
            writer.write_sample(*sample).unwrap();
        }
        writer.finalize().unwrap();
        file
    }

    #[test]
    fn silence_and_accidental_taps_are_empty() {
        for samples in [vec![], vec![0; 16_000], vec![1000; MIN_SAMPLES - 1]] {
            let wav = wav_file(1, SAMPLE_RATE, &samples);
            assert!(read_audio(wav.path()).unwrap().is_empty());
        }
    }

    #[test]
    fn short_quiet_audio_is_preserved_normalized_and_padded() {
        let mut raw = vec![1; 3200];
        raw[0] = i16::MIN;
        raw[1] = i16::MAX;
        let wav = wav_file(1, SAMPLE_RATE, &raw);
        let audio = read_audio(wav.path()).unwrap();
        assert_eq!(audio.len(), 16_000);
        assert_eq!(audio[0], -1.0);
        assert_eq!(audio[1], 32767.0 / 32768.0);
        assert_eq!(audio[2], 1.0 / 32768.0);
        assert!(audio[raw.len()..].iter().all(|x| *x == 0.0));
    }

    #[test]
    fn incompatible_audio_is_rejected() {
        for (channels, rate) in [(2, SAMPLE_RATE), (1, 48_000)] {
            let wav = wav_file(channels, rate, &[100; 3200]);
            assert!(read_audio(wav.path()).is_err());
        }
        let file = NamedTempFile::new().unwrap();
        assert!(read_audio(file.path()).is_err());
    }

    #[test]
    fn truncated_sample_data_is_rejected() {
        let wav = wav_file(1, SAMPLE_RATE, &[100; 3200]);
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(wav.path())
            .unwrap();
        file.set_len(file.metadata().unwrap().len() - 2).unwrap();
        assert!(read_audio(wav.path()).is_err());
    }

    #[test]
    fn overlong_recording_is_rejected_before_decoding() {
        let wav = wav_file(1, SAMPLE_RATE, &vec![100; MAX_SAMPLES + 1]);
        assert!(
            read_audio(wav.path())
                .unwrap_err()
                .to_string()
                .contains("121-second")
        );
    }

    #[test]
    fn incomplete_models_are_rejected() {
        let directory = tempdir().unwrap();
        assert!(validate_model(directory.path()).is_err());
        for name in MODEL_FILES {
            std::fs::write(directory.path().join(name), [1]).unwrap();
        }
        assert!(validate_model(directory.path()).is_ok());
        std::fs::write(directory.path().join("tokens.txt"), []).unwrap();
        assert!(validate_model(directory.path()).is_err());
    }

    #[test]
    #[ignore = "requires JUST_SPEAK_TEST_MODEL_DIR with downloaded Parakeet model and its test_wavs/0.wav"]
    fn real_model_recognizes_fixture_and_reuses_engine() {
        let model = std::env::var_os("JUST_SPEAK_TEST_MODEL_DIR")
            .expect("set JUST_SPEAK_TEST_MODEL_DIR to the downloaded model directory");
        let model = Path::new(&model);
        let mut engine = Engine::load(model, 2).unwrap();
        let fixture = model.join("test_wavs/0.wav");
        let first = engine.transcribe(&fixture).unwrap();
        assert!(first.to_lowercase().contains("old portrait"), "{first}");
        let silence = wav_file(1, SAMPLE_RATE, &vec![0; 16_000]);
        assert!(engine.transcribe(silence.path()).unwrap().is_empty());
        assert_eq!(engine.transcribe(&fixture).unwrap(), first);
    }
}
