mod audio;
mod config;
mod daemon;
mod desktop;
mod desktop_assets;
mod feedback;
mod history;
mod inference;
mod inputs;
mod model_download;
mod protocol;
mod shortcut;
mod ui_refresh;
mod updater;

use anyhow::{Context, Result, bail, ensure};
use clap::{Parser, Subcommand};
use config::Config;
use protocol::{Request, Status};
use serde::Serialize;
use std::{
    env,
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    time::Instant,
};

#[derive(Parser)]
#[command(
    version,
    about = "Offline dictation with a resident speech model and desktop integrations"
)]
struct Cli {
    /// Override the Parakeet model directory (default: XDG data directory).
    #[arg(long, global = true)]
    model_dir: Option<PathBuf>,
    /// CPU inference threads, 1–64 (default: available parallelism, capped at 6).
    #[arg(long, global = true)]
    threads: Option<usize>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Print the embedded launcher icon for installation.
    #[command(hide = true)]
    ExportIcon,
    /// Run the resident model and recording service in the foreground.
    Daemon,
    /// Start recording (safe to call repeatedly while the key is held).
    Start,
    /// Stop recording, transcribe locally, and paste.
    Stop,
    /// Cancel recording or discard an in-flight transcription; idle is a no-op.
    Cancel,
    /// Read the running service's state.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Emit newline-delimited JSON status, initially and whenever it changes.
    Watch,
    /// Check local prerequisites without recording or pasting.
    Doctor,
    /// Read settings, microphones, desktop capabilities and recent transcripts.
    Menu {
        #[arg(long)]
        json: bool,
    },
    Input {
        #[command(subcommand)]
        command: InputCommand,
    },
    Settings {
        #[command(subcommand)]
        command: SettingsCommand,
    },
    Shortcut {
        #[command(subcommand)]
        command: ShortcutCommand,
    },
    History {
        #[command(subcommand)]
        command: HistoryCommand,
    },
    /// Open the shared GTK4 settings and history window.
    Window {
        /// Open the key-capture dialog instead of the settings page.
        #[arg(long)]
        record_shortcut: bool,
    },
    Quit,
    Launch,
    Restart,
    Update {
        #[command(subcommand)]
        command: UpdateCommand,
    },
    /// Manage the separately downloaded speech model.
    Model {
        #[command(subcommand)]
        command: ModelCommand,
    },
    /// Transcribe a mono 16 kHz PCM16 WAV to stdout, without clipboard changes.
    Transcribe {
        file: PathBuf,
    },
    /// Measure first and resident inference; fails if the speed target is missed.
    Benchmark {
        file: PathBuf,
        /// Total inferences: first use followed by resident runs (at least 2).
        #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(2..=20))]
        runs: u32,
        /// Minimum resident speed, in multiples of real time.
        #[arg(long, default_value_t = 20.0)]
        minimum_speed: f64,
        /// Require this phrase in every result, ignoring ASCII case.
        #[arg(long)]
        expected_text: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum InputCommand {
    Set { id: String },
}
#[derive(Subcommand)]
enum SettingsCommand {
    Set {
        key: String,
        #[arg(action = clap::ArgAction::Set)]
        value: bool,
    },
}
#[derive(Subcommand)]
enum ShortcutCommand {
    Set {
        shortcut: String,
    },
    /// Resolve a key captured by the desktop interface, without opening a window.
    #[command(hide = true)]
    ResolveKey {
        #[arg(value_parser = clap::value_parser!(u32).range(8..=65535))]
        code: u32,
        key: u32,
    },
}
#[derive(Subcommand)]
enum HistoryCommand {
    Copy { id: String },
    Paste { id: String },
    Clear,
}
#[derive(Subcommand)]
enum UpdateCommand {
    /// Reload the installed Omarchy interface after an upgrade.
    RefreshUi,
    Check {
        #[arg(long)]
        json: bool,
    },
    Install {
        #[arg(long)]
        version: Option<String>,
    },
    #[command(hide = true)]
    Apply {
        #[arg(long)]
        version: Option<String>,
    },
    #[command(hide = true)]
    InstallArchive {
        file: PathBuf,
        #[arg(long)]
        version: String,
    },
}

#[derive(Subcommand)]
enum ModelCommand {
    /// Download and SHA256-verify the pinned model (separate from application updates).
    Download,
    /// Download and load the missing model in the running service.
    Setup,
    /// Print the configured model path.
    Path,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // A consumer closing `watch` or piping transcription into `head` is normal.
            if error
                .downcast_ref::<io::Error>()
                .is_some_and(|e| e.kind() == io::ErrorKind::BrokenPipe)
            {
                return ExitCode::SUCCESS;
            }
            eprintln!("just-speak: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    if matches!(cli.command, Commands::ExportIcon) {
        io::stdout().lock().write_all(desktop_assets::ICON)?;
        return Ok(());
    }
    // Control an already-running service even if settings were edited into an
    // invalid state. In particular, Escape must always be able to cancel.
    if let Some(result) = control_command(&cli.command) {
        return result;
    }
    let mut config = Config::load()?;
    if let Some(path) = cli.model_dir {
        config.model_dir = Some(if path.is_absolute() {
            path
        } else {
            env::current_dir()?.join(path)
        });
    }
    if let Some(threads) = cli.threads {
        config.num_threads = threads;
    }
    config.validate()?;
    match cli.command {
        Commands::Daemon => {
            if let Err(error) = desktop_assets::refresh() {
                eprintln!("just-speak: could not refresh launcher icon: {error:#}");
            }
            daemon::run(config)
        }
        Commands::Doctor => doctor(&config),
        Commands::Update { command } => match command {
            UpdateCommand::RefreshUi => ui_refresh::refresh().map(|_| ()),
            UpdateCommand::Check { json } => {
                let info = updater::check(&config.updates_repo)?;
                if json {
                    protocol::write_json(&mut io::stdout().lock(), &info)?;
                } else {
                    println!("{}", serde_json::to_string_pretty(&info)?);
                }
                Ok(())
            }
            UpdateCommand::Install { version } => {
                // The service outlives the menu that triggered it. Keep its output in
                // the journal: restarting the shell closes the menu's pipe collectors.
                let mut command = Command::new("systemd-run");
                command.args(["--user", "--collect", "--wait", "--unit=just-speak-update"]);
                for name in [
                    "JUST_SPEAK_PREFIX",
                    "XDG_CONFIG_HOME",
                    "XDG_DATA_HOME",
                    "XDG_STATE_HOME",
                    "XDG_RUNTIME_DIR",
                    "OMARCHY_PATH",
                    "WAYLAND_DISPLAY",
                    "HYPRLAND_INSTANCE_SIGNATURE",
                    "PATH",
                ] {
                    if let Some(value) = env::var_os(name) {
                        command
                            .arg("--setenv")
                            .arg(format!("{name}={}", value.to_string_lossy()));
                    }
                }
                command.arg(env::current_exe()?).args(["update", "apply"]);
                if let Some(version) = version {
                    command.arg("--version").arg(version);
                }
                ensure!(
                    command.status()?.success(),
                    "Update needs attention; files may already be installed. See `journalctl --user -u just-speak-update`"
                );
                Ok(())
            }
            UpdateCommand::Apply { version } => apply_update(&config, version.as_deref()),
            UpdateCommand::InstallArchive { file, version } => {
                let mut gated = false;
                let result = updater::install_archive(&file, &version, || {
                    if config::socket_path()?.exists() {
                        control(Request::BeginUpdate {})?;
                        gated = true;
                    }
                    Ok(())
                });
                if result.is_err() && gated {
                    let _ = control(Request::EndUpdate {});
                }
                protocol::write_json(&mut io::stdout().lock(), &result?)
            }
        },
        Commands::Model { command } => match command {
            ModelCommand::Path => {
                println!("{}", config.model_dir()?.display());
                Ok(())
            }
            ModelCommand::Download => download_model(&config.model_dir()?),
            ModelCommand::Setup => unreachable!("handled as a service command"),
        },
        Commands::Transcribe { file } => {
            let mut engine = inference::Engine::load(&config.model_dir()?, config.num_threads)?;
            println!("{}", engine.transcribe(&file)?);
            Ok(())
        }
        Commands::Benchmark {
            file,
            runs,
            minimum_speed,
            expected_text,
            json,
        } => benchmark(
            &config,
            &file,
            runs,
            minimum_speed,
            expected_text.as_deref(),
            json,
        ),
        _ => unreachable!("service commands are handled before configuration loading"),
    }
}

fn control_command(command: &Commands) -> Option<Result<()>> {
    Some(match command {
        Commands::Start => control(Request::Start {}),
        Commands::Stop => control(Request::Stop {}),
        Commands::Cancel => control(Request::Cancel {}),
        Commands::Status { json } => service_status(*json),
        Commands::Watch => watch(),
        Commands::Quit => quit_service(),
        Commands::Launch => user_service("start"),
        Commands::Window { record_shortcut } => open_window(*record_shortcut),
        Commands::Restart => restart_service(),
        Commands::Menu { json: _ } => menu(),
        Commands::Model {
            command: ModelCommand::Setup,
        } => control(Request::SetupModel {}),
        Commands::Input {
            command: InputCommand::Set { id },
        } => control(Request::SetInput {
            input: if id == "default" {
                None
            } else {
                Some(id.clone())
            },
        }),
        Commands::Settings {
            command: SettingsCommand::Set { key, value },
        } => control(Request::SetOption {
            key: key.clone(),
            value: *value,
        }),
        Commands::Shortcut {
            command: ShortcutCommand::Set { shortcut },
        } => control(Request::SetShortcut {
            shortcut: shortcut.clone(),
        }),
        Commands::Shortcut {
            command: ShortcutCommand::ResolveKey { code, key },
        } => {
            use std::os::unix::process::CommandExt;
            Err(Command::new("gjs")
                .arg("-c")
                .arg(include_str!("../gtk/shortcut-keymap.js"))
                .arg(code.to_string())
                .arg(key.to_string())
                .exec())
            .context("Resolve shortcut key (requires GJS and GTK4)")
        }
        Commands::History { command } => control(match command {
            HistoryCommand::Copy { id } => Request::HistoryCopy { id: id.clone() },
            HistoryCommand::Paste { id } => Request::HistoryPaste { id: id.clone() },
            HistoryCommand::Clear => Request::HistoryClear {},
        }),
        _ => return None,
    })
}

fn open_window(record_shortcut: bool) -> Result<()> {
    use std::os::unix::process::CommandExt;
    if let Err(error) = desktop_assets::refresh() {
        eprintln!("just-speak: could not refresh launcher icon: {error:#}");
    }
    let exe = env::current_exe()?;
    let prefix = env::var_os("JUST_SPEAK_PREFIX")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local")));
    let stable = prefix
        .as_ref()
        .map(|prefix| prefix.join("bin/just-speak"))
        .filter(|path| path.canonicalize().is_ok_and(|resolved| resolved == exe))
        .or_else(|| {
            let path = PathBuf::from("/usr/bin/just-speak");
            path.canonicalize()
                .is_ok_and(|resolved| resolved == exe)
                .then_some(path)
        })
        .unwrap_or_else(|| exe.clone());
    let mut candidates = Vec::new();
    // A checkout must use its matching frontend, not an older installed UI.
    if let Some(root) = exe.parent().and_then(Path::parent).and_then(Path::parent)
        && root.join("Cargo.toml").is_file()
    {
        candidates.push(root.join("gtk/main.js"));
    }
    if let Some(prefix) = prefix {
        candidates.push(prefix.join("share/just-speak/gtk/main.js"));
    }
    candidates.push(PathBuf::from("/usr/share/just-speak/gtk/main.js"));
    if let Some(root) = exe.parent().and_then(Path::parent).and_then(Path::parent) {
        candidates.push(root.join("gtk/main.js"));
    }
    let path = candidates
        .into_iter()
        .find(|path| path.is_file())
        .context("GTK window not installed; reinstall JustSpeak")?;
    let mut command = Command::new("gjs");
    command.arg("-m").arg(path).env("JUST_SPEAK_BIN", stable);
    if record_shortcut {
        // The Omarchy popup closes before launch. Its short-lived CLI child
        // must not own the dialog or keep the popup's output collectors open.
        command
            .arg("--record-shortcut")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .context("Open shortcut recorder (requires GJS and GTK4)")?;
        Ok(())
    } else {
        Err(command.exec()).context("Open GTK4 window (requires GJS and GTK4)")
    }
}

fn menu() -> Result<()> {
    let response = protocol::call(&config::socket_path()?, Request::Menu {})?;
    ensure!(
        response.ok,
        "{}",
        response.error.as_deref().unwrap_or("menu failed")
    );
    protocol::write_json(
        &mut io::stdout().lock(),
        &response.data.context("menu data missing")?,
    )
}

fn user_service(action: &str) -> Result<()> {
    ensure!(
        Command::new("systemctl")
            .args(["--user", action, "just-speak.service"])
            .status()?
            .success(),
        "Cannot {action} JustSpeak user service"
    );
    Ok(())
}

fn restart_service() -> Result<()> {
    if let Ok(response) = protocol::call(&config::socket_path()?, Request::Status {}) {
        ensure!(
            !response.status.can_cancel
                && !matches!(
                    response.status.phase,
                    protocol::Phase::Updating
                        | protocol::Phase::Stopping
                        | protocol::Phase::Canceling
                )
                && !response
                    .status
                    .model_setup
                    .is_some_and(protocol::ModelSetup::is_busy),
            "Wait for model setup or finish dictation before restarting"
        );
    }
    user_service("restart")
}

fn quit_service() -> Result<()> {
    match control(Request::Quit {}) {
        Ok(()) => {}
        // Quit remains useful when only the settings window is running.
        Err(error)
            if error.downcast_ref::<io::Error>().is_some_and(|error| {
                matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                )
            }) => {}
        Err(error) => return Err(error),
    }
    // GApplication routes this to the existing window, including when Quit
    // originated in the bar. No running window is a normal case.
    let _ = Command::new("gapplication")
        .args(["action", "io.github.zyuapp.JustSpeak", "quit"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    Ok(())
}

fn apply_update(config: &Config, version: Option<&str>) -> Result<()> {
    let mut gate = UpdateGate {
        socket: config::socket_path()?,
        active: false,
    };
    let result = updater::install(&config.updates_repo, version, || gate.begin())?;
    gate.restart(|| user_service("restart"))
        .context("Update installed, but JustSpeak could not restart. Try Restart again.")?;
    if Command::new("systemctl")
        .args([
            "--user",
            "is-active",
            "--quiet",
            "just-speak-overlay.service",
        ])
        .status()
        .is_ok_and(|s| s.success())
    {
        let _ = Command::new("systemctl")
            .args(["--user", "restart", "just-speak-overlay.service"])
            .status();
    }
    ui_refresh::refresh().context(
        "Update installed, but the Omarchy interface could not reload. Unlock the session if needed, then run `just-speak update refresh-ui`",
    )?;
    protocol::write_json(&mut io::stdout().lock(), &result)
}

struct UpdateGate {
    socket: PathBuf,
    active: bool,
}

impl UpdateGate {
    fn begin(&mut self) -> Result<()> {
        // Check at the point of installation: the user may have launched or
        // quit the service while the archive was being downloaded.
        match protocol::call(&self.socket, Request::BeginUpdate {}) {
            Ok(response) => {
                ensure!(
                    response.ok,
                    "{}",
                    response.error.as_deref().unwrap_or("Cannot begin update")
                );
                self.active = true;
            }
            Err(error)
                if error.downcast_ref::<io::Error>().is_some_and(|error| {
                    matches!(
                        error.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                    )
                }) => {}
            Err(error) => return Err(error),
        }
        Ok(())
    }

    fn restart(&mut self, restart: impl FnOnce() -> Result<()>) -> Result<()> {
        if self.active {
            restart()?;
            self.active = false;
        }
        Ok(())
    }
}

impl Drop for UpdateGate {
    fn drop(&mut self) {
        // A failed restart must release the old daemon's update gate too, so
        // recording, Quit and a manual Restart remain usable after the error.
        if self.active {
            let _ = protocol::call(&self.socket, Request::EndUpdate {});
        }
    }
}

fn service_status(json: bool) -> Result<()> {
    let response = protocol::call(&config::socket_path()?, Request::Status {})?;
    if json {
        protocol::write_json(&mut io::stdout().lock(), &response.status)?;
    } else {
        print_status(&response.status);
    }
    Ok(())
}

fn control(request: Request) -> Result<()> {
    let response = protocol::call(&config::socket_path()?, request)?;
    ensure!(
        response.ok,
        "{}",
        response.error.as_deref().unwrap_or("command failed")
    );
    Ok(())
}

fn print_status(status: &Status) {
    println!(
        "{}{}",
        serde_json::to_value(status.phase)
            .unwrap()
            .as_str()
            .unwrap(),
        status
            .message
            .as_ref()
            .map(|message| format!(": {message}"))
            .unwrap_or_default()
    );
}

fn watch() -> Result<()> {
    let stream = protocol::connect(&config::socket_path()?, Request::Watch {})?;
    let mut output = io::stdout().lock();
    for line in BufReader::new(stream).lines() {
        let status: Status = serde_json::from_str(&line?)?;
        protocol::write_json(&mut output, &status)?;
    }
    bail!("JustSpeak disconnected")
}

fn download_model(path: &Path) -> Result<()> {
    let mut child = Command::new("bash")
        .args(["-s", "--"])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .context("start model downloader")?;
    child
        .stdin
        .take()
        .context("downloader stdin missing")?
        .write_all(include_bytes!("../scripts/download-model.sh"))?;
    ensure!(
        child.wait()?.success(),
        "model download failed; see the downloader output above"
    );
    Ok(())
}

fn executable_exists(name: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    env::var_os("PATH").is_some_and(|paths| {
        env::split_paths(&paths).any(|directory| {
            directory
                .join(name)
                .metadata()
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
    })
}

fn doctor(config: &Config) -> Result<()> {
    let mut healthy = true;
    for program in ["pw-record", "pw-dump", "wl-copy"] {
        let exists = executable_exists(program);
        println!("{} {program}", if exists { "OK     " } else { "MISSING" });
        healthy &= exists;
    }
    if desktop::capabilities().experimental {
        println!(
            "NOTE    Experimental desktop integration: clipboard delivery only; global shortcuts and automatic paste are not verified on GNOME."
        );
    }
    let model_dir = config.model_dir()?;
    match inference::validate_model(&model_dir) {
        Ok(()) => println!("OK      model: {}", model_dir.display()),
        Err(error) => {
            println!("MISSING model: {error:#}\n        Run `just-speak model download`.");
            healthy = false;
        }
    }
    if config.paste {
        match desktop::check() {
            Ok(()) => println!("OK      desktop adapter: {}", desktop::capabilities().name),
            Err(error) => {
                println!("ERROR   desktop: {error:#}");
                healthy = false;
            }
        }
    }
    match protocol::call(&config::socket_path()?, Request::Status {}) {
        Ok(response) => {
            print!("OK      daemon: ");
            print_status(&response.status);
            healthy &= response.status.model_ready;
        }
        Err(_) => {
            println!("STOPPED daemon: run `just-speak daemon` or start the user service");
            healthy = false;
        }
    }
    println!(
        "Config: {}\nCPU threads: {}\nNo microphone recording or paste was performed.",
        config::config_path()?.display(),
        config.num_threads
    );
    ensure!(healthy, "some checks need attention");
    Ok(())
}

#[derive(Serialize)]
struct Benchmark {
    audio_seconds: f64,
    model_load_seconds: f64,
    first_inference_seconds: f64,
    resident_inference_seconds: Vec<f64>,
    slowest_resident_speed: f64,
    minimum_speed: f64,
    threshold_met: bool,
    num_threads: usize,
}

fn benchmark(
    config: &Config,
    path: &Path,
    runs: u32,
    minimum_speed: f64,
    expected: Option<&str>,
    json: bool,
) -> Result<()> {
    ensure!(
        minimum_speed.is_finite() && minimum_speed > 0.0,
        "minimum speed must be positive and finite"
    );
    let wav = hound::WavReader::open(path).context("open benchmark WAV")?;
    let duration = wav.duration() as f64 / wav.spec().sample_rate as f64;
    ensure!(
        duration >= 0.1,
        "benchmark needs at least 100 ms of real speech"
    );
    let start = Instant::now();
    let mut engine = inference::Engine::load(&config.model_dir()?, config.num_threads)?;
    let load = start.elapsed().as_secs_f64();
    let mut elapsed = Vec::new();
    for _ in 0..runs {
        let start = Instant::now();
        let text = engine.transcribe(path)?;
        elapsed.push(start.elapsed().as_secs_f64());
        ensure!(
            !text.trim().is_empty(),
            "benchmark produced an empty transcript"
        );
        if let Some(expected) = expected {
            ensure!(
                text.to_ascii_lowercase()
                    .contains(&expected.to_ascii_lowercase()),
                "benchmark transcript is missing the expected phrase"
            );
        }
    }
    let speed = duration / elapsed[1..].iter().copied().fold(0.0, f64::max);
    let result = Benchmark {
        audio_seconds: duration,
        model_load_seconds: load,
        first_inference_seconds: elapsed[0],
        resident_inference_seconds: elapsed[1..].to_vec(),
        slowest_resident_speed: speed,
        minimum_speed,
        threshold_met: speed >= minimum_speed,
        num_threads: config.num_threads,
    };
    if json {
        protocol::write_json(&mut io::stdout().lock(), &result)?;
    } else {
        println!(
            "Audio: {:.3}s; model load: {:.3}s; first inference: {:.3}s",
            duration, load, elapsed[0]
        );
        for (i, time) in elapsed[1..].iter().enumerate() {
            println!(
                "Resident {}: {:.3}s ({:.1}× real time)",
                i + 1,
                time,
                duration / time
            );
        }
        println!(
            "{}: {:.1}× resident speed; target {:.1}×",
            if result.threshold_met { "PASS" } else { "FAIL" },
            speed,
            minimum_speed
        );
    }
    ensure!(
        result.threshold_met,
        "resident inference missed the {minimum_speed:.1}× speed target ({speed:.1}× measured)"
    );
    Ok(())
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use std::{os::unix::net::UnixListener, thread, time::Duration};

    fn update_server(path: &Path, requests: Vec<(&'static str, bool)>) -> thread::JoinHandle<()> {
        let listener = UnixListener::bind(path).unwrap();
        listener.set_nonblocking(true).unwrap();
        thread::spawn(move || {
            for (expected, ok) in requests {
                let deadline = Instant::now() + Duration::from_secs(3);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "missing {expected} request");
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let request: serde_json::Value =
                    serde_json::from_str(&protocol::read_line(&stream).unwrap()).unwrap();
                assert_eq!(request["command"], expected);
                protocol::write_json(
                    &mut stream,
                    &protocol::Response {
                        ok,
                        status: Status::default(),
                        error: (!ok).then(|| "Finish dictation first".into()),
                        data: None,
                    },
                )
                .unwrap();
            }
        })
    }

    #[test]
    fn failed_restart_and_install_failure_both_release_the_service_update_gate() {
        for restart_fails in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let socket = root.path().join("service.sock");
            let server = update_server(&socket, vec![("begin_update", true), ("end_update", true)]);
            let mut gate = UpdateGate {
                socket,
                active: false,
            };
            gate.begin().unwrap();
            assert!(gate.active);
            if restart_fails {
                assert!(
                    gate.restart(|| bail!("service manager unavailable"))
                        .is_err()
                );
            }
            // Also covers an installer error after BeginUpdate but before restart.
            drop(gate);
            server.join().unwrap();
        }
    }

    #[test]
    fn successful_restart_does_not_release_a_new_daemons_unrelated_operation() {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("service.sock");
        let server = update_server(&socket, vec![("begin_update", true)]);
        let mut gate = UpdateGate {
            socket,
            active: false,
        };
        gate.begin().unwrap();
        gate.restart(|| Ok(())).unwrap();
        assert!(!gate.active);
        drop(gate);
        server.join().unwrap();
    }

    #[test]
    fn update_tolerates_a_stopped_service_but_preserves_a_busy_rejection() {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("service.sock");
        for stale_socket in [false, true] {
            if stale_socket {
                drop(UnixListener::bind(&socket).unwrap());
            }
            let mut gate = UpdateGate {
                socket: socket.clone(),
                active: false,
            };
            gate.begin().unwrap();
            gate.restart(|| panic!("stopped service must stay stopped"))
                .unwrap();
            assert!(!gate.active);
        }
        std::fs::remove_file(&socket).unwrap();
        let server = update_server(&socket, vec![("begin_update", false)]);
        let mut gate = UpdateGate {
            socket,
            active: false,
        };
        assert!(
            gate.begin()
                .unwrap_err()
                .to_string()
                .contains("Finish dictation")
        );
        assert!(!gate.active);
        drop(gate);
        server.join().unwrap();
    }
}
