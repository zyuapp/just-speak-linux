//! Optional earcons and reversible output muting. No volume values are changed.

use crate::inputs;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const JOURNAL_LIMIT: u64 = 16 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SavedMute {
    core_cookie: u64,
    name: String,
    serial: u64,
    was_muted: bool,
    volumes: Vec<f64>,
}

#[derive(Clone)]
struct Sink {
    id: u32,
    saved: SavedMute,
    muted: bool,
}

trait Backend: Send {
    fn snapshot(&self) -> Result<Vec<Value>>;
    fn set_mute(&mut self, id: u32, muted: bool) -> Result<()>;
    fn cue(&mut self, path: &Path, wait: bool) -> Result<()>;
}

pub struct Feedback {
    directory: PathBuf,
    saved: Option<SavedMute>,
    backend: Box<dyn Backend>,
}

impl Feedback {
    pub fn new() -> Result<Self> {
        let runtime = env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::geteuid() })));
        Self::load_at(
            runtime.join("just-speak/feedback"),
            Box::<SystemBackend>::default(),
        )
    }

    fn load_at(directory: PathBuf, backend: Box<dyn Backend>) -> Result<Self> {
        fs::create_dir_all(&directory).context("create feedback cache")?;
        let metadata = fs::symlink_metadata(&directory)?;
        ensure!(
            metadata.is_dir() && metadata.uid() == unsafe { libc::geteuid() },
            "feedback cache must be owned by this user and cannot be a symlink"
        );
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        let journal = directory.join("mute.json");
        let saved = read_journal(&journal)?;
        let mut feedback = Self {
            directory,
            saved,
            backend,
        };
        // An interrupted previous daemon can only restore the exact node in the
        // same PipeWire server lifetime, and only if its mute/volume stayed ours.
        feedback.restore()?;
        write_cue(&feedback.directory.join("start-v1.wav"), true)?;
        write_cue(&feedback.directory.join("end-v1.wav"), false)?;
        Ok(feedback)
    }

    /// Call before Recorder::start, on the start worker. The 60 ms start cue is
    /// allowed to finish (250 ms deadline) before capture begins, avoiding cue
    /// leakage into transcription. Output muting follows the cue.
    pub fn begin(&mut self, sound: bool, mute: bool) -> Result<()> {
        self.restore()?;
        if sound {
            self.backend
                .cue(&self.directory.join("start-v1.wav"), true)?;
        }
        if !mute {
            return Ok(());
        }
        let objects = self.backend.snapshot()?;
        let name = inputs::default_node_name(&objects, "default.audio.sink")
            .context("no default audio output is available to mute")?;
        let sink = find_sink(&objects, &name).context("default audio output is unavailable")?;
        if sink.muted {
            return Ok(());
        }
        self.write_journal(&sink.saved)?;
        self.saved = Some(sink.saved);
        if let Err(error) = self.backend.set_mute(sink.id, true) {
            // A timed-out helper might already have applied its request. Keep
            // recovery state if rollback also fails, so shutdown/restart retries.
            let _ = self.restore();
            return Err(error).context("mute audio output for recording");
        }
        Ok(())
    }

    /// Call after microphone capture has stopped, including error and cancel
    /// paths. The optional end cue is asynchronous and never delays inference.
    pub fn end(&mut self, sound: bool) -> Result<()> {
        self.restore()?;
        if sound {
            self.backend
                .cue(&self.directory.join("end-v1.wav"), false)?;
        }
        Ok(())
    }

    fn restore(&mut self) -> Result<()> {
        let Some(saved) = &self.saved else {
            return Ok(());
        };
        let objects = self
            .backend
            .snapshot()
            .context("check audio output before restoring mute")?;
        if let Some(sink) =
            find_sink(&objects, &saved.name).filter(|sink| should_restore(saved, sink))
        {
            self.backend
                .set_mute(sink.id, saved.was_muted)
                .context("restore audio output after recording")?;
        }
        // Different/disconnected output, a new server, or user changes mean the
        // journal is obsolete. Never modify whichever output is now the default.
        match fs::remove_file(self.directory.join("mute.json")) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("remove restored mute state"),
        }
        self.saved = None;
        Ok(())
    }

    fn write_journal(&self, saved: &SavedMute) -> Result<()> {
        let mut file = tempfile::Builder::new()
            .prefix(".mute-")
            .tempfile_in(&self.directory)?;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        serde_json::to_writer(&mut file, saved)?;
        file.write_all(b"\n")?;
        file.as_file().sync_all()?;
        file.persist(self.directory.join("mute.json"))?;
        File::open(&self.directory)?.sync_all()?;
        Ok(())
    }
}

