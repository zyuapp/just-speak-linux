//! Managed Hyprland bindings. Only JustSpeak's marked block is rewritten.
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub fn normalize(value: &str) -> Result<String> {
    normalize_inner(value, false)
}

// Existing settings must remain loadable so users can replace an old shortcut.
pub fn normalize_existing(value: &str) -> Result<String> {
    normalize_inner(value, true)
}

fn normalize_inner(value: &str, allow_legacy_modifiers: bool) -> Result<String> {
    ensure!(value.len() <= 128, "shortcut is too long");
    let parts: Vec<_> = value.split('+').map(str::trim).collect();
    ensure!(
        !parts.is_empty() && parts.iter().all(|part| !part.is_empty()),
        "enter a key or a combination such as SUPER + F10"
    );
    let key = parts.last().unwrap().to_ascii_uppercase();
    ensure!(
        key.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
        "shortcut key must be a Hyprland key name"
    );
    let modifier_key = matches!(
        key.as_str(),
        "SUPER_L"
            | "SUPER_R"
            | "CONTROL_L"
            | "CONTROL_R"
            | "ALT_L"
            | "ALT_R"
            | "SHIFT_L"
            | "SHIFT_R"
            | "SUPER"
            | "CTRL"
            | "CONTROL"
            | "ALT"
            | "SHIFT"
            | "WIN"
            | "META"
    ) || key.starts_with("ISO_")
        || key.starts_with("META_")
        || key.starts_with("HYPER_");
    ensure!(
        allow_legacy_modifiers || !modifier_key,
        "modifier-only shortcuts cannot reliably stop recording; use F10 or a combination such as SUPER + F10"
    );
    let mut modifiers = Vec::new();
    for part in &parts[..parts.len() - 1] {
        let modifier = match part.to_ascii_uppercase().as_str() {
            "SUPER" | "WIN" | "META" => "SUPER",
            "CTRL" | "CONTROL" => "CTRL",
            "ALT" => "ALT",
            "SHIFT" => "SHIFT",
            _ => bail!("unsupported modifier {part}; use SUPER, CTRL, ALT, or SHIFT"),
        };
        ensure!(
            !modifiers.contains(&modifier),
            "duplicate shortcut modifier"
        );
        modifiers.push(modifier);
    }
    ensure!(
        key != "ESCAPE" && key != "ESC",
        "Escape is reserved for cancellation"
    );
    if modifiers.is_empty() {
        let function = key
            .strip_prefix('F')
            .and_then(|number| number.parse::<u8>().ok())
            .is_some_and(|number| (1..=35).contains(&number));
        ensure!(
            function
                || matches!(
                    key.as_str(),
                    "SUPER_L"
                        | "SUPER_R"
                        | "CONTROL_L"
                        | "CONTROL_R"
                        | "ALT_L"
                        | "ALT_R"
                        | "SHIFT_L"
                        | "SHIFT_R"
                        | "PAUSE"
                        | "INSERT"
                ),
            "use a function key or a modifier-key combination"
        );
    }
    modifiers.sort_by_key(|value| match *value {
        "SUPER" => 0,
        "CTRL" => 1,
        "ALT" => 2,
        _ => 3,
    });
    let mut normalized = modifiers
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    normalized.push(key);
    Ok(normalized.join(" + "))
}

