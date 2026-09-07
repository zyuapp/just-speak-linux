use crate::{
    audio::{Recorder, Recording},
    config::{Config, socket_path},
    desktop::{self, PasteTarget},
    feedback::Feedback,
    history::History,
    inference::Engine,
    inputs, model_download,
    protocol::{self, ModelSetup, Phase, Request, Response, Status},
    shortcut,
};
use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io::ErrorKind,
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

enum Event {
    Request(Request, Sender<Response>),
    Watch(SyncSender<Status>),
    Loaded(Result<()>),
    ModelProgress(ModelSetup),
    ModelDownloadFailed(anyhow::Error),
    Prepared(u64, Result<(Recorder, Option<PasteTarget>)>, Feedback),
    Recorded(u64, Result<Recording>, Feedback),
    FeedbackReady(Feedback),
    Preferences(Result<Config>, Sender<Response>),
    Transcribed(u64, Result<String>),
    Delivered(u64, Result<bool>),
}

enum Work {
    SetupModel,
    Transcribe(u64, Recording),
    Shutdown,
}

/// Generation numbers prevent canceled or superseded results from ever pasting.
#[derive(Default)]
struct Generation {
    next: u64,
    active: Option<u64>,
}

impl Generation {
    fn begin(&mut self) -> u64 {
        self.next += 1;
        self.active = Some(self.next);
        self.next
    }

    fn accepts(&self, id: u64) -> bool {
        self.active == Some(id)
    }

    fn cancel(&mut self) {
        self.active = None;
    }
}

struct Service {
    config: Config,
    status: Status,
    generation: Generation,
    current_generation: Arc<AtomicU64>,
    recorder: Option<Recorder>,
    started: Option<Instant>,
    target: Option<PasteTarget>,
    events: Sender<Event>,
    worker: Sender<Work>,
    watchers: Vec<SyncSender<Status>>,
    helpers: Vec<thread::JoinHandle<()>>,
    history: History,
    feedback: Option<Feedback>,
    stop_pending: bool,
    shutdown: Arc<AtomicBool>,
    delivered_text: Option<String>,
}

impl Service {
    fn snapshot(&self) -> Status {
        let mut status = self.status.clone();
        status.shortcut = self.config.shortcut.clone();
        if status.phase == Phase::Recording {
            status.elapsed_seconds = self.started.map(|start| start.elapsed().as_secs_f64());
        }
        status
    }

    fn broadcast(&mut self) {
        let status = self.snapshot();
        // Slow/disconnected UI clients cannot block recording or accumulate memory.
        self.watchers
            .retain(|watcher| watcher.try_send(status.clone()).is_ok());
    }

    fn idle(&mut self, message: Option<String>) {
        self.status.phase = Phase::Idle;
        self.status.message = message;
        self.status.elapsed_seconds = None;
        self.status.can_cancel = false;
        self.started = None;
        self.delivered_text = None;
        self.target = None;
        self.generation.cancel();
        self.current_generation.store(0, Ordering::Release);
    }

    fn fail(&mut self, error: impl std::fmt::Display) {
        self.idle(Some(error.to_string()));
        self.status.phase = Phase::Error;
        eprintln!("JustSpeak: {error}");
    }

