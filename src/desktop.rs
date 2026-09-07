//! Hyprland 0.56 Lua integration. Transcripts only travel through stdin to wl-copy.

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// Intentionally excludes window titles, which may contain private document text.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PasteTarget {
    pub address: String,
    pub class: String,
    #[serde(default, rename = "initialClass", alias = "initial_class")]
    pub initial_class: String,
    pub pid: u32,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub fn capture_target() -> Result<PasteTarget> {
    let output = run_command(Command::new("hyprctl").args(["-j", "activewindow"]), None)
        .context("read active Hyprland window")?;
    ensure!(
        output.status.success(),
        "cannot query Hyprland: {}",
        output.errors
    );
    parse_target(&output.stdout)
}

fn parse_target(json: &[u8]) -> Result<PasteTarget> {
    let target: PasteTarget = serde_json::from_slice(json)
        .context("no active application window; focus an application before recording")?;
    validate_target(&target)?;
    Ok(target)
}

fn validate_target(target: &PasteTarget) -> Result<()> {
    let address = target.address.strip_prefix("0x").unwrap_or_default();
    ensure!(
        !address.is_empty()
            && address.len() <= 16
            && address.bytes().all(|b| b.is_ascii_hexdigit())
            && u64::from_str_radix(address, 16).unwrap_or(0) != 0,
        "invalid or missing Hyprland window address"
    );
    ensure!(target.pid > 0, "active window has no process ID");
    ensure!(
        target.class.len() <= 4096,
        "active window class is too long"
    );
    Ok(())
}

pub fn copy(text: &str) -> Result<()> {
    ensure!(!text.trim().is_empty(), "nothing to copy");
    // wl-copy v2.3 forks only from did_set_selection_callback, after the clipboard
    // offer is established. Waiting for its parent is the readiness handshake.
    // https://github.com/bugaevc/wl-clipboard/blob/v2.3.0/src/wl-copy.c
    let output = run_command(
        Command::new("wl-copy").args(["--type", "text/plain;charset=utf-8"]),
        Some(text.as_bytes()),
    )
    .context("copy transcript with wl-copy")?;
    ensure!(output.status.success(), "wl-copy failed: {}", output.errors);
    Ok(())
}

/// Cancelable delivery for a background worker. A canceled generation never
/// requests a paste; clipboard ownership already established cannot be undone.
pub fn paste_if_current(
    text: &str,
    target: &PasteTarget,
    current: &AtomicU64,
    id: u64,
) -> Result<bool> {
    deliver_if_current(
        current,
        id,
        || copy(text),
        || {
            paste_copied(target)
                .context("transcript copied to clipboard; automatic paste was skipped or failed")
        },
    )
}

fn deliver_if_current(
    current: &AtomicU64,
    id: u64,
    copy: impl FnOnce() -> Result<()>,
    dispatch: impl FnOnce() -> Result<()>,
) -> Result<bool> {
    if id == 0 || current.load(Ordering::Acquire) != id {
        return Ok(false);
    }
    copy()?;
    if current.load(Ordering::Acquire) != id {
        return Ok(false);
    }
    // Requesting dispatch is the commit point: an already delivered keystroke
    // cannot be retracted. The compositor independently checks window identity.
    dispatch()?;
    Ok(true)
}

pub fn paste_copied(target: &PasteTarget) -> Result<()> {
    validate_target(target)?;
    let script = paste_script(target);
    let output = run_command(Command::new("hyprctl").args(["eval", &script]), None)
        .context("send paste shortcut to Hyprland")?;
    let response = String::from_utf8_lossy(&output.stdout);
    if response.contains("JUST_SPEAK_FOCUS_CHANGED") {
        bail!("focused application changed while transcribing");
    }
    ensure!(
        output.status.success() && response.trim() == "ok",
        "Hyprland could not send paste shortcut: {} {}",
        response.trim(),
        output.errors
    );
    Ok(())
}

