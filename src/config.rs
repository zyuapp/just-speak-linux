use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub model_dir: Option<PathBuf>,
    pub input: Option<String>,
    pub num_threads: usize,
    pub paste: bool,
    pub max_recording_seconds: u64,
    pub shortcut: String,
    pub sound_feedback: bool,
    pub mute_while_recording: bool,
    pub history_enabled: bool,
    pub updates_repo: String,
    pub auto_check_updates: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model_dir: None,
            input: None,
            num_threads: std::thread::available_parallelism().map_or(4, |n| n.get().min(6)),
            paste: true,
            max_recording_seconds: 120,
            shortcut: "F10".into(),
            sound_feedback: true,
            mute_while_recording: true,
            history_enabled: true,
            updates_repo: "zyuapp/just-speak-linux".into(),
            auto_check_updates: true,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        let config = if path.exists() {
            toml::from_str(&fs::read_to_string(&path)?)
                .with_context(|| format!("invalid config {}", path.display()))?
        } else {
            Self::default()
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if !(1..=64).contains(&self.num_threads) {
            bail!("num_threads must be between 1 and 64");
        }
        if !(1..=120).contains(&self.max_recording_seconds) {
            bail!("max_recording_seconds must be between 1 and 120");
        }
        if self.model_dir.as_ref().is_some_and(|p| !p.is_absolute()) {
            bail!(
                "model_dir must be an absolute path (shell ~ expansion is not supported in TOML)"
            );
        }
        crate::shortcut::normalize_existing(&self.shortcut)?;
        Ok(())
    }

    /// Preserve comments and unrelated settings when changing a menu preference.
    pub fn save_preference(&self, key: &str) -> Result<()> {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        let path = config_path()?;
        let directory = path.parent().context("config directory missing")?;
        fs::create_dir_all(directory)?;
        let mut text = if path.exists() {
            fs::read_to_string(&path)?
        } else {
            String::new()
        };
        let values = toml::Value::try_from(self)?;
        let value = values.get(key);
        let replacement = value.map(|value| format!("{key} = {value}"));
        let mut found = false;
        let lines: Vec<String> = text
            .lines()
            .filter_map(|line| {
                if line
                    .split_once('=')
                    .is_some_and(|(name, _)| name.trim() == key)
                {
                    found = true;
                    replacement.clone()
                } else {
                    Some(line.to_owned())
                }
            })
            .collect();
        text = lines.join("\n");
        if !found && let Some(line) = replacement {
            text.push_str(&format!("\n{line}"));
        }
        text.push('\n');
        let decoded: Self = toml::from_str(&text)
            .context("existing config cannot be updated; fix its syntax first")?;
        decoded.validate()?;
        let mut staged = tempfile::NamedTempFile::new_in(directory)?;
        staged
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        staged.write_all(text.as_bytes())?;
        staged.as_file().sync_all()?;
        staged.persist(&path)?;
        Ok(())
    }

    pub fn model_dir(&self) -> Result<PathBuf> {
        if let Some(path) = &self.model_dir {
            return Ok(path.clone());
        }
        Ok(xdg_dir("XDG_DATA_HOME", ".local/share")?
            .join("just-speak/models/parakeet-tdt-0.6b-v2-int8"))
    }
}

fn xdg_dir(name: &str, fallback: &str) -> Result<PathBuf> {
    if let Some(path) = env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return Ok(path);
    }
    let home = env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(fallback))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(xdg_dir("XDG_CONFIG_HOME", ".config")?.join("just-speak/config.toml"))
}

pub fn socket_path() -> Result<PathBuf> {
    let runtime = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::geteuid() })));
    Ok(runtime.join("just-speak/control.sock"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unbounded_recording_and_misspelled_settings() {
        let config: Config = toml::from_str("max_recording_seconds = 0").unwrap();
        assert!(config.validate().is_err());
        assert!(toml::from_str::<Config>("thread_count = 4").is_err());
        assert!(toml::from_str::<Config>("num_threads = -1").is_err());
    }

    #[test]
    fn legacy_modifier_shortcut_remains_loadable_for_replacement() {
        let config: Config = toml::from_str("shortcut = \"ALT_R\"").unwrap();
        assert!(config.validate().is_ok());
        assert!(crate::shortcut::normalize(&config.shortcut).is_err());
    }

    #[test]
    fn relative_model_path_is_not_silently_resolved_against_service_directory() {
        let config: Config = toml::from_str("model_dir = '~/models'").unwrap();
        assert!(config.validate().is_err());
    }
}