    fn start(&mut self) -> Result<()> {
        if self.status.phase == Phase::Recording {
            return Ok(()); // Key repeat must not create another recorder.
        }
        ensure!(
            self.status.model_ready,
            "speech model is not ready; check `just-speak status`"
        );
        ensure!(
            matches!(self.status.phase, Phase::Idle | Phase::Error),
            "transcription is still running; cancel it before starting again"
        );
        let mut feedback = self
            .feedback
            .take()
            .context("Microphone is still stopping; try again in a moment")?;
        let id = self.generation.begin();
        self.current_generation.store(id, Ordering::Release);
        let events = self.events.clone();
        let config = self.config.clone();
        let current = self.current_generation.clone();
        self.stop_pending = false;
        self.helpers.push(thread::spawn(move || {
            let result = (|| {
                let target = if config.paste {
                    desktop::capture_target()?
                } else {
                    None
                };
                if let Some(input) = &config.input {
                    inputs::validate_selected(input)?;
                }
                ensure!(current.load(Ordering::Acquire) == id, "recording canceled");
                // Capture speech immediately, including while optional sound playback
                // or output muting is starting. A slow cue must not lose first words.
                let recorder = Recorder::start(config.input.as_deref())?;
                if let Err(error) =
                    feedback.begin(config.sound_feedback, config.mute_while_recording)
                {
                    eprintln!("JustSpeak: optional recording feedback unavailable: {error:#}");
                    let _ = feedback.end(false);
                }
                ensure!(current.load(Ordering::Acquire) == id, "recording canceled");
                Ok((recorder, target))
            })();
            let _ = events.send(Event::Prepared(id, result, feedback));
        }));
        self.started = Some(Instant::now());
        self.status.phase = Phase::Recording;
        self.status.message = None;
        self.status.elapsed_seconds = Some(0.0);
        self.status.can_cancel = true;
        Ok(())
    }

    fn stop(&mut self) {
        let Some(recorder) = self.recorder.take() else {
            if self.status.phase == Phase::Recording {
                self.stop_pending = true;
            }
            return;
        };
        let Some(mut feedback) = self.feedback.take() else {
            return;
        };
        let Some(id) = self.generation.active else {
            return;
        };
        self.status.phase = Phase::Transcribing;
        self.status.elapsed_seconds = self.started.map(|start| start.elapsed().as_secs_f64());
        self.status.message = Some("Transcribing locally".into());
        let events = self.events.clone();
        // Finalizing the WAV is bounded, but must not delay Escape or status commands.
        let sound = self.config.sound_feedback;
        self.helpers.push(thread::spawn(move || {
            let result = recorder.finish();
            if let Err(error) = feedback.end(sound) {
                eprintln!("JustSpeak: restoring audio: {error:#}");
            }
            let _ = events.send(Event::Recorded(id, result, feedback));
        }));
    }

    fn cancel(&mut self) {
        if !self.status.can_cancel {
            return;
        }
        self.idle(Some("Canceled".into()));
        if let Some(recorder) = self.recorder.take() {
            let feedback = self.feedback.take();
            let events = self.events.clone();
            self.helpers.push(thread::spawn(move || {
                drop(recorder);
                if let Some(mut feedback) = feedback {
                    let _ = feedback.end(false);
                    let _ = events.send(Event::FeedbackReady(feedback));
                }
            }));
        }
    }

    fn require_idle(&self) -> Result<()> {
        ensure!(
            matches!(self.status.phase, Phase::Idle | Phase::Error),
            "Finish or cancel dictation before changing settings"
        );
        ensure!(
            self.feedback.is_some(),
            "Microphone is still stopping; try again in a moment"
        );
        Ok(())
    }