fn reject_conflict(shortcut: &str, bindings: &Value) -> Result<()> {
    let parts: Vec<_> = shortcut.split(" + ").collect();
    let mask = parts[..parts.len() - 1].iter().fold(0, |mask, modifier| {
        mask | match *modifier {
            "SUPER" => 64,
            "CTRL" => 4,
            "ALT" => 8,
            "SHIFT" => 1,
            _ => 0,
        }
    });
    for binding in bindings
        .as_array()
        .context("Hyprland returned invalid bindings")?
    {
        if binding["modmask"].as_u64() != Some(mask)
            || !binding["key"]
                .as_str()
                .unwrap_or("")
                .eq_ignore_ascii_case(parts.last().unwrap())
        {
            continue;
        }
        if binding["submap"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
        {
            continue;
        }
        let description = binding["description"]
            .as_str()
            .unwrap_or("another application");
        ensure!(
            description.starts_with("JustSpeak:"),
            "{shortcut} is already assigned to {description}; choose another shortcut"
        );
    }
    Ok(())
}

fn lua_string(value: &str) -> String {
    let escaped = value
        .as_bytes()
        .iter()
        .map(|byte| format!("\\{byte:03}"))
        .collect::<String>();
    format!("\"{escaped}\"")
}

fn merge_block(original: &str, block: &str) -> Result<String> {
    let mut output = String::new();
    let mut inside = false;
    for line in original.lines() {
        if line.trim().starts_with("-- BEGIN JustSpeak") {
            ensure!(
                !inside,
                "nested JustSpeak binding markers; fix bindings.lua manually"
            );
            inside = true;
        } else if line.trim().starts_with("-- END JustSpeak") {
            ensure!(inside, "unmatched JustSpeak binding marker");
            inside = false;
        } else if !inside {
            output.push_str(line);
            output.push('\n');
        }
    }
    ensure!(!inside, "unclosed JustSpeak binding marker");
    Ok(format!(
        "{}\n\n-- BEGIN JustSpeak\n{block}\n-- END JustSpeak\n",
        output.trim_end()
    ))
}

fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let parent = path.parent().context("binding directory missing")?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(contents.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

pub struct Change {
    original: Vec<(PathBuf, Option<String>)>,
    committed: bool,
}

impl Change {
    pub fn commit(mut self) {
        self.committed = true;
    }
}
impl Drop for Change {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        for (path, content) in &self.original {
            if let Some(content) = content {
                let _ = write_atomic(path, content);
            } else {
                let _ = fs::remove_file(path);
            }
        }
        let _ = crate::inputs::output("hyprctl", &["reload"], Duration::from_secs(2), 65536);
    }
}

pub fn apply(value: &str) -> Result<Change> {
    let shortcut = normalize(value)?;
    let output = crate::inputs::output(
        "hyprctl",
        &["-j", "binds"],
        Duration::from_secs(2),
        1024 * 1024,
    )?;
    reject_conflict(&shortcut, &serde_json::from_slice(&output)?)?;
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME is missing")?);
    let config_root = crate::config::config_path()?
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let personal = config_root.join("hypr/bindings.lua");
    let managed = config_root.join("just-speak/bindings.lua");
    let original = fs::read_to_string(&personal).context("read your Hyprland bindings.lua")?;
    let executable = home.join(".local/bin/just-speak");
    let executable = if executable.exists() {
        executable
    } else {
        std::env::current_exe()?
    };
    let command = |action| {
        lua_string(&format!(
            "{} {action}",
            shell_quote(&executable.to_string_lossy())
        ))
    };
    let content = format!(
        "-- Managed by JustSpeak's shortcut menu.\nhl.unbind({key})\no.bind({key}, \"JustSpeak: start dictation\", {start})\no.bind({key}, \"JustSpeak: stop and paste\", {stop}, {{ release = true }})\no.bind(\"ESCAPE\", \"JustSpeak: cancel dictation\", {cancel}, {{ non_consuming = true }})\n",
        key = lua_string(&shortcut),
        start = command("start"),
        stop = command("stop"),
        cancel = command("cancel")
    );
    let merged = merge_block(
        &original,
        &format!("dofile({})", lua_string(&managed.to_string_lossy())),
    )?;
    let suffix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    fs::copy(
        &personal,
        personal.with_extension(format!("lua.just-speak-{suffix}.bak")),
    )?;
    let change = Change {
        original: vec![
            (managed.clone(), fs::read_to_string(&managed).ok()),
            (personal.clone(), Some(original)),
        ],
        committed: false,
    };
    write_atomic(&managed, &content)?;
    crate::inputs::output(
        "luac",
        &[
            "-p",
            managed.to_str().context("shortcut path must be UTF-8")?,
        ],
        Duration::from_secs(2),
        65536,
    )
    .context("validate shortcut Lua")?;
    write_atomic(&personal, &merged)?;
    crate::inputs::output("hyprctl", &["reload"], Duration::from_secs(2), 65536)?;
    let output =
        crate::inputs::output("hyprctl", &["configerrors"], Duration::from_secs(2), 65536)?;
    let errors = String::from_utf8_lossy(&output);
    ensure!(
        errors.trim().is_empty(),
        "Hyprland rejected the shortcut: {errors}"
    );
    Ok(change)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_shortcut_syntax_and_preserves_typing_keys() {
        assert_eq!(
            normalize("ctrl + super + f12").unwrap(),
            "SUPER + CTRL + F12"
        );
        assert_eq!(normalize_existing("Super_R").unwrap(), "SUPER_R");
        for value in [
            "",
            "A",
            "ESCAPE",
            "SUPER + ",
            "CTRL + CTRL + F10",
            "F10;exec bad",
            "SUPER + $(bad)",
        ] {
            assert!(normalize(value).is_err(), "{value}");
        }
    }
    #[test]
    fn rejects_modifier_only_shortcuts() {
        for key in [
            "Super_L",
            "Super_R",
            "Control_L",
            "Control_R",
            "Alt_L",
            "Alt_R",
            "Shift_L",
            "Shift_R",
            "ALT",
            "ISO_Level3_Shift",
        ] {
            assert!(normalize(key).is_err(), "{key}");
            assert!(normalize(&format!("CTRL + {key}")).is_err(), "CTRL + {key}");
        }
        assert_eq!(normalize("ALT + F10").unwrap(), "ALT + F10");
    }
    #[test]
    fn never_steals_voxtype_or_another_apps_binding() {
        let bindings = serde_json::json!([{"key":"F9","modmask":0,"submap":"","description":"Voxtype"},{"key":"F10","modmask":0,"submap":"","description":"JustSpeak: start dictation"}]);
        assert!(reject_conflict("F9", &bindings).is_err());
        assert!(reject_conflict("F10", &bindings).is_ok());
        assert!(reject_conflict("SUPER + F9", &bindings).is_ok());
    }
    #[test]
    fn only_replaces_the_owned_block() {
        let original = "-- personal\no.bind('F9', 'Voxtype', 'voxtype')\n-- BEGIN JustSpeak F10\nold\n-- END JustSpeak F10\n-- more personal\n";
        let merged = merge_block(original, "new").unwrap();
        assert!(merged.contains("o.bind('F9', 'Voxtype', 'voxtype')"));
        assert!(merged.contains("-- more personal"));
        assert!(!merged.contains("\nold\n"));
        assert_eq!(merge_block(&merged, "new").unwrap(), merged);
    }
}