impl Drop for Feedback {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn read_journal(path: &Path) -> Result<Option<SavedMute>> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("open saved audio mute state"),
    };
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.len() <= JOURNAL_LIMIT,
        "saved audio mute state is invalid"
    );
    let saved: SavedMute = serde_json::from_reader(file.take(JOURNAL_LIMIT))
        .context("saved audio mute state could not be read")?;
    ensure!(
        !saved.name.is_empty()
            && saved.name.len() <= 1024
            && saved.serial > 0
            && !saved.was_muted
            && saved.volumes.len() <= 65
            && saved
                .volumes
                .iter()
                .all(|value| value.is_finite() && *value >= 0.0),
        "saved audio mute state is invalid"
    );
    Ok(Some(saved))
}

fn find_sink(objects: &[Value], name: &str) -> Option<Sink> {
    let core_cookie = objects
        .iter()
        .find(|object| object["type"] == "PipeWire:Interface:Core")?["info"]["cookie"]
        .as_u64()?;
    let object = objects.iter().find(|object| {
        object["type"] == "PipeWire:Interface:Node"
            && object["info"]["props"]["node.name"] == name
            && object["info"]["props"]["media.class"] == "Audio/Sink"
    })?;
    let props = &object["info"]["props"];
    let serial = props["object.serial"]
        .as_u64()
        .or_else(|| props["object.serial"].as_str()?.parse().ok())?;
    let parameters = object["info"]["params"]["Props"].as_array()?;
    let mute_props = parameters.iter().find(|props| props["mute"].is_boolean())?;
    let muted = mute_props["mute"].as_bool()?;
    let mut volumes = Vec::new();
    if let Some(volume) = mute_props["volume"].as_f64() {
        volumes.push(volume);
    }
    if let Some(channels) = mute_props["channelVolumes"].as_array() {
        for channel in channels {
            volumes.push(channel.as_f64()?);
        }
    }
    if name.is_empty()
        || name.len() > 1024
        || serial == 0
        || volumes.len() > 65
        || volumes
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
    {
        return None;
    }
    Some(Sink {
        id: object["id"].as_u64()?.try_into().ok()?,
        muted,
        saved: SavedMute {
            core_cookie,
            name: name.to_owned(),
            serial,
            was_muted: muted,
            volumes,
        },
    })
}

fn should_restore(saved: &SavedMute, sink: &Sink) -> bool {
    !saved.was_muted
        && sink.muted
        && saved.name == sink.saved.name
        && saved.serial == sink.saved.serial
        && saved.core_cookie == sink.saved.core_cookie
        && saved.volumes.len() == sink.saved.volumes.len()
        && saved
            .volumes
            .iter()
            .zip(&sink.saved.volumes)
            .all(|(before, after)| (before - after).abs() < 0.000_001)
}