    fn command(&mut self, request: Request) -> Result<()> {
        let paste_history = matches!(&request, Request::HistoryPaste { .. });
        match request {
            Request::Start {} => {
                if let Err(error) = self.start() {
                    // A busy/not-ready response must preserve the running operation.
                    if self.status.model_ready
                        && !self.status.can_cancel
                        && self.status.phase != Phase::Updating
                    {
                        self.fail(format!("{error:#}"));
                    }
                    return Err(error);
                }
            }
            Request::Stop {} => self.stop(),
            Request::Cancel {} => self.cancel(),
            Request::Status {} | Request::Watch {} | Request::Menu {} => {}
            Request::SetupModel {} => {
                self.require_idle()?;
                ensure!(
                    !self.status.model_ready
                        && matches!(
                            self.status.model_setup,
                            Some(ModelSetup::Required | ModelSetup::Failed)
                        ),
                    "Speech model setup is not needed or is already running"
                );
                self.worker
                    .send(Work::SetupModel)
                    .context("model worker stopped")?;
                self.model_progress(ModelSetup::Downloading);
            }
            Request::SetInput { input } => {
                self.require_idle()?;
                if let Some(id) = &input {
                    inputs::validate_selected(id)?;
                }
                let mut config = self.config.clone();
                config.input = input;
                config.save_preference("input")?;
                self.config = config;
            }
            Request::SetOption { key, value } => {
                self.require_idle()?;
                let mut config = self.config.clone();
                match key.as_str() {
                    "paste" => config.paste = value,
                    "sound_feedback" => config.sound_feedback = value,
                    "mute_while_recording" => config.mute_while_recording = value,
                    "history_enabled" => config.history_enabled = value,
                    "auto_check_updates" => config.auto_check_updates = value,
                    _ => bail!("unknown preference"),
                }
                config.save_preference(&key)?;
                self.config = config;
            }
            Request::SetShortcut { .. } => unreachable!("shortcut is applied by a settings worker"),
            Request::HistoryClear {} => {
                self.require_idle()?;
                self.history.clear()?;
            }
            Request::HistoryCopy { id } | Request::HistoryPaste { id } => {
                self.require_idle()?;
                let text = self
                    .history
                    .get(&id)
                    .context("transcript no longer in history")?
                    .text
                    .clone();
                let target = if paste_history {
                    desktop::capture_target()?
                } else {
                    None
                };
                let id = self.generation.begin();
                self.current_generation.store(id, Ordering::Release);
                self.target = target;
                self.status.phase = Phase::Transcribing;
                self.status.can_cancel = true;
                self.deliver(id, text);
            }
            Request::Quit {} => {
                self.require_idle()?;
                self.shutdown.store(true, Ordering::Relaxed);
            }
            Request::BeginUpdate {} => {
                self.require_idle()?;
                self.status.phase = Phase::Updating;
                self.status.message = Some("Installing update".into());
            }
            Request::EndUpdate {} => {
                if self.status.phase == Phase::Updating {
                    self.idle(None);
                }
            }
        }
        Ok(())
    }

    fn transcribed(&mut self, id: u64, result: Result<String>) {
        if !self.generation.accepts(id) {
            return;
        }
        match result {
            Ok(text) if text.trim().is_empty() => self.idle(Some("No speech detected".into())),
            Ok(text) => {
                self.delivered_text = Some(text.clone());
                self.deliver(id, text);
            }
            Err(error) => self.fail(format!("Transcription failed: {error:#}")),
        }
    }

