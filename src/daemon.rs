use crate::{
    audio::{Recorder, Recording},
    config::{Config, socket_path},
    desktop::{self, PasteTarget},
    inference::Engine,
    protocol::{self, Phase, Request, Response, Status},
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
    Recorded(u64, Result<Recording>),
    Transcribed(u64, Result<String>),
    Delivered(u64, Result<bool>),
}

enum Work {
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
}

impl Service {
    fn snapshot(&self) -> Status {
        let mut status = self.status.clone();
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
            self.status.phase != Phase::Transcribing,
            "transcription is still running; cancel it before starting again"
        );
        let target = if self.config.paste {
            Some(desktop::capture_target()?)
        } else {
            None
        };
        let recorder = Recorder::start(self.config.input.as_deref())?;
        let id = self.generation.begin();
        self.current_generation.store(id, Ordering::Release);
        self.recorder = Some(recorder);
        self.target = target;
        self.started = Some(Instant::now());
        self.status.phase = Phase::Recording;
        self.status.message = None;
        self.status.elapsed_seconds = Some(0.0);
        self.status.can_cancel = true;
        Ok(())
    }

    fn stop(&mut self) {
        let Some(recorder) = self.recorder.take() else {
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
        self.helpers.push(thread::spawn(move || {
            let _ = events.send(Event::Recorded(id, recorder.finish()));
        }));
    }

    fn cancel(&mut self) {
        if !self.status.can_cancel {
            return;
        }
        self.idle(Some("Canceled".into()));
        if let Some(recorder) = self.recorder.take() {
            self.helpers.push(thread::spawn(move || drop(recorder)));
        }
    }

    fn command(&mut self, request: Request) -> Result<()> {
        match request {
            Request::Start => {
                if let Err(error) = self.start() {
                    // A busy/not-ready response must preserve the running operation.
                    if self.status.model_ready && !self.status.can_cancel {
                        self.fail(format!("{error:#}"));
                    }
                    return Err(error);
                }
            }
            Request::Stop => self.stop(),
            Request::Cancel => self.cancel(),
            Request::Status | Request::Watch => {}
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
            Err(error) => self.fail(format!("Transcription failed: {error:#}")),
        }
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Request(request, reply) => {
                let result = self.command(request);
                let _ = reply.send(Response {
                    ok: result.is_ok(),
                    error: result.err().map(|error| format!("{error:#}")),
                    status: self.snapshot(),
                });
                if !matches!(request, Request::Status) {
                    self.broadcast();
                }
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
                        self.idle(None);
                        eprintln!("JustSpeak: model loaded; ready");
                    }
                    Err(error) => self.fail(format!("Cannot load model: {error:#}. Run `just-speak model download`, then restart the service.")),
                }
                self.broadcast();
            }
            Event::Recorded(id, result) => {
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
                match result {
                    Ok(true) => self.idle(Some(
                        if self.config.paste {
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
    if matches!(request, Request::Watch) {
        let (sender, receiver) = mpsc::sync_channel(8);
        events.send(Event::Watch(sender))?;
        for status in receiver {
            protocol::write_json(&mut stream, &status)?;
        }
    } else {
        let (sender, receiver) = mpsc::channel();
        events.send(Event::Request(request, sender))?;
        protocol::write_json(&mut stream, &receiver.recv_timeout(Duration::from_secs(4))?)?;
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
) -> Result<()> {
    let mut engine = match config
        .model_dir()
        .and_then(|path| Engine::load(&path, config.num_threads))
    {
        Ok(engine) => engine,
        Err(error) => {
            events.send(Event::Loaded(Err(error)))?;
            return Ok(());
        }
    };
    events.send(Event::Loaded(Ok(())))?;
    for work in receiver {
        match work {
            Work::Transcribe(id, recording) => {
                if current.load(Ordering::Acquire) != id {
                    continue;
                }
                let result = engine.transcribe(&recording.path);
                // Recording is deleted before notifying the main service.
                drop(recording);
                events.send(Event::Transcribed(id, result))?;
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
    let inference =
        thread::spawn(move || inference_worker(&worker_config, worker_events, current, work));
    let listener_events = events.clone();
    let listener_shutdown = shutdown.clone();
    let server =
        thread::spawn(move || accept_clients(listener, listener_events, listener_shutdown));
    let mut service = Service {
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