fn write_cue(path: &Path, rising: bool) -> Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        ensure!(
            metadata.is_file() && metadata.uid() == unsafe { libc::geteuid() },
            "feedback cue must be a regular file owned by this user"
        );
        return Ok(());
    }
    let directory = path.parent().context("sound cue has no parent directory")?;
    let file = tempfile::Builder::new()
        .prefix(".cue-")
        .tempfile_in(directory)?;
    let mut writer = hound::WavWriter::new(
        file.as_file().try_clone()?,
        hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for index in 0..960 {
        let position = index as f32 / 959.0;
        let frequency = if rising {
            660.0 + 220.0 * position
        } else {
            880.0 - 220.0 * position
        };
        let envelope = (std::f32::consts::PI * position).sin().powi(2);
        let sample = (std::f32::consts::TAU * frequency * index as f32 / 16_000.0).sin()
            * envelope
            * 3_000.0;
        writer.write_sample(sample as i16)?;
    }
    writer.finalize()?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

#[derive(Default)]
struct SystemBackend {
    players: Vec<JoinHandle<()>>,
}

impl Backend for SystemBackend {
    fn snapshot(&self) -> Result<Vec<Value>> {
        inputs::snapshot()
    }

    fn set_mute(&mut self, id: u32, muted: bool) -> Result<()> {
        inputs::output(
            "wpctl",
            &["set-mute", &id.to_string(), if muted { "1" } else { "0" }],
            Duration::from_secs(1),
            4096,
        )?;
        Ok(())
    }

    fn cue(&mut self, path: &Path, wait: bool) -> Result<()> {
        self.players.retain(|player| !player.is_finished());
        let child = Command::new("pw-play")
            .args(["--latency", "10ms", "--media-role", "Notification"])
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("play recording sound cue")?;
        if wait {
            finish_player(child, Duration::from_millis(250))
        } else {
            self.players.push(thread::spawn(move || {
                let _ = finish_player(child, Duration::from_secs(1));
            }));
            Ok(())
        }
    }
}

impl Drop for SystemBackend {
    fn drop(&mut self) {
        for player in self.players.drain(..) {
            let _ = player.join();
        }
    }
}

fn finish_player(mut child: Child, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                ensure!(status.success(), "sound cue playback failed");
                return Ok(());
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                result.context("wait for sound cue")?;
                anyhow::bail!("sound cue playback timed out");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    struct State {
        objects: Vec<Value>,
        changes: Vec<(u32, bool)>,
        cues: Vec<bool>,
    }
    struct Fake(Arc<Mutex<State>>);
    impl Backend for Fake {
        fn snapshot(&self) -> Result<Vec<Value>> {
            Ok(self.0.lock().unwrap().objects.clone())
        }
        fn set_mute(&mut self, id: u32, muted: bool) -> Result<()> {
            let mut state = self.0.lock().unwrap();
            state.changes.push((id, muted));
            state.objects[1]["info"]["params"]["Props"][0]["mute"] = json!(muted);
            Ok(())
        }
        fn cue(&mut self, _: &Path, wait: bool) -> Result<()> {
            self.0.lock().unwrap().cues.push(wait);
            Ok(())
        }
    }

    fn state() -> Arc<Mutex<State>> {
        Arc::new(Mutex::new(State {
            changes: vec![],
            cues: vec![],
            objects: vec![
                json!({"type":"PipeWire:Interface:Core","info":{"cookie":123}}),
                json!({"id":42,"type":"PipeWire:Interface:Node","info":{
                "props":{"node.name":"stable-speakers","object.serial":77,"media.class":"Audio/Sink"},
                "params":{"Props":[{"mute":false,"volume":1.0,"channelVolumes":[0.5,0.5]}]}}}),
                json!({"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"default"},
                "metadata":[{"subject":0,"key":"default.audio.sink","value":{"name":"stable-speakers"}}]}),
            ],
        }))
    }

    #[test]
    fn mute_and_restore_are_scoped_to_original_sink_and_do_not_change_volume() {
        let root = tempfile::tempdir().unwrap();
        let state = state();
        let mut feedback =
            Feedback::load_at(root.path().to_owned(), Box::new(Fake(state.clone()))).unwrap();
        feedback.begin(true, true).unwrap();
        assert!(root.path().join("mute.json").exists());
        state.lock().unwrap().objects[2]["metadata"][0]["value"]["name"] =
            json!("different-default");
        feedback.end(true).unwrap();
        assert_eq!(state.lock().unwrap().changes, vec![(42, true), (42, false)]);
        assert_eq!(state.lock().unwrap().cues, vec![true, false]);
        assert!(!root.path().join("mute.json").exists());
    }

    #[test]
    fn user_changes_and_reused_node_ids_are_never_overwritten() {
        for change in ["unmute", "volume", "serial", "server"] {
            let root = tempfile::tempdir().unwrap();
            let state = state();
            let mut feedback =
                Feedback::load_at(root.path().to_owned(), Box::new(Fake(state.clone()))).unwrap();
            feedback.begin(false, true).unwrap();
            {
                let mut state = state.lock().unwrap();
                match change {
                    "unmute" => {
                        state.objects[1]["info"]["params"]["Props"][0]["mute"] = json!(false)
                    }
                    "volume" => {
                        state.objects[1]["info"]["params"]["Props"][0]["channelVolumes"] =
                            json!([0.8, 0.8])
                    }
                    "serial" => state.objects[1]["info"]["props"]["object.serial"] = json!(999),
                    "server" => state.objects[0]["info"]["cookie"] = json!(456),
                    _ => unreachable!(),
                }
            }
            feedback.end(false).unwrap();
            assert_eq!(state.lock().unwrap().changes, vec![(42, true)], "{change}");
        }
    }

    #[test]
    fn recovery_journal_restores_after_crash_and_existing_mute_stays_untouched() {
        let root = tempfile::tempdir().unwrap();
        let state = state();
        let mut feedback =
            Feedback::load_at(root.path().to_owned(), Box::new(Fake(state.clone()))).unwrap();
        feedback.begin(false, true).unwrap();
        // Simulate process loss without invoking Drop. The persisted journal is
        // all that a newly started Feedback instance can use for recovery.
        feedback.saved = None;
        drop(feedback);
        let mut recovered =
            Feedback::load_at(root.path().to_owned(), Box::new(Fake(state.clone()))).unwrap();
        assert_eq!(state.lock().unwrap().changes, vec![(42, true), (42, false)]);
        state.lock().unwrap().objects[1]["info"]["params"]["Props"][0]["mute"] = json!(true);
        recovered.begin(false, true).unwrap();
        recovered.end(false).unwrap();
        assert_eq!(state.lock().unwrap().changes.len(), 2);
    }

    #[test]
    fn cues_are_short_pcm_wavs_with_smooth_silent_edges() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("cue.wav");
        write_cue(&path, true).unwrap();
        let mut wav = hound::WavReader::open(path).unwrap();
        assert_eq!(wav.spec().sample_rate, 16000);
        assert_eq!(wav.duration(), 960);
        let samples: Vec<i16> = wav.samples().map(Result::unwrap).collect();
        assert_eq!(samples.first(), Some(&0));
        assert_eq!(samples.last(), Some(&0));
        assert!(samples.iter().any(|sample| sample.abs() > 100));
    }
}