    fn deliver(&mut self, id: u64, text: String) {
        let target = self.target.clone();
        let current = self.current_generation.clone();
        let events = self.events.clone();
        self.status.message = Some("Delivering transcript".into());
        self.helpers.push(thread::spawn(move || {
            let result = match target {
                Some(target) => desktop::paste_if_current(&text, &target, &current, id),
                None if current.load(Ordering::Acquire) == id => {
                    desktop::copy(&text).map(|()| true)
                }
                None => Ok(false),
            };
            let _ = events.send(Event::Delivered(id, result));
        }));
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Request(request, reply) => {
                if matches!(&request, Request::Menu {}) {
                    let status = self.snapshot();
                    let settings = self.config.clone();
                    let history = self.history.entries().to_vec();
                    self.helpers.push(thread::spawn(move || {
                        let (inputs, input_error) = match inputs::list() {
                            Ok(inputs) => (inputs, None), Err(error) => (Vec::new(), Some(format!("{error:#}"))),
                        };
                        let data = serde_json::json!({"version": env!("CARGO_PKG_VERSION"), "settings": settings, "history": history, "inputs": inputs, "input_error": input_error, "desktop": desktop::capabilities()});
                        let _ = reply.send(Response { ok: true, error: None, status, data: Some(data) });
                    }));
                    return;
                }
                if let Request::SetShortcut { shortcut: value } = &request {
                    let validation = self.require_idle().and_then(|()| {
                        ensure!(desktop::capabilities().shortcut_editing, "Shortcut editing requires Hyprland Lua; use your desktop's shortcut settings");
                        shortcut::normalize(value)
                    });
                    match validation {
                        Err(error) => {
                            let _ = reply.send(Response {
                                ok: false,
                                error: Some(format!("{error:#}")),
                                status: self.snapshot(),
                                data: None,
                            });
                        }
                        Ok(value) => {
                            let mut config = self.config.clone();
                            config.shortcut = value;
                            let events = self.events.clone();
                            self.status.phase = Phase::Updating;
                            self.status.message = Some("Saving shortcut".into());
                            self.broadcast();
                            self.helpers.push(thread::spawn(move || {
                                let result = (|| {
                                    let change = shortcut::apply(&config.shortcut)?;
                                    config.save_preference("shortcut")?;
                                    change.commit();
                                    Ok(config)
                                })();
                                let _ = events.send(Event::Preferences(result, reply));
                            }));
                        }
                    }
                    return;
                }
                let quiet = matches!(&request, Request::Status {});
                let result = self.command(request);
                let _ = reply.send(Response {
                    ok: result.is_ok(),
                    error: result.err().map(|error| format!("{error:#}")),
                    status: self.snapshot(),
                    data: None,
                });
                if !quiet {
                    self.broadcast();
                }
            }
            Event::Preferences(result, reply) => {
                let error = match result {
                    Ok(config) => {
                        self.config = config;
                        None
                    }
                    Err(error) => Some(format!("{error:#}")),
                };
                self.idle(error.clone());
                let _ = reply.send(Response {
                    ok: error.is_none(),
                    error,
                    status: self.snapshot(),
                    data: None,
                });
                self.broadcast();
            }
            Event::Watch(watcher) => {
                if watcher.try_send(self.snapshot()).is_ok() {
                    self.watchers.push(watcher);
                }
            }
            Event::Loaded(result) => {
                match result {
                    Ok(()) => {
                        self.status.model_ready = true;
                        self.status.model_setup = None;
                        self.idle(None);
                        eprintln!("JustSpeak: model loaded; ready");
                    }
                    Err(error) => {
                        let missing = self.config.model_dir().is_ok_and(|path| {
                            fs::symlink_metadata(path)
                                .is_err_and(|error| error.kind() == ErrorKind::NotFound)
                        });
                        self.status.model_setup = missing.then_some(ModelSetup::Required);
                        if missing {
                            self.fail("Download the speech model to start dictating.");
                        } else {
                            self.fail(format!("Cannot load model: {error:#}. Check the model files, then restart JustSpeak."));
                        }
                    }
                }
                self.broadcast();
            }
            Event::ModelProgress(stage) => {
                self.model_progress(stage);
                self.broadcast();
            }
            Event::ModelDownloadFailed(error) => {
                self.status.model_setup = Some(ModelSetup::Failed);
                self.fail(format!("Model download failed: {error:#}"));
                self.broadcast();
            }
            Event::FeedbackReady(feedback) => {
                self.feedback = Some(feedback);
            }
            Event::Prepared(id, result, mut feedback) => {
                if !self.generation.accepts(id) || result.is_err() {
                    let error = result.as_ref().err().map(|error| format!("{error:#}"));
                    let events = self.events.clone();
                    self.helpers.push(thread::spawn(move || {
                        drop(result);
                        let _ = feedback.end(false);
                        let _ = events.send(Event::FeedbackReady(feedback));
                    }));
                    if self.generation.accepts(id) {
                        self.fail(format!("Cannot start recording: {:#}", error.unwrap()));
                        self.broadcast();
                    }
                    return;
                }
                let (recorder, target) = result.unwrap();
                self.recorder = Some(recorder);
                self.target = target;
                self.feedback = Some(feedback);
                if self.stop_pending {
                    self.stop();
                }
                self.broadcast();
            }
            Event::Recorded(id, result, feedback) => {
                self.feedback = Some(feedback);
                if !self.generation.accepts(id) {
                    return;
                }
                match result {
                    Ok(recording) => {
                        self.status.elapsed_seconds = recording.duration_seconds().ok();
                        if self.worker.send(Work::Transcribe(id, recording)).is_err() {
                            self.fail("inference worker stopped; restart the service");
                        }
                    }
                    Err(error) => self.fail(format!("Recording failed: {error:#}")),
                }
                self.broadcast();
            }
            Event::Transcribed(id, result) => {
                self.transcribed(id, result);
                self.broadcast();
            }
            Event::Delivered(id, result) => {
                if !self.generation.accepts(id) {
                    return;
                }
                if let Some(text) = self.delivered_text.take()
                    && self.config.history_enabled
                    && !matches!(&result, Ok(false))
                    && let Err(error) = self.history.add(&text)
                {
                    eprintln!("JustSpeak: cannot save transcript history: {error:#}");
                }
                let pasted = self.target.is_some();
                match result {
                    Ok(true) => self.idle(Some(
                        if pasted {
                            "Pasted"
                        } else {
                            "Copied to clipboard"
                        }
                        .into(),
                    )),
                    Ok(false) => self.idle(Some("Canceled".into())),
                    Err(error) => self.fail(format!("{error:#}")),
                }
                self.broadcast();
            }
        }
    }
    fn model_progress(&mut self, stage: ModelSetup) {
        self.status.model_setup = Some(stage);
        self.status.phase = Phase::Loading;
        self.status.message = Some(
            match stage {
                ModelSetup::Verifying => "Verifying the speech model…",
                ModelSetup::Extracting => "Unpacking the speech model…",
                ModelSetup::Loading => "Loading the speech model…",
                _ => "Downloading the speech model…",
            }
            .into(),
        );
    }
}