/// Read-only capability check; does not alter clipboard, focus, or keyboard state.
pub fn check() -> Result<()> {
    let script = "assert(type(hl.get_active_window) == 'function' and type(hl.timer) == 'function' and type(hl.dsp.send_key_state) == 'function', 'JustSpeak requires Hyprland 0.56 with Lua configuration')";
    let output = run_command(Command::new("hyprctl").args(["eval", script]), None)?;
    let response = String::from_utf8_lossy(&output.stdout);
    ensure!(
        output.status.success() && response.trim() == "ok",
        "Hyprland Lua integration unavailable: {} {}",
        response.trim(),
        output.errors
    );
    let output = run_command(Command::new("wl-copy").arg("--version"), None)?;
    ensure!(
        output.status.success(),
        "wl-copy is unavailable: {}",
        output.errors
    );
    Ok(())
}

fn terminal(target: &PasteTarget) -> bool {
    if target
        .tags
        .iter()
        .any(|tag| tag.trim_end_matches('*') == "terminal")
    {
        return true;
    }
    [&target.class, &target.initial_class].iter().any(|class| {
        matches!(
            class.to_ascii_lowercase().as_str(),
            "alacritty"
                | "foot"
                | "footclient"
                | "kitty"
                | "ghostty"
                | "com.mitchellh.ghostty"
                | "wezterm"
                | "org.wezfurlong.wezterm"
                | "konsole"
                | "org.kde.konsole"
                | "xfce4-terminal"
                | "xterm"
                | "urxvt"
                | "st"
                | "tilix"
                | "com.gexperts.tilix"
                | "gnome-terminal"
                | "gnome-terminal-server"
                | "org.gnome.terminal"
                | "ptyxis"
                | "org.gnome.ptyxis"
                | "org.gnome.console"
                | "kgx"
        )
    })
}

/// Decimal byte escapes are valid Lua, including for UTF-8. JSON escaping alone
/// is insufficient: JSON's \uXXXX escape is not a Lua string escape.
fn lua_string(value: &str) -> String {
    let mut result = String::from("\"");
    for byte in value.bytes() {
        write!(&mut result, "\\{byte:03}").unwrap();
    }
    result.push('"');
    result
}

fn paste_script(target: &PasteTarget) -> String {
    let mods = if terminal(target) {
        "CTRL SHIFT"
    } else {
        "CTRL"
    };
    // Check focus and dispatch within one compositor callback, closing the race
    // between a client-side activewindow query and sending a shortcut.
    // Explicit down/up with a 50 ms timer follows installed Omarchy clipboard.lua
    // and avoids Hyprland's send_shortcut repeating/stuck-key issue (#14099).
    // Schedule release before key-down so it survives the daemon exiting.
    format!(
        r#"local w = hl.get_active_window()
if not w or w.address ~= {address} or w.class ~= {class} or w.pid ~= {pid} or not w.mapped or w.hidden then
    error('JUST_SPEAK_FOCUS_CHANGED')
end
local mods = '{mods}'
local release = hl.dsp.send_key_state({{ mods = mods, key = 'V', state = 'up', window = w }})
local press = hl.dsp.send_key_state({{ mods = mods, key = 'V', state = 'down', window = w }})
hl.timer(function() hl.dispatch(release) end, {{ timeout = 50, type = 'oneshot' }})
hl.dispatch(press)"#,
        address = lua_string(&target.address),
        class = lua_string(&target.class),
        pid = target.pid
    )
}

struct CommandOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    errors: String,
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

fn run_command(command: &mut Command, input: Option<&[u8]>) -> Result<CommandOutput> {
    // Anonymous 0600 temp files avoid pipe deadlocks, including wl-copy's forked
    // clipboard owner keeping stderr open. No transcript appears in argv.
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    command
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?);
    if let Some(input) = input {
        let mut stdin = tempfile::tempfile()?;
        stdin.write_all(input)?;
        stdin.rewind()?;
        command.stdin(stdin);
    } else {
        command.stdin(Stdio::null());
    }
    let mut child = ChildGuard(command.spawn().context("start desktop helper")?);
    let deadline = Instant::now() + Duration::from_secs(3);
    let status = loop {
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        ensure!(
            Instant::now() < deadline,
            "desktop helper timed out after 3 seconds"
        );
        thread::sleep(Duration::from_millis(5));
    };
    stdout.seek(SeekFrom::Start(0))?;
    stderr.seek(SeekFrom::Start(0))?;
    Ok(CommandOutput {
        status,
        stdout: read_limited(stdout, 65_536)?,
        errors: String::from_utf8_lossy(&read_limited(stderr, 4096)?)
            .trim()
            .to_string(),
    })
}

