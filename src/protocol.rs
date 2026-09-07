use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::Duration,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Start {},
    Stop {},
    Cancel {},
    Status {},
    Watch {},
    Menu {},
    SetupModel {},
    SetInput { input: Option<String> },
    SetOption { key: String, value: bool },
    SetShortcut { shortcut: String },
    HistoryCopy { id: String },
    HistoryPaste { id: String },
    HistoryClear {},
    Quit {},
    BeginUpdate {},
    EndUpdate {},
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Loading,
    Idle,
    Recording,
    Transcribing,
    Canceling,
    Error,
    Updating,
    Stopping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSetup {
    Required,
    Downloading,
    Verifying,
    Extracting,
    Loading,
    Failed,
}

impl ModelSetup {
    pub fn is_busy(self) -> bool {
        !matches!(self, Self::Required | Self::Failed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub phase: Phase,
    pub message: Option<String>,
    pub elapsed_seconds: Option<f64>,
    pub model_ready: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_setup: Option<ModelSetup>,
    pub can_cancel: bool,
    #[serde(default)]
    pub shortcut: String,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            phase: Phase::Loading,
            message: Some("Loading the speech model".into()),
            elapsed_seconds: None,
            model_ready: false,
            model_setup: None,
            can_cancel: false,
            shortcut: "F10".into(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    pub status: Status,
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

pub fn read_line(reader: impl Read) -> Result<String> {
    let mut line = String::new();
    BufReader::new(reader.take(16 * 1024)).read_line(&mut line)?;
    if !line.ends_with('\n') {
        bail!("request is incomplete or exceeds 16 KiB");
    }
    Ok(line)
}

pub fn write_json(writer: &mut impl Write, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

pub fn connect(path: &Path, request: Request) -> Result<UnixStream> {
    let mut stream = UnixStream::connect(path).with_context(|| {
        "JustSpeak is not running. Start it with `just-speak daemon` or `systemctl --user start just-speak`"
    })?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    write_json(&mut stream, &request)?;
    Ok(stream)
}

pub fn call(path: &Path, request: Request) -> Result<Response> {
    // Quit replies only after microphone/download/model cleanup has completed.
    let timeout = if matches!(request, Request::Quit {}) {
        30
    } else {
        16
    };
    let stream = connect(path, request)?;
    stream.set_read_timeout(Some(Duration::from_secs(timeout)))?;
    let mut line = String::new();
    BufReader::new(stream.take(1024 * 1024)).read_line(&mut line)?;
    ensure_response_complete(&line)?;
    serde_json::from_str(&line).context("invalid response from JustSpeak")
}

fn ensure_response_complete(line: &str) -> Result<()> {
    if !line.ends_with('\n') {
        bail!("response incomplete or exceeds 1 MiB");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncated_and_oversized_requests_are_rejected() {
        assert!(read_line(b"{\"command\":\"start\"}".as_slice()).is_err());
        let payload = format!("{}\n", "x".repeat(16 * 1024));
        assert!(read_line(payload.as_bytes()).is_err());
        assert!(
            serde_json::from_str::<Request>("{\"command\":\"start\",\"shell\":\"anything\"}")
                .is_err()
        );
    }

    #[test]
    fn json_protocol_works_over_a_real_unix_socket() {
        let (mut sender, receiver) = UnixStream::pair().unwrap();
        write_json(&mut sender, &Request::Cancel {}).unwrap();
        let request: Request = serde_json::from_str(&read_line(receiver).unwrap()).unwrap();
        assert!(matches!(request, Request::Cancel {}));
    }
}
