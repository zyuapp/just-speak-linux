mod audio;
mod config;
mod daemon;
mod desktop;
mod inference;
mod protocol;

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
    about = "Offline push-to-talk dictation for Omarchy / Hyprland"
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
    /// Manage the separately downloaded speech model.
    Model {
        #[command(subcommand)]
        command: ModelCommand,
    },
    /// Transcribe a mono 16 kHz PCM16 WAV to stdout, without clipboard changes.
    Transcribe { file: PathBuf },
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
enum ModelCommand {
    /// Download and SHA256-verify the pinned model (the only network operation).
    Download,
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
        Commands::Daemon => daemon::run(config),
        Commands::Doctor => doctor(&config),
        Commands::Model { command } => match command {
            ModelCommand::Path => {
                println!("{}", config.model_dir()?.display());
                Ok(())
            }
            ModelCommand::Download => download_model(&config.model_dir()?),
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
        Commands::Start => control(Request::Start),
        Commands::Stop => control(Request::Stop),
        Commands::Cancel => control(Request::Cancel),
        Commands::Status { json } => service_status(*json),
        Commands::Watch => watch(),
        _ => return None,
    })
}

fn service_status(json: bool) -> Result<()> {
    let response = protocol::call(&config::socket_path()?, Request::Status)?;
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
    let stream = protocol::connect(&config::socket_path()?, Request::Watch)?;
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
    for program in ["pw-record", "wl-copy", "hyprctl"] {
        let exists = executable_exists(program);
        println!("{} {program}", if exists { "OK     " } else { "MISSING" });
        healthy &= exists;
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
            Ok(()) => println!("OK      Hyprland desktop connection"),
            Err(error) => {
                println!("ERROR   desktop: {error:#}");
                healthy = false;
            }
        }
    }
    match protocol::call(&config::socket_path()?, Request::Status) {
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
