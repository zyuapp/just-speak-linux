//! Launcher artwork is embedded so old OTA validators can read new releases.
use anyhow::{Context, Result};
use std::{env, fs, io::Write, os::unix::fs::PermissionsExt, path::Path};

pub const ICON: &[u8] = include_bytes!("../ui/just-speak.svg");

pub fn refresh() -> Result<()> {
    let home = env::var_os("HOME").context("HOME is not set")?;
    let prefix = env::var_os("JUST_SPEAK_PREFIX")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new(&home).join(".local"));
    // Development and package-manager binaries must not rewrite a local install.
    if fs::canonicalize(prefix.join("bin/just-speak")).ok()
        != Some(fs::canonicalize(env::current_exe()?)?)
    {
        return Ok(());
    }
    let data = env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| Path::new(&home).join(".local/share"));
    refresh_in(&data)
}

fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    // Replace old v0.2.5 icon symlinks too, including dangling ones.
    if !path.is_symlink() && fs::read(path).ok().as_deref() == Some(bytes) {
        return Ok(());
    }
    let parent = path.parent().context("asset has no parent")?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o644))?;
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(())
}

fn refresh_in(data: &Path) -> Result<()> {
    replace(
        &data.join("icons/hicolor/scalable/apps/just-speak.svg"),
        ICON,
    )?;
    let launcher = data.join("applications/just-speak.desktop");
    match fs::read_to_string(&launcher) {
        Ok(text) => {
            // Preserve the installer's quoted Exec and any user customizations.
            let updated = text.replace("\nIcon=audio-input-microphone\n", "\nIcon=just-speak\n");
            if updated != text {
                replace(&launcher, updated.as_bytes())?;
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn migrates_legacy_launcher_and_replaces_dangling_icon_link() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path();
        let launcher = data.join("applications/just-speak.desktop");
        fs::create_dir_all(launcher.parent().unwrap()).unwrap();
        fs::write(&launcher, "[Desktop Entry]\nExec=\"/custom prefix/bin/just-speak\" window\nIcon=audio-input-microphone\nX-Custom=keep\n").unwrap();
        let icon = data.join("icons/hicolor/scalable/apps/just-speak.svg");
        fs::create_dir_all(icon.parent().unwrap()).unwrap();
        symlink(data.join("missing-old-release.svg"), &icon).unwrap();
        refresh_in(data).unwrap();
        assert!(!icon.is_symlink());
        assert_eq!(fs::read(&icon).unwrap(), ICON);
        let text = fs::read_to_string(&launcher).unwrap();
        assert!(text.contains("Icon=just-speak\n"));
        assert!(text.contains("Exec=\"/custom prefix/bin/just-speak\" window\n"));
        assert!(text.contains("X-Custom=keep\n"));
        let modified = fs::metadata(&icon).unwrap().modified().unwrap();
        refresh_in(data).unwrap();
        assert_eq!(fs::metadata(&icon).unwrap().modified().unwrap(), modified);
    }
}
