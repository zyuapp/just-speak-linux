//! Run the verified downloader independently of UI clients, with bounded
//! diagnostics and process-group cleanup when the service shuts down.

use crate::protocol::ModelSetup;
use anyhow::{Context, Result, bail, ensure};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::process::CommandExt,
    path::Path,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

pub fn download(
    destination: &Path,
    shutdown: &AtomicBool,
    progress: impl FnMut(ModelSetup),
) -> Result<()> {
    run_script(
        include_bytes!("../scripts/download-model.sh"),
        destination,
        shutdown,
        progress,
    )
}

fn run_script(
    script: &[u8],
    destination: &Path,
    shutdown: &AtomicBool,
    mut progress: impl FnMut(ModelSetup),
) -> Result<()> {
    let temporary = tempfile::tempdir().context("create model download status directory")?;
    let progress_path = temporary.path().join("progress");
    let diagnostics_path = temporary.path().join("errors");
    let mut process = DownloadChild(
        Command::new("bash")
            .args(["-s", "--"])
            .arg(destination)
            .env("JUST_SPEAK_MODEL_PROGRESS_FILE", &progress_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(File::create(&diagnostics_path)?)
            .process_group(0)
            .spawn()
            .context("start model downloader")?,
    );
    process
        .0
        .stdin
        .take()
        .context("downloader stdin missing")?
        .write_all(script)?;
    let mut previous = None;
    loop {
        ensure!(
            !shutdown.load(Ordering::Relaxed),
            "Model download interrupted"
        );
        let state = match fs::read_to_string(&progress_path)
            .unwrap_or_default()
            .trim()
        {
            "downloading" => Some(ModelSetup::Downloading),
            "verifying" => Some(ModelSetup::Verifying),
            "extracting" => Some(ModelSetup::Extracting),
            _ => None,
        };
        if let Some(state) = state
            && previous != Some(state)
        {
            previous = Some(state);
            progress(state);
        }
        if let Some(status) = process.0.try_wait()? {
            if status.success() {
                return Ok(());
            }
            let mut diagnostics = File::open(diagnostics_path)?;
            let length = diagnostics.metadata()?.len();
            diagnostics.seek(SeekFrom::Start(length.saturating_sub(1200)))?;
            let mut tail = Vec::new();
            diagnostics.take(1200).read_to_end(&mut tail)?;
            let message = String::from_utf8_lossy(&tail);
            bail!(
                "{}",
                if message.trim().is_empty() {
                    "Could not download the speech model."
                } else {
                    message.trim()
                }
            );
        }
        thread::sleep(Duration::from_millis(100));
    }
}

struct DownloadChild(Child);

impl Drop for DownloadChild {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(Some(_))) {
            return;
        }
        // SAFETY: this is our unreaped child and the leader of its own process
        // group. Signal curl/extraction too, so the shell can clean its staging.
        unsafe { libc::kill(-(self.0.id() as libc::pid_t), libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if matches!(self.0.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        unsafe { libc::kill(-(self.0.id() as libc::pid_t), libc::SIGKILL) };
        let _ = self.0.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downloader_reports_stages_and_preserves_failure_details() {
        let shutdown = AtomicBool::new(false);
        let mut stages = Vec::new();
        let result = run_script(
            b"printf 'verifying\\n' > \"$JUST_SPEAK_MODEL_PROGRESS_FILE\"\nsleep 0.3\nprintf 'Checksum mismatch\\n' >&2\nexit 1\n",
            Path::new("/unused"),
            &shutdown,
            |stage| stages.push(stage),
        );
        assert_eq!(stages, vec![ModelSetup::Verifying]);
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Checksum mismatch")
        );
    }

    #[test]
    fn shutdown_interrupts_download_and_allows_shell_cleanup() {
        let temporary = tempfile::tempdir().unwrap();
        let marker = temporary.path().join("cleaned-up");
        let shutdown = AtomicBool::new(false);
        let start = Instant::now();
        let result = run_script(
            b"trap 'touch \"$1\"; exit 143' TERM\nprintf 'downloading\\n' > \"$JUST_SPEAK_MODEL_PROGRESS_FILE\"\nsleep 60\n",
            &marker,
            &shutdown,
            |_| shutdown.store(true, Ordering::Relaxed),
        );
        assert!(result.is_err());
        assert!(start.elapsed() < Duration::from_secs(5));
        assert!(marker.exists(), "the shell must run its cleanup trap");
    }
}
