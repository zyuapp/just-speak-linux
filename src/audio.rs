//! PipeWire recording with bounded shutdown and private temporary audio.

use anyhow::{Context, Result, bail, ensure};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

pub struct Recorder {
    child: Option<Child>,
    directory: Option<TempDir>,
    _parent: Option<SpawnParent>,
}

// Linux parent-death signals follow the *spawning thread*. Keep that thread
// alive while the recorder moves between startup, event-loop and finish workers.
struct SpawnParent {
    release: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Drop for SpawnParent {
    fn drop(&mut self) {
        self.release.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The temporary WAV remains available until this value is dropped.
#[derive(Debug)]
pub struct Recording {
    pub path: PathBuf,
    _directory: TempDir,
    duration: f64,
}

impl Recording {
    pub fn duration_seconds(&self) -> Result<f64> {
        Ok(self.duration)
    }
}

impl Recorder {
    pub fn start(input: Option<&str>) -> Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix("just-speak-recording-")
            .tempdir()
            .context("create private recording directory")?;
        let path = directory.path().join("recording.wav");
        // Precreate with mode 0600; pw-record preserves permissions when opening it.
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        let errors = File::create(directory.path().join("pipewire.log"))?;
        let mut command = Command::new("pw-record");
        command.args([
            "--rate",
            "16000",
            "--channels",
            "1",
            "--format",
            "s16",
            "--container",
            "wav",
        ]);
        if let Some(input) = input {
            ensure!(
                !input.trim().is_empty(),
                "microphone target must not be empty"
            );
            command.arg("--target").arg(input);
        }
        command
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(errors);
        let parent = std::process::id() as libc::pid_t;
        // SAFETY: the child only invokes async-signal-safe kernel operations before exec.
        // The parent check closes the race between fork and setting PDEATHSIG.
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGINT) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() != parent {
                    libc::_exit(1);
                }
                Ok(())
            });
        }
        let (created, child_ready) = std::sync::mpsc::sync_channel(1);
        let (release, lifetime) = std::sync::mpsc::channel();
        let spawning = thread::spawn(move || {
            let _ = created.send(command.spawn());
            let _ = lifetime.recv();
        });
        let parent = SpawnParent {
            release: Some(release),
            thread: Some(spawning),
        };
        let child = child_ready
            .recv()
            .context("recorder spawn thread stopped")?
            .context("start pw-record (install PipeWire's audio tools)")?;
        let mut recorder = Self {
            child: Some(child),
            directory: Some(directory),
            _parent: Some(parent),
        };
        // Catch common startup failures without adding substantial hotkey latency.
        thread::sleep(Duration::from_millis(40));
        if let Some(status) = recorder.child.as_mut().unwrap().try_wait()? {
            bail!(
                "pw-record exited during startup ({status}): {}",
                recorder.error_output()
            );
        }
        Ok(recorder)
    }

    pub fn finish(mut self) -> Result<Recording> {
        let child = self.child.as_mut().context("recorder already stopped")?;
        let stopped =
            stop_child(child, Duration::from_secs(2)).context("stop PipeWire recording")?;
        self.child.take();
        let status = stopped.status;
        // pw-record 1.6.8 returns 1 even after a normal SIGINT because pw-cat
        // marks only drained playback as successful. Accept that status only
        // after our stop signal; strict WAV validation below remains required.
        // https://github.com/PipeWire/pipewire/blob/1.6.8/src/tools/pw-cat.c#L2509
        ensure!(
            status.success()
                || status.signal() == Some(libc::SIGINT)
                || (stopped.interrupt_sent && status.code() == Some(1)),
            "pw-record failed ({status}): {}",
            self.error_output()
        );
        let directory = self
            .directory
            .take()
            .context("recording directory missing")?;
        let path = directory.path().join("recording.wav");
        let duration =
            wav_duration(&path).context("recording contains no usable 16 kHz mono audio")?;
        Ok(Recording {
            path,
            _directory: directory,
            duration,
        })
    }

    fn error_output(&self) -> String {
        self.directory
            .as_ref()
            .and_then(|d| File::open(d.path().join("pipewire.log")).ok())
            .map(|file| {
                let mut bytes = Vec::new();
                let _ = file.take(4096).read_to_end(&mut bytes);
                String::from_utf8_lossy(&bytes).trim().to_string()
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "check microphone availability and PipeWire session".into())
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // Cancelling always stops capture before TempDir deletes the audio.
            if stop_child(&mut child, Duration::from_millis(500)).is_err() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

struct StoppedChild {
    status: ExitStatus,
    interrupt_sent: bool,
}

fn stop_child(child: &mut Child, grace: Duration) -> Result<StoppedChild> {
    if let Some(status) = child.try_wait()? {
        return Ok(StoppedChild {
            status,
            interrupt_sent: false,
        });
    }
    // SAFETY: Child owns this unreaped process; its PID cannot be reused yet.
    let interrupt_sent = unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGINT) } == 0;
    if !interrupt_sent {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            return Err(error.into());
        }
    }
    let deadline = Instant::now() + grace;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(StoppedChild {
                status,
                interrupt_sent,
            });
        }
        if Instant::now() >= deadline {
            child.kill().context("kill unresponsive pw-record")?;
            child.wait().context("reap unresponsive pw-record")?;
            bail!("pw-record did not stop within {} ms", grace.as_millis());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Validate all RIFF chunks, including extra metadata emitted by libsndfile.
fn wav_duration(path: &Path) -> Result<f64> {
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    let mut header = [0; 12];
    file.read_exact(&mut header).context("missing WAV header")?;
    ensure!(
        &header[..4] == b"RIFF" && &header[8..] == b"WAVE",
        "not a RIFF WAV file"
    );
    let end = u64::from(u32::from_le_bytes(header[4..8].try_into().unwrap())) + 8;
    ensure!(end <= length && end >= 12, "incomplete WAV container");
    let mut position = 12;
    let mut format_seen = false;
    let mut data_length = None;
    while position < end {
        ensure!(end - position >= 8, "truncated WAV chunk header");
        let mut chunk = [0; 8];
        file.read_exact(&mut chunk)?;
        let size = u64::from(u32::from_le_bytes(chunk[4..].try_into().unwrap()));
        let next = position + 8 + size + size % 2;
        ensure!(next <= end, "truncated WAV chunk");
        match &chunk[..4] {
            b"fmt " => {
                ensure!(!format_seen && size >= 16, "invalid WAV format chunk");
                let mut format = [0; 16];
                file.read_exact(&mut format)?;
                ensure!(
                    u16::from_le_bytes(format[0..2].try_into().unwrap()) == 1,
                    "WAV must contain PCM samples"
                );
                ensure!(
                    u16::from_le_bytes(format[2..4].try_into().unwrap()) == 1,
                    "WAV must be mono"
                );
                ensure!(
                    u32::from_le_bytes(format[4..8].try_into().unwrap()) == 16_000,
                    "WAV sample rate must be 16000 Hz"
                );
                ensure!(
                    u32::from_le_bytes(format[8..12].try_into().unwrap()) == 32_000
                        && u16::from_le_bytes(format[12..14].try_into().unwrap()) == 2
                        && u16::from_le_bytes(format[14..16].try_into().unwrap()) == 16,
                    "WAV must contain signed 16-bit samples"
                );
                format_seen = true;
            }
            b"data" => {
                ensure!(data_length.is_none(), "multiple WAV audio chunks");
                ensure!(size > 0 && size % 2 == 0, "empty or incomplete WAV samples");
                data_length = Some(size);
            }
            _ => {}
        }
        file.seek(SeekFrom::Start(next))?;
        position = next;
    }
    ensure!(format_seen, "missing WAV format");
    let samples = data_length.context("missing WAV audio")?;
    Ok(samples as f64 / 32_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::{BufRead, BufReader, Write};

    fn wav(data_size: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_size as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&16_000u32.to_le_bytes());
        bytes.extend_from_slice(&32_000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(data_size as u32).to_le_bytes());
        bytes.resize(44 + data_size, 0);
        bytes
    }

    fn inspect(bytes: &[u8]) -> Result<f64> {
        let mut file = tempfile::NamedTempFile::new()?;
        file.write_all(bytes)?;
        wav_duration(file.path())
    }

    #[test]
    fn measures_samples_and_accepts_metadata_chunks() {
        assert_eq!(inspect(&wav(32_000)).unwrap(), 1.0);
        let mut bytes = wav(320);
        bytes.splice(12..12, [b'J', b'U', b'N', b'K', 1, 0, 0, 0, 42, 0]);
        let size = (bytes.len() - 8) as u32;
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
        assert_eq!(inspect(&bytes).unwrap(), 0.01);
    }

    #[test]
    fn rejects_empty_truncated_and_wrong_format_audio() {
        assert!(inspect(&wav(0)).is_err());
        let valid = wav(320);
        assert!(inspect(&valid[..valid.len() - 1]).is_err());
        let mut stereo = valid.clone();
        stereo[22] = 2;
        assert!(inspect(&stereo).is_err());
        let mut wrong_rate = valid;
        wrong_rate[24..28].copy_from_slice(&48_000u32.to_le_bytes());
        assert!(inspect(&wrong_rate).is_err());
    }

    #[test]
    fn cancellation_reaps_child_and_deletes_recording() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().to_path_buf();
        fs::write(path.join("recording.wav"), wav(320)).unwrap();
        let child = Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id() as libc::pid_t;
        drop(Recorder {
            child: Some(child),
            directory: Some(directory),
            _parent: None,
        });
        assert!(!path.exists());
        // SAFETY: signal 0 only tests process existence; it sends no signal.
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
    }

    #[test]
    fn finished_recording_lives_until_transcription_drops_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recording.wav");
        fs::write(&path, wav(32_000)).unwrap();
        let child = Command::new("sleep").arg("30").spawn().unwrap();
        let recording = Recorder {
            child: Some(child),
            directory: Some(directory),
            _parent: None,
        }
        .finish()
        .unwrap();
        assert_eq!(recording.duration_seconds().unwrap(), 1.0);
        assert!(path.exists());
        drop(recording);
        assert!(!path.exists());
    }

    fn recorder_exiting_one_after_interrupt(bytes: &[u8]) -> Recorder {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("recording.wav"), bytes).unwrap();
        let mut child = Command::new("sh")
            .args([
                "-c",
                "trap 'exit 1' INT; printf 'ready\\n'; while :; do :; done",
            ])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut ready = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready, "ready\n");
        Recorder {
            child: Some(child),
            directory: Some(directory),
            _parent: None,
        }
    }

    #[test]
    fn pipewire_exit_one_after_requested_stop_requires_valid_audio() {
        let recording = recorder_exiting_one_after_interrupt(&wav(320))
            .finish()
            .unwrap();
        assert_eq!(recording.duration_seconds().unwrap(), 0.01);
        assert!(
            recorder_exiting_one_after_interrupt(&wav(0))
                .finish()
                .is_err()
        );
        assert!(
            recorder_exiting_one_after_interrupt(b"incomplete WAV")
                .finish()
                .is_err()
        );
    }

    #[test]
    fn exit_one_before_stop_is_failure_even_with_valid_audio() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("recording.wav"), wav(320)).unwrap();
        let mut child = Command::new("sh").args(["-c", "exit 1"]).spawn().unwrap();
        assert_eq!(child.wait().unwrap().code(), Some(1));
        let recorder = Recorder {
            child: Some(child),
            directory: Some(directory),
            _parent: None,
        };
        assert!(recorder.finish().is_err());
    }

    #[test]
    fn unresponsive_recorder_is_killed_and_reaped_after_deadline() {
        let mut child = Command::new("sh")
            .args(["-c", "trap '' INT; printf 'ready\\n'; exec sleep 30"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        // Handshake confirms SIGINT is ignored before attempting graceful stop.
        let mut ready = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready, "ready\n");
        let started = Instant::now();
        assert!(stop_child(&mut child, Duration::from_millis(20)).is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(child.try_wait().unwrap().is_some());
    }
}