struct SocketGuard {
    path: PathBuf,
    _lock: File,
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn listen(path: &Path) -> Result<(UnixListener, SocketGuard)> {
    let directory = path.parent().context("socket has no parent directory")?;
    // Never follow a precreated symlink in the runtime directory.
    match fs::DirBuilder::new().mode(0o700).create(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(error).context("create runtime directory; run inside your desktop session");
        }
    }
    let metadata = fs::symlink_metadata(directory)?;
    ensure!(
        metadata.is_dir() && metadata.uid() == unsafe { libc::geteuid() },
        "runtime directory must be owned by the current user and not a symlink"
    );
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    let lock = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(directory.join("daemon.lock"))?;
    lock.try_lock_exclusive()
        .context("JustSpeak is already running")?;
    if path.exists() {
        fs::remove_file(path).context("remove stale socket")?;
    }
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    Ok((
        listener,
        SocketGuard {
            path: path.to_owned(),
            _lock: lock,
        },
    ))
}

fn serve_client(mut stream: UnixStream, events: Sender<Event>) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    let request: Request = serde_json::from_str(&protocol::read_line(&stream)?)?;
    if matches!(request, Request::Watch {}) {
        let (sender, receiver) = mpsc::sync_channel(8);
        events
            .send(Event::Watch(sender))
            .map_err(|_| anyhow::anyhow!("service stopped"))?;
        for status in receiver {
            protocol::write_json(&mut stream, &status)?;
        }
    } else {
        let (sender, receiver) = mpsc::channel();
        events
            .send(Event::Request(request, sender))
            .map_err(|_| anyhow::anyhow!("service stopped"))?;
        protocol::write_json(
            &mut stream,
            &receiver.recv_timeout(Duration::from_secs(15))?,
        )?;
    }
    Ok(())
}

fn accept_clients(listener: UnixListener, events: Sender<Event>, shutdown: Arc<AtomicBool>) {
    while !shutdown.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let events = events.clone();
                thread::spawn(move || {
                    let _ = serve_client(stream, events);
                });
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10))
            }
            Err(error) => {
                eprintln!("JustSpeak socket: {error}");
                break;
            }
        }
    }
}

fn inference_worker(
    config: &Config,
    events: Sender<Event>,
    current: Arc<AtomicU64>,
    receiver: Receiver<Work>,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    let load = || match config
        .model_dir()
        .and_then(|path| Engine::load(&path, config.num_threads))
    {
        Ok(engine) => {
            let _ = events.send(Event::Loaded(Ok(())));
            Some(engine)
        }
        Err(error) => {
            let _ = events.send(Event::Loaded(Err(error)));
            None
        }
    };
    let mut engine = load();
    for work in receiver {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        match work {
            Work::SetupModel => {
                let result = config.model_dir().and_then(|path| {
                    model_download::download(&path, &shutdown, |stage| {
                        let _ = events.send(Event::ModelProgress(stage));
                    })
                });
                match result {
                    Ok(()) if !shutdown.load(Ordering::Relaxed) => {
                        let _ = events.send(Event::ModelProgress(ModelSetup::Loading));
                        engine = load();
                    }
                    Err(error) => {
                        let _ = events.send(Event::ModelDownloadFailed(error));
                    }
                    _ => break,
                }
            }
            Work::Transcribe(id, recording) => {
                if current.load(Ordering::Acquire) != id {
                    continue;
                }
                let result = engine
                    .as_mut()
                    .context("Speech model is not ready")
                    .and_then(|engine| engine.transcribe(&recording.path));
                // Recording is deleted before notifying the main service.
                drop(recording);
                events
                    .send(Event::Transcribed(id, result))
                    .map_err(|_| anyhow::anyhow!("service stopped"))?;
            }
            Work::Shutdown => break,
        }
    }
    Ok(())
}

