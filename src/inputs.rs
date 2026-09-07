//! PipeWire source discovery; saved IDs are persistent node names, not object IDs.

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashSet,
    io::{Read, Seek},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Input {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

pub fn list() -> Result<Vec<Input>> {
    Ok(parse_inputs(&snapshot()?))
}

/// Call before recording when an explicit microphone is configured. An absent
/// device is an error, so PipeWire cannot silently substitute another source.
pub fn validate_selected(id: &str) -> Result<()> {
    ensure!(
        list()?.iter().any(|input| input.id == id),
        "selected microphone is unavailable; reconnect it or choose another microphone"
    );
    Ok(())
}

pub(crate) fn snapshot() -> Result<Vec<Value>> {
    let bytes = output(
        "pw-dump",
        &["--no-colors"],
        Duration::from_secs(2),
        8 * 1024 * 1024,
    )
    .context("read PipeWire audio devices")?;
    let objects: Vec<Value> =
        serde_json::from_slice(&bytes).context("invalid PipeWire device response")?;
    ensure!(
        objects.len() <= 32_768,
        "PipeWire device response has too many objects"
    );
    Ok(objects)
}

pub(crate) fn default_node_name(objects: &[Value], key: &str) -> Option<String> {
    objects
        .iter()
        .filter(|object| {
            object["type"] == "PipeWire:Interface:Metadata"
                && object["props"]["metadata.name"] == "default"
        })
        .filter_map(|object| object["metadata"].as_array())
        .flatten()
        .filter(|entry| entry["key"] == key && entry["subject"].as_u64() == Some(0))
        .find_map(|entry| {
            let value = &entry["value"];
            value["name"].as_str().map(str::to_owned).or_else(|| {
                let decoded: Value = serde_json::from_str(value.as_str()?).ok()?;
                decoded["name"].as_str().map(str::to_owned)
            })
        })
}

fn parse_inputs(objects: &[Value]) -> Vec<Input> {
    let default = default_node_name(objects, "default.audio.source");
    let mut ids = HashSet::new();
    let mut inputs = Vec::new();
    for object in objects {
        if object["type"] != "PipeWire:Interface:Node" {
            continue;
        }
        let props = &object["info"]["props"];
        let class = props["media.class"].as_str().unwrap_or_default();
        if !matches!(class, "Audio/Source" | "Audio/Source/Virtual") {
            continue;
        }
        if props["node.disabled"] == true
            || props["node.hidden"] == true
            || props["stream.capture.sink"] == true
            || props["device.class"] == "monitor"
        {
            continue;
        }
        let Some(id) = props["node.name"].as_str() else {
            continue;
        };
        if id.is_empty()
            || id.len() > 1024
            || id.chars().any(char::is_control)
            || id.ends_with(".monitor")
            || !ids.insert(id.to_owned())
        {
            continue;
        }
        let name = ["node.description", "node.nick", "device.description"]
            .iter()
            .filter_map(|key| props[*key].as_str())
            .find(|name| !name.trim().is_empty())
            .unwrap_or(id)
            .chars()
            .filter(|character| !character.is_control())
            .take(256)
            .collect();
        inputs.push(Input {
            id: id.to_owned(),
            name,
            is_default: default.as_deref() == Some(id),
        });
    }
    inputs.sort_by(|left, right| {
        right
            .is_default
            .cmp(&left.is_default)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.id.cmp(&right.id))
    });
    inputs
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// Bounded helpers with file-backed output, avoiding pipe deadlocks. All command
/// arguments are passed directly; device names are never interpreted by a shell.
pub(crate) fn output(
    program: &str,
    args: &[&str],
    timeout: Duration,
    limit: u64,
) -> Result<Vec<u8>> {
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let mut child = ChildGuard(
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(stdout.try_clone()?)
            .stderr(stderr.try_clone()?)
            .spawn()
            .with_context(|| format!("start {program}"))?,
    );
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        ensure!(Instant::now() < deadline, "{program} timed out");
        thread::sleep(Duration::from_millis(5));
    };
    if !status.success() {
        stderr.rewind()?;
        let mut error = String::new();
        stderr.take(4096).read_to_string(&mut error)?;
        anyhow::bail!("{program} failed: {}", error.trim());
    }
    ensure!(
        stdout.metadata()?.len() <= limit,
        "{program} response exceeds its size limit"
    );
    stdout.rewind()?;
    let mut bytes = Vec::new();
    stdout.take(limit).read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn node(runtime_id: u32, id: &str, name: &str, class: &str) -> Value {
        json!({"id":runtime_id,"type":"PipeWire:Interface:Node", "info":{"props":{
            "node.name":id,"node.description":name,"media.class":class}}})
    }

    #[test]
    fn source_picker_preserves_names_and_excludes_playback_and_monitor_nodes() {
        let objects = vec![
            node(47, "alsa_input.usb-mic", "USB microphone", "Audio/Source"),
            node(
                48,
                "noise-suppressed",
                "Clean voice",
                "Audio/Source/Virtual",
            ),
            node(49, "alsa_output.speakers", "Speakers", "Audio/Sink"),
            node(
                50,
                "alsa_output.speakers.monitor",
                "Speakers monitor",
                "Audio/Source",
            ),
            node(51, "hidden-internal", "Internal", "Audio/Source/Internal"),
            json!({"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"default"},
                "metadata":[{"subject":0,"key":"default.audio.source","value":{"name":"alsa_input.usb-mic"}}]}),
        ];
        let inputs = parse_inputs(&objects);
        assert_eq!(inputs.len(), 2);
        assert_eq!(
            inputs[0],
            Input {
                id: "alsa_input.usb-mic".into(),
                name: "USB microphone".into(),
                is_default: true
            }
        );
        let mut replaced = objects.clone();
        replaced[0]["id"] = json!(999);
        assert_eq!(parse_inputs(&replaced), inputs);
    }

    #[test]
    fn default_metadata_supports_serialized_values_and_ignores_other_stores() {
        let objects = vec![
            json!({"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"unrelated"},
                "metadata":[{"subject":0,"key":"default.audio.source","value":{"name":"wrong"}}]}),
            json!({"type":"PipeWire:Interface:Metadata","props":{"metadata.name":"default"},
                "metadata":[{"subject":0,"key":"default.audio.source","value":"{\"name\":\"real-mic\"}"}]}),
        ];
        assert_eq!(
            default_node_name(&objects, "default.audio.source").as_deref(),
            Some("real-mic")
        );
        assert_eq!(default_node_name(&objects, "default.audio.sink"), None);
    }

    #[test]
    fn helper_deadlines_and_output_limits_are_enforced() {
        let start = Instant::now();
        assert!(
            output(
                "sh",
                &["-c", "exec sleep 30"],
                Duration::from_millis(20),
                100
            )
            .is_err()
        );
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(output("printf", &["too much output"], Duration::from_secs(1), 4).is_err());
    }
}