fn read_limited(file: File, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(class: &str) -> PasteTarget {
        PasteTarget {
            address: "0x1234".into(),
            class: class.into(),
            initial_class: String::new(),
            pid: 42,
            tags: vec![],
        }
    }

    #[test]
    fn parses_identity_without_retaining_window_title() {
        let target = parse_target(br#"{"address":"0x1234","class":"firefox","initialClass":"firefox","pid":42,"title":"private document","tags":[]}"#).unwrap();
        assert_eq!(target.class, "firefox");
        assert!(
            !serde_json::to_string(&target)
                .unwrap()
                .contains("private document")
        );
        assert!(parse_target(b"{}").is_err());
        let mut malicious = target;
        malicious.address = "0x1234; exec anything".into();
        assert!(validate_target(&malicious).is_err());
    }

    #[test]
    fn recognizes_terminal_classes_and_omarchy_tags() {
        for class in [
            "Alacritty",
            "com.mitchellh.ghostty",
            "org.wezfurlong.wezterm",
            "foot",
        ] {
            assert!(terminal(&target(class)), "{class}");
        }
        assert!(!terminal(&target("terminal-document-viewer")));
        assert!(!terminal(&target("firefox")));
        let mut tagged = target("custom-terminal");
        tagged.tags.push("terminal*".into());
        assert!(terminal(&tagged));
    }

    #[test]
    fn lua_escaping_preserves_utf8_and_untrusted_class_as_data() {
        assert_eq!(lua_string("\"\\\n\0"), "\"\\034\\092\\010\\000\"");
        assert_eq!(lua_string("é"), "\"\\195\\169\"");
        let malicious = "'); os.execute('bad'); --";
        assert!(!paste_script(&target(malicious)).contains(malicious));
    }

    #[test]
    fn cancellation_during_clipboard_handshake_suppresses_paste() {
        let current = AtomicU64::new(7);
        let delivered = deliver_if_current(
            &current,
            7,
            || {
                // Simulate Escape while wl-copy is waiting for the compositor.
                current.store(0, Ordering::Release);
                Ok(())
            },
            || panic!("canceled transcript must not be pasted"),
        )
        .unwrap();
        assert!(!delivered);

        assert!(
            !deliver_if_current(
                &current,
                7,
                || panic!("stale work must not overwrite the clipboard"),
                || panic!("stale work must not paste")
            )
            .unwrap()
        );
    }

    #[test]
    fn clipboard_failure_never_dispatches_paste() {
        let current = AtomicU64::new(7);
        let result = deliver_if_current(
            &current,
            7,
            || bail!("clipboard unavailable"),
            || panic!("paste requires successful clipboard offer"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn generated_lua_checks_focus_and_balances_key_events() {
        // Runs against a stub compositor, never the user's actual keyboard.
        // Lua is part of this project's Hyprland development environment.
        let script = paste_script(&target("firefox"));
        let harness = format!(
            r#"
local events, release_timer = {{}}, nil
local focused = {{address='0x1234', class='firefox', pid=42, mapped=true, hidden=false}}
hl = {{
  get_active_window = function() return focused end,
  dsp = {{ send_key_state = function(args) return args end }},
  dispatch = function(args) table.insert(events, args.state) end,
  timer = function(callback, options) assert(options.timeout == 50); release_timer = callback end,
}}
local paste = assert(load({script}))
focused.address = '0x4321'
local ok, err = pcall(paste)
assert(not ok and err:find('JUST_SPEAK_FOCUS_CHANGED'))
assert(#events == 0 and not release_timer)
focused.address = '0x1234'
paste()
assert(#events == 1 and events[1] == 'down')
focused = nil
release_timer()
assert(#events == 2 and events[2] == 'up')
"#,
            script = lua_string(&script)
        );
        let output = run_command(Command::new("lua").arg("-"), Some(harness.as_bytes())).unwrap();
        assert!(output.status.success(), "{}", output.errors);
    }
}