pub fn run(config: Config) -> Result<()> {
    let (listener, _guard) = listen(&socket_path()?)?;
    let shutdown = Arc::new(AtomicBool::new(false));
    let signal = shutdown.clone();
    ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))?;
    let current_generation = Arc::new(AtomicU64::new(0));
    let (events, receiver) = mpsc::channel();
    let (worker, work) = mpsc::channel();
    let worker_config = config.clone();
    let worker_events = events.clone();
    let current = current_generation.clone();
    let worker_shutdown = shutdown.clone();
    let inference = thread::spawn(move || {
        inference_worker(
            &worker_config,
            worker_events,
            current,
            work,
            worker_shutdown,
        )
    });
    let listener_events = events.clone();
    let listener_shutdown = shutdown.clone();
    let server =
        thread::spawn(move || accept_clients(listener, listener_events, listener_shutdown));
    let history = History::load()?;
    let feedback = Feedback::new()?;
    let mut service = Service {
        history,
        feedback: Some(feedback),
        stop_pending: false,
        shutdown: shutdown.clone(),
        delivered_text: None,
        config,
        status: Status::default(),
        generation: Generation::default(),
        current_generation,
        recorder: None,
        started: None,
        target: None,
        events,
        worker,
        watchers: Vec::new(),
        helpers: Vec::new(),
    };
    let mut last_tick = Instant::now();
    while !shutdown.load(Ordering::Relaxed) {
        if let Ok(event) = receiver.recv_timeout(Duration::from_millis(50)) {
            service.handle(event);
        }
        service.helpers.retain(|helper| !helper.is_finished());
        if service.status.phase == Phase::Recording {
            if service.started.is_some_and(|start| {
                start.elapsed().as_secs() >= service.config.max_recording_seconds
            }) {
                service.stop();
                service.broadcast();
            }
            if last_tick.elapsed() >= Duration::from_secs(1) {
                service.broadcast();
                last_tick = Instant::now();
            }
        }
    }
    service.cancel();
    drop(service.recorder.take());
    service.current_generation.store(0, Ordering::Release);
    let _ = service.worker.send(Work::Shutdown);
    let _ = server.join();
    for helper in service.helpers {
        let _ = helper.join();
    }
    // Startup events may own capture/feedback guards. Release them before a
    // canceled native inference finishes, so shutdown never keeps recording.
    while let Ok(event) = receiver.try_recv() {
        drop(event);
    }
    match inference.join() {
        Ok(result) => result?,
        Err(_) => bail!("inference worker panicked"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_then_restart_never_accepts_the_previous_transcript() {
        let mut generation = Generation::default();
        let old = generation.begin();
        generation.cancel();
        assert!(!generation.accepts(old));
        let new = generation.begin();
        assert!(!generation.accepts(old));
        assert!(generation.accepts(new));
    }

    #[test]
    fn single_instance_lock_keeps_a_second_daemon_from_deleting_the_socket() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("runtime/control.sock");
        let (_listener, guard) = listen(&path).unwrap();
        assert!(listen(&path).is_err());
        assert!(UnixStream::connect(&path).is_ok());
        drop(guard);
        assert!(!path.exists());
    }

    #[test]
    fn runtime_directory_symlinks_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let link = directory.path().join("runtime");
        std::os::unix::fs::symlink(directory.path(), &link).unwrap();
        assert!(listen(&link.join("control.sock")).is_err());
    }
}
