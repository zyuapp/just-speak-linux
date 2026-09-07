//! Explicit GitHub release updates. Downloads and validation happen before the
//! caller briefly gates recording and we switch the installed payload pointer.

use anyhow::{Context, Result, ensure};
use flate2::read::GzDecoder;
use fs2::FileExt;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    env,
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::Read,
    os::unix::{ffi::OsStrExt, fs::MetadataExt, fs::PermissionsExt, fs::symlink},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};
use tempfile::NamedTempFile;

const CURRENT: &str = env!("CARGO_PKG_VERSION");
const TARGET: &str = "linux-x86_64";
const MAX_ARCHIVE: u64 = 128 * 1024 * 1024;
const MAX_EXPANDED: u64 = 256 * 1024 * 1024;
const MAX_METADATA: u64 = 1024 * 1024;
const STABLE_PATHS: [&str; 4] = [
    "bin/just-speak",
    "share/just-speak/ui",
    "share/just-speak/gtk",
    "share/licenses/just-speak-linux",
];
const REQUIRED_FILES: [&str; 14] = [
    "bin/just-speak",
    "share/just-speak/ui/manifest.json",
    "share/just-speak/ui/Widget.qml",
    "share/just-speak/ui/StatusFeed.qml",
    "share/just-speak/ui/MenuModel.qml",
    "share/just-speak/ui/UpdateModel.qml",
    "share/just-speak/ui/DictationMenu.qml",
    "share/just-speak/ui/RecordingOverlay.qml",
    "share/just-speak/ui/shell.qml",
    "share/just-speak/gtk/main.js",
    "share/just-speak/gtk/backend.js",
    "share/just-speak/gtk/shortcut-recorder.js",
    "share/licenses/just-speak-linux/LICENSE",
    "share/licenses/just-speak-linux/THIRD_PARTY_NOTICES.md",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub available: bool,
    pub release_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallResult {
    pub previous_version: String,
    pub installed_version: String,
    pub release_url: String,
    pub restart_required: bool,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: u32,
    version: String,
    target: String,
    files: Vec<String>,
}

pub fn check(repository: &str) -> Result<UpdateInfo> {
    let release = release(repository, None)?;
    let latest = release_version(&release)?;
    // A release is actionable only when both assets for this target exist.
    release_assets(repository, &release, &latest)?;
    Ok(UpdateInfo {
        current_version: CURRENT.into(),
        latest_version: latest.to_string(),
        available: is_newer(CURRENT, &latest)?,
        release_url: release.html_url,
    })
}

/// The callback runs after all network and archive work, immediately before
/// applying files. The caller must block new recordings until restart/failure.
pub fn install(
    repository: &str,
    version: Option<&str>,
    before_apply: impl FnOnce() -> Result<()>,
) -> Result<InstallResult> {
    ensure!(
        env::consts::ARCH == "x86_64",
        "release updates support x86_64 only"
    );
    let layout = Layout::installed()?;
    let _lock = layout.lock()?;
    let release = release(repository, version)?;
    let latest = release_version(&release)?;
    ensure!(
        is_newer(CURRENT, &latest)?,
        "release {latest} is not newer than {CURRENT}"
    );
    let (archive, checksums) = release_assets(repository, &release, &latest)?;
    let staging = tempfile::Builder::new()
        .prefix(".download-")
        .tempdir_in(&layout.updates)?;
    let archive_file = staging.path().join("release.tar.gz");
    let checksum_file = staging.path().join("SHA256SUMS");
    download(&archive.browser_download_url, &archive_file, MAX_ARCHIVE)?;
    download(
        &checksums.browser_download_url,
        &checksum_file,
        MAX_METADATA,
    )?;
    verify_checksum(
        &archive_file,
        &fs::read_to_string(checksum_file)?,
        &archive.name,
    )?;
    let payload = staging.path().join("payload");
    fs::create_dir(&payload)?;
    extract_payload(&archive_file, &payload, &latest)?;
    before_apply()?;
    layout.apply(&payload, &latest)?;
    Ok(InstallResult {
        previous_version: CURRENT.into(),
        installed_version: latest.to_string(),
        release_url: release.html_url,
        restart_required: true,
    })
}

/// Install an explicit local release archive, including the first installation.
/// The shell installer verifies its publisher-provided checksum before invoking
/// the downloaded executable. Every archive entry is validated again here.
pub fn install_archive(
    archive: &Path,
    expected_version: &str,
    before_apply: impl FnOnce() -> Result<()>,
) -> Result<InstallResult> {
    ensure!(
        env::consts::ARCH == "x86_64",
        "release installation supports x86_64 only"
    );
    let version = Version::parse(expected_version.trim_start_matches('v'))?;
    ensure!(
        version.to_string() == CURRENT,
        "bootstrap executable and release must have the same version"
    );
    ensure!(
        fs::metadata(archive)?.len() <= MAX_ARCHIVE,
        "release archive exceeds its size limit"
    );
    let layout = Layout::local_prefix(true)?;
    let _lock = layout.lock()?;
    let previous_version = if layout.updates.join("current/release.json").is_file() {
        let manifest: Manifest =
            serde_json::from_slice(&fs::read(layout.updates.join("current/release.json"))?)?;
        ensure!(
            version >= Version::parse(&manifest.version)?,
            "release installer refuses a downgrade"
        );
        manifest.version
    } else if layout.prefix.join("bin/just-speak").exists() {
        "legacy".into()
    } else {
        "not installed".into()
    };
    let staging = tempfile::Builder::new()
        .prefix(".bootstrap-")
        .tempdir_in(&layout.updates)?;
    let payload = staging.path().join("payload");
    fs::create_dir(&payload)?;
    extract_payload(archive, &payload, &version)?;
    before_apply()?;
    layout.apply(&payload, &version)?;
    Ok(InstallResult {
        previous_version,
        installed_version: version.to_string(),
        release_url: String::new(),
        restart_required: true,
    })
}

pub fn validate_repository(repository: &str) -> Result<()> {
    let parts: Vec<_> = repository.split('/').collect();
    ensure!(
        parts.len() == 2
            && parts.iter().all(|part| {
                !part.is_empty()
                    && part.len() <= 100
                    && *part != "."
                    && *part != ".."
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
            }),
        "updates repository must be an owner/repository GitHub name"
    );
    Ok(())
}

fn release(repository: &str, version: Option<&str>) -> Result<Release> {
    validate_repository(repository)?;
    let suffix = if let Some(version) = version {
        let version =
            Version::parse(version.trim_start_matches('v')).context("invalid update version")?;
        ensure!(
            version.pre.is_empty() && version.build.is_empty(),
            "only stable release versions are supported"
        );
        format!("tags/v{version}")
    } else {
        "latest".into()
    };
    let file = NamedTempFile::new()?;
    download(
        &format!("https://api.github.com/repos/{repository}/releases/{suffix}"),
        file.path(),
        MAX_METADATA,
    )
    .context(
        "cannot check GitHub releases (the repository may not have a published stable release yet)",
    )?;
    let release: Release = serde_json::from_slice(&fs::read(file.path())?)
        .context("invalid GitHub release metadata")?;
    ensure!(
        release
            .html_url
            .starts_with(&format!("https://github.com/{repository}/releases/tag/")),
        "unexpected release page URL"
    );
    Ok(release)
}

fn release_version(release: &Release) -> Result<Version> {
    ensure!(
        !release.draft && !release.prerelease,
        "draft and prerelease updates are not supported"
    );
    let tag = release
        .tag_name
        .strip_prefix('v')
        .context("release tag must start with v")?;
    let version = Version::parse(tag).context("release tag is not a semantic version")?;
    ensure!(
        version.pre.is_empty() && version.build.is_empty(),
        "release must have a stable semantic version"
    );
    Ok(version)
}

fn is_newer(current: &str, latest: &Version) -> Result<bool> {
    Ok(latest > &Version::parse(current)?)
}

fn release_assets<'a>(
    repository: &str,
    release: &'a Release,
    version: &Version,
) -> Result<(&'a Asset, &'a Asset)> {
    let expected = format!("just-speak-linux-v{version}-{TARGET}.tar.gz");
    let find = |name: &str, max: u64| -> Result<&'a Asset> {
        let matches: Vec<_> = release
            .assets
            .iter()
            .filter(|asset| asset.name == name)
            .collect();
        ensure!(
            matches.len() == 1,
            "release must contain exactly one {name} asset"
        );
        let asset = matches[0];
        ensure!(
            asset.size > 0 && asset.size <= max,
            "release asset {name} has an invalid size"
        );
        ensure!(
            asset.browser_download_url
                == format!("https://github.com/{repository}/releases/download/v{version}/{name}"),
            "unexpected download URL for {name}"
        );
        Ok(asset)
    };
    Ok((
        find(&expected, MAX_ARCHIVE)?,
        find("SHA256SUMS", MAX_METADATA)?,
    ))
}

fn download(url: &str, output: &Path, limit: u64) -> Result<()> {
    ensure!(url.starts_with("https://"), "update URLs must use HTTPS");
    let result = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--tlsv1.2",
            "--connect-timeout",
            "15",
            "--max-time",
            "180",
            "--retry",
            "1",
            "--max-filesize",
        ])
        .arg(limit.to_string())
        .arg("--output")
        .arg(output)
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .context("cannot run curl for the update download")?;
    ensure!(
        result.status.success(),
        "update download failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    ensure!(
        fs::metadata(output)?.len() <= limit,
        "update download exceeded its size limit"
    );
    Ok(())
}

fn verify_checksum(archive: &Path, checksums: &str, asset_name: &str) -> Result<()> {
    let mut matching = Vec::new();
    for line in checksums.lines() {
        let mut words = line.split_whitespace();
        if let (Some(digest), Some(name), None) = (words.next(), words.next(), words.next())
            && name.trim_start_matches('*') == asset_name
        {
            ensure!(
                digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()),
                "invalid release SHA256"
            );
            matching.push(digest.to_ascii_lowercase());
        }
    }
    ensure!(
        matching.len() == 1,
        "SHA256SUMS must contain exactly one checksum for {asset_name}"
    );
    let mut input = File::open(archive)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    ensure!(
        format!("{:x}", digest.finalize()) == matching[0],
        "release SHA256 verification failed"
    );
    Ok(())
}

fn allowed_file(path: &str) -> bool {
    if matches!(path, "release.json" | "bin/just-speak") {
        return true;
    }
    if let Some(name) = path.strip_prefix("share/just-speak/ui/") {
        return !name.contains('/')
            && (name.ends_with(".qml") || matches!(name, "manifest.json" | "just-speak.svg"));
    }
    if let Some(name) = path.strip_prefix("share/just-speak/gtk/") {
        return !name.contains('/')
            && (name.ends_with(".js") || name.ends_with(".css") || name.ends_with(".json"));
    }
    if path.starts_with("share/licenses/just-speak-linux/") {
        return true;
    }
    matches!(
        path,
        "share/just-speak/packaging/just-speak.service"
            | "share/just-speak/packaging/just-speak-overlay.service"
            | "share/just-speak/packaging/hyprland.lua"
            | "share/just-speak/packaging/README.md"
            | "share/just-speak/packaging/just-speak.desktop"
            | "share/just-speak/scripts/download-model.sh"
    )
}

fn extract_payload(archive: &Path, destination: &Path, version: &Version) -> Result<()> {
    let mut archive = tar::Archive::new(GzDecoder::new(File::open(archive)?));
    let mut paths = BTreeSet::new();
    let mut total = 0u64;
    for (index, entry) in archive.entries()?.enumerate() {
        ensure!(index < 512, "release archive has too many entries");
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        ensure!(
            path.components()
                .all(|part| matches!(part, Component::Normal(_))),
            "unsafe release archive path"
        );
        let name = path.to_str().context("release paths must be UTF-8")?;
        ensure!(
            name.len() <= 240
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"/._-".contains(&byte)),
            "invalid release archive filename"
        );
        ensure!(
            entry.header().entry_type().is_file(),
            "release archive must contain regular files only"
        );
        ensure!(
            allowed_file(name) && paths.insert(name.to_owned()),
            "unexpected or duplicate release file: {name}"
        );
        let size = entry.size();
        let maximum = if name == "bin/just-speak" {
            MAX_EXPANDED
        } else {
            8 * 1024 * 1024
        };
        ensure!(
            size > 0 && size <= maximum,
            "invalid release file size: {name}"
        );
        total = total
            .checked_add(size)
            .context("release archive size overflow")?;
        ensure!(
            total <= MAX_EXPANDED,
            "release archive expands beyond the size limit"
        );
        let output = destination.join(&path);
        fs::create_dir_all(output.parent().context("release file has no parent")?)?;
        let mut output_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)?;
        ensure!(
            std::io::copy(&mut entry, &mut output_file)? == size,
            "truncated release archive file"
        );
        output_file.sync_all()?;
        fs::set_permissions(
            &output,
            fs::Permissions::from_mode(
                if name == "bin/just-speak" || name.ends_with("download-model.sh") {
                    0o755
                } else {
                    0o644
                },
            ),
        )?;
    }
    ensure!(paths.remove("release.json"), "release manifest is missing");
    let manifest: Manifest = serde_json::from_slice(&fs::read(destination.join("release.json"))?)
        .context("invalid release manifest")?;
    ensure!(
        manifest.format == 1
            && manifest.target == TARGET
            && manifest.version == version.to_string(),
        "release manifest version or target does not match the selected release"
    );
    let listed: BTreeSet<_> = manifest.files.iter().cloned().collect();
    ensure!(
        listed.len() == manifest.files.len() && listed == paths,
        "release manifest file list does not match the archive"
    );
    for required in REQUIRED_FILES {
        ensure!(paths.contains(required), "release is missing {required}");
    }
    let mut header = [0; 20];
    File::open(destination.join("bin/just-speak"))?.read_exact(&mut header)?;
    ensure!(
        &header[..6] == b"\x7fELF\x02\x01" && header[18..20] == [62, 0],
        "release binary is not a Linux x86_64 ELF executable"
    );
    Ok(())
}

struct Layout {
    prefix: PathBuf,
    updates: PathBuf,
}

impl Layout {
    fn installed() -> Result<Self> {
        let layout = Self::local_prefix(false)?;
        let installed = fs::canonicalize(layout.prefix.join("bin/just-speak"))
            .context("local JustSpeak binary is missing")?;
        ensure!(
            fs::canonicalize(env::current_exe()?)? == installed,
            "this executable is not the local installation; use its installed binary or your package manager"
        );
        Ok(layout)
    }

    fn local_prefix(create: bool) -> Result<Self> {
        let prefix = env::var_os("JUST_SPEAK_PREFIX")
            .map(PathBuf::from)
            .unwrap_or(
                PathBuf::from(env::var_os("HOME").context("HOME is not set")?).join(".local"),
            );
        ensure!(
            prefix.is_absolute(),
            "local install prefix must be absolute"
        );
        ensure!(
            !["/usr", "/bin", "/sbin", "/lib", "/lib64"]
                .iter()
                .any(|system| prefix.starts_with(system)),
            "system installations must be updated using the package manager"
        );
        if create {
            fs::create_dir_all(&prefix)?;
        }
        let prefix = fs::canonicalize(prefix)
            .context("local installation not found; use the release installer first")?;
        ensure!(
            !["/usr", "/bin", "/sbin", "/lib", "/lib64"]
                .iter()
                .any(|system| prefix.starts_with(system)),
            "system installations must be updated using the package manager"
        );
        ensure!(
            fs::metadata(&prefix)?.uid() == unsafe { libc::geteuid() },
            "local install prefix is not owned by this user"
        );
        let updates = prefix.join("share/just-speak/updates");
        fs::create_dir_all(&updates)?;
        ensure!(
            !fs::symlink_metadata(&updates)?.file_type().is_symlink()
                && fs::canonicalize(&updates)?.starts_with(&prefix),
            "updates directory must remain inside the local install prefix"
        );
        Ok(Self { prefix, updates })
    }

    fn lock(&self) -> Result<File> {
        use std::os::unix::fs::OpenOptionsExt;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(self.updates.join("update.lock"))?;
        file.try_lock_exclusive()
            .context("another update is already running")?;
        Ok(file)
    }

    fn apply(&self, payload: &Path, version: &Version) -> Result<()> {
        let releases = self.updates.join("releases");
        fs::create_dir_all(&releases)?;
        ensure!(
            !fs::symlink_metadata(&releases)?.file_type().is_symlink(),
            "release storage must not be a symlink"
        );
        let fresh = !STABLE_PATHS
            .iter()
            .any(|relative| self.prefix.join(relative).exists())
            && !self.updates.join("current").exists();
        if !fresh {
            self.migrate()?;
        }
        let current = self.updates.join("current");
        let old = if fresh {
            None
        } else {
            Some(fs::read_link(&current).context("installed release pointer is invalid")?)
        };
        let reservation = tempfile::Builder::new()
            .prefix(&format!("v{version}-"))
            .tempdir_in(self.updates.join("releases"))?;
        let release_path = reservation.path().to_owned();
        fs::remove_dir(&release_path)?;
        fs::rename(payload, &release_path)?;
        let _ = reservation.keep();
        // All stable paths already point through current. One rename switches
        // the executable, UI, and notices together. A failure before this line
        // leaves the previous version fully active.
        if let Some(old) = &old {
            atomic_symlink(&self.updates.join("previous"), old)?;
        }
        let new_target = Path::new("releases").join(
            release_path
                .file_name()
                .context("release directory has no name")?,
        );
        let mut created_links = Vec::new();
        let commit = (|| -> Result<()> {
            if fresh {
                // Preflight every destination before writing any stable link.
                for relative in STABLE_PATHS {
                    let stable = self.prefix.join(relative);
                    fs::create_dir_all(stable.parent().context("stable path has no parent")?)?;
                    ensure!(
                        fs::symlink_metadata(&stable).is_err(),
                        "fresh install path already exists: {}",
                        stable.display()
                    );
                }
                for relative in STABLE_PATHS {
                    let stable = self.prefix.join(relative);
                    atomic_symlink(&stable, &current.join(relative))?;
                    created_links.push(stable);
                }
            }
            atomic_symlink(&current, &new_target)?;
            File::open(&self.updates)?.sync_all()?;
            Ok(())
        })();
        if let Err(error) = commit {
            if let Some(old) = &old {
                atomic_symlink(&current, old).context("update failed and rollback also failed")?;
            } else {
                if fs::symlink_metadata(&current).is_ok() {
                    fs::remove_file(&current)?;
                }
                for path in created_links {
                    fs::remove_file(path)?;
                }
            }
            return Err(error).context("update failed; previous installation restored");
        }
        Ok(())
    }

    fn migrate(&self) -> Result<()> {
        let current = self.updates.join("current");
        if current.exists() {
            let target = fs::read_link(&current).context("current release must be a symlink")?;
            ensure!(
                target.components().count() == 2
                    && target.parent() == Some(Path::new("releases"))
                    && target
                        .components()
                        .all(|part| matches!(part, Component::Normal(_)))
                    && fs::canonicalize(&current)?.parent()
                        == Some(fs::canonicalize(self.updates.join("releases"))?.as_path()),
                "current release pointer must reference an installed release"
            );
        }
        if !current.exists() {
            ensure!(
                fs::symlink_metadata(&current).is_err(),
                "current release link is broken"
            );
            let legacy = self
                .updates
                .join("releases")
                .join(format!("legacy-{CURRENT}"));
            if !legacy.exists() {
                let staged = tempfile::Builder::new()
                    .prefix(".legacy-")
                    .tempdir_in(&self.updates)?;
                for relative in STABLE_PATHS {
                    if relative == "share/just-speak/gtk" && !self.prefix.join(relative).exists() {
                        fs::create_dir_all(staged.path().join(relative))?;
                    } else {
                        copy_tree(&self.prefix.join(relative), &staged.path().join(relative))?;
                    }
                }
                fs::rename(staged.path(), &legacy)?;
            }
            atomic_symlink(&current, Path::new(&format!("releases/legacy-{CURRENT}")))?;
        }
        for relative in STABLE_PATHS {
            let stable = self.prefix.join(relative);
            let target = self.updates.join("current").join(relative);
            if fs::read_link(&stable).ok().as_deref() == Some(target.as_path()) {
                continue;
            }
            if !stable.exists() && fs::symlink_metadata(&stable).is_err() {
                atomic_symlink(&stable, &target)?;
                continue;
            }
            let backup = self
                .updates
                .join(format!("migration-backup-{}", relative.replace('/', "-")));
            ensure!(
                !backup.exists(),
                "migration backup already exists: {}",
                backup.display()
            );
            // Both paths resolve to the same old payload until migration is
            // complete. Exchange also handles a nonempty legacy UI directory.
            let temporary =
                NamedTempFile::new_in(stable.parent().context("stable path has no parent")?)?;
            let temporary_path = temporary.into_temp_path();
            fs::remove_file(&temporary_path)?;
            symlink(&target, &temporary_path)?;
            exchange_paths(&stable, &temporary_path)?;
            // Preserve original paths as recovery backups, never recursively
            // delete a legacy user directory as part of an update.
            fs::rename(&temporary_path, backup)?;
        }
        Ok(())
    }
}

fn atomic_symlink(path: &Path, target: &Path) -> Result<()> {
    let temporary = NamedTempFile::new_in(path.parent().context("link has no parent")?)?;
    let temporary = temporary.into_temp_path();
    fs::remove_file(&temporary)?;
    symlink(target, &temporary)?;
    fs::rename(&temporary, path)?;
    Ok(())
}

fn exchange_paths(first: &Path, second: &Path) -> Result<()> {
    let first = CString::new(first.as_os_str().as_bytes())?;
    let second = CString::new(second.as_os_str().as_bytes())?;
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            first.as_ptr(),
            libc::AT_FDCWD,
            second.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    ensure!(
        result == 0,
        "cannot migrate local install atomically: {}",
        std::io::Error::last_os_error()
    );
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("missing local installation asset {}", source.display()))?;
    ensure!(
        !metadata.file_type().is_symlink(),
        "legacy installation asset must not be a symlink: {}",
        source.display()
    );
    if metadata.is_dir() {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else {
        ensure!(
            metadata.is_file(),
            "unexpected local installation file type"
        );
        fs::create_dir_all(destination.parent().context("legacy asset has no parent")?)?;
        fs::copy(source, destination)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::GzEncoder};
    use std::io::Cursor;

    fn fake_elf() -> Vec<u8> {
        let mut bytes = vec![0; 20];
        bytes[..6].copy_from_slice(b"\x7fELF\x02\x01");
        bytes[18] = 62;
        bytes
    }

    fn fixture_files() -> Vec<(String, Vec<u8>)> {
        let mut files: Vec<_> = REQUIRED_FILES
            .iter()
            .map(|name| {
                (
                    name.to_string(),
                    if *name == "bin/just-speak" {
                        fake_elf()
                    } else {
                        b"fixture".to_vec()
                    },
                )
            })
            .collect();
        files.push((
            "share/just-speak/ui/just-speak.svg".into(),
            include_bytes!("../ui/just-speak.svg").to_vec(),
        ));
        let manifest = Manifest {
            format: 1,
            version: "0.3.0".into(),
            target: TARGET.into(),
            files: files.iter().map(|(name, _)| name.clone()).collect(),
        };
        files.push((
            "release.json".into(),
            serde_json::to_vec(&manifest).unwrap(),
        ));
        files
    }

    fn archive(path: &Path, files: &[(String, Vec<u8>)], symlink_entry: bool) {
        let mut builder = tar::Builder::new(GzEncoder::new(
            File::create(path).unwrap(),
            Compression::fast(),
        ));
        for (index, (name, data)) in files.iter().enumerate() {
            let mut header = tar::Header::new_gnu();
            // Direct header bytes allow adversarial traversal fixtures that
            // tar::Builder::append_data intentionally refuses to construct.
            header.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
            header.set_size(data.len() as u64);
            header.set_mode(0o777);
            header.set_entry_type(if symlink_entry && index == 0 {
                tar::EntryType::Symlink
            } else {
                tar::EntryType::Regular
            });
            header.set_cksum();
            builder.append(&header, Cursor::new(data)).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap();
    }

    #[test]
    fn semantic_versions_and_repository_names_are_validated() {
        assert!(is_newer("0.9.0", &Version::parse("0.10.0").unwrap()).unwrap());
        assert!(!is_newer("0.10.0", &Version::parse("0.9.0").unwrap()).unwrap());
        assert!(!is_newer("0.10.0", &Version::parse("0.10.0").unwrap()).unwrap());
        assert!(validate_repository("zyuapp/just-speak-linux").is_ok());
        for bad in [
            "",
            "foo",
            "https://github.com/foo/bar",
            "foo/../bar",
            "foo/bar?x",
            "foo/..",
        ] {
            assert!(validate_repository(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn checksum_requires_one_matching_untampered_archive() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("asset");
        fs::write(&file, b"original").unwrap();
        let digest = format!("{:x}", Sha256::digest(b"original"));
        let sums = format!("{digest}  release.tar.gz\n");
        assert!(verify_checksum(&file, &sums, "release.tar.gz").is_ok());
        assert!(verify_checksum(&file, &(sums.clone() + &sums), "release.tar.gz").is_err());
        assert!(verify_checksum(&file, &sums, "other.tar.gz").is_err());
        fs::write(&file, b"tampered").unwrap();
        assert!(verify_checksum(&file, &sums, "release.tar.gz").is_err());
    }

    #[test]
    fn extracts_complete_payload_with_restricted_modes() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("release.tar.gz");
        archive(&file, &fixture_files(), false);
        let destination = directory.path().join("payload");
        fs::create_dir(&destination).unwrap();
        extract_payload(&file, &destination, &Version::parse("0.3.0").unwrap()).unwrap();
        assert_eq!(
            fs::read(destination.join("share/just-speak/ui/just-speak.svg")).unwrap(),
            include_bytes!("../ui/just-speak.svg")
        );
        assert_eq!(
            fs::metadata(destination.join("bin/just-speak"))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o755
        );
        assert_eq!(
            fs::metadata(destination.join("release.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777,
            0o644
        );
    }

    #[test]
    fn rejects_archive_traversal_links_duplicates_and_unexpected_files() {
        for bad in [
            "../outside",
            "/tmp/outside",
            "share/../outside",
            "models/private",
            "bin/other",
            "bin/just-speak",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let file = directory.path().join("release.tar.gz");
            let mut files = fixture_files();
            files.push((bad.into(), b"unexpected".to_vec()));
            archive(&file, &files, false);
            assert!(
                extract_payload(&file, directory.path(), &Version::parse("0.3.0").unwrap())
                    .is_err(),
                "{bad}"
            );
        }
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("release.tar.gz");
        archive(&file, &fixture_files(), true);
        assert!(
            extract_payload(&file, directory.path(), &Version::parse("0.3.0").unwrap()).is_err()
        );
    }

    #[test]
    fn rejects_manifest_mismatch_missing_files_and_wrong_binary_architecture() {
        for variant in 0..3 {
            let directory = tempfile::tempdir().unwrap();
            let file = directory.path().join("release.tar.gz");
            let mut files = fixture_files();
            if variant == 1 {
                files.remove(1);
            }
            if variant == 2 {
                files[0].1[18] = 183;
            }
            archive(&file, &files, false);
            let version = Version::parse(if variant == 0 { "0.4.0" } else { "0.3.0" }).unwrap();
            assert!(extract_payload(&file, directory.path(), &version).is_err());
        }
    }

    fn legacy_layout(directory: &Path) -> Layout {
        let prefix = directory.join("prefix");
        for path in REQUIRED_FILES {
            let file = prefix.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, b"old payload").unwrap();
        }
        let updates = prefix.join("share/just-speak/updates");
        fs::create_dir_all(updates.join("releases")).unwrap();
        Layout { prefix, updates }
    }

    #[test]
    fn migration_and_atomic_switch_preserve_previous_payload_and_models() {
        let directory = tempfile::tempdir().unwrap();
        let layout = legacy_layout(directory.path());
        let model = layout.prefix.join("share/just-speak/models/preserved");
        fs::create_dir_all(model.parent().unwrap()).unwrap();
        fs::write(&model, b"model weights").unwrap();
        layout.migrate().unwrap();
        layout.migrate().unwrap(); // Safe to resume an interrupted migration.
        let old_pointer = fs::read_link(layout.updates.join("current")).unwrap();
        let payload = directory.path().join("payload");
        for path in REQUIRED_FILES {
            let file = payload.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, b"new payload").unwrap();
        }
        layout
            .apply(&payload, &Version::parse("0.3.0").unwrap())
            .unwrap();
        for path in REQUIRED_FILES {
            assert_eq!(fs::read(layout.prefix.join(path)).unwrap(), b"new payload");
            assert_eq!(
                fs::read(layout.updates.join("previous").join(path)).unwrap(),
                b"old payload"
            );
        }
        assert_eq!(
            fs::read_link(layout.updates.join("previous")).unwrap(),
            old_pointer
        );
        assert_eq!(fs::read(model).unwrap(), b"model weights");
    }

    #[test]
    fn failed_apply_leaves_legacy_version_active() {
        let directory = tempfile::tempdir().unwrap();
        let layout = legacy_layout(directory.path());
        assert!(
            layout
                .apply(
                    &directory.path().join("missing-payload"),
                    &Version::parse("0.3.0").unwrap()
                )
                .is_err()
        );
        for path in REQUIRED_FILES {
            assert_eq!(fs::read(layout.prefix.join(path)).unwrap(), b"old payload");
        }
    }
    #[test]
    fn first_install_creates_consistent_links_and_preserves_user_data() {
        let directory = tempfile::tempdir().unwrap();
        let prefix = directory.path().join("prefix");
        let updates = prefix.join("share/just-speak/updates");
        fs::create_dir_all(&updates).unwrap();
        let layout = Layout { prefix, updates };
        let payload = directory.path().join("payload");
        for path in REQUIRED_FILES {
            let file = payload.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, b"new payload").unwrap();
        }
        layout
            .apply(&payload, &Version::parse("0.3.0").unwrap())
            .unwrap();
        for path in REQUIRED_FILES {
            assert_eq!(fs::read(layout.prefix.join(path)).unwrap(), b"new payload");
        }
        assert!(!layout.updates.join("previous").exists());
    }

    #[test]
    fn failed_first_install_does_not_leave_dangling_stable_links() {
        let directory = tempfile::tempdir().unwrap();
        let prefix = directory.path().join("prefix");
        let updates = prefix.join("share/just-speak/updates");
        fs::create_dir_all(&updates).unwrap();
        let layout = Layout { prefix, updates };
        // Force a later stable path preflight to fail, after bin can be created.
        fs::write(layout.prefix.join("share/licenses"), b"occupied").unwrap();
        let payload = directory.path().join("payload");
        fs::create_dir(&payload).unwrap();
        assert!(
            layout
                .apply(&payload, &Version::parse("0.3.0").unwrap())
                .is_err()
        );
        assert!(fs::symlink_metadata(layout.prefix.join("bin/just-speak")).is_err());
        assert!(fs::symlink_metadata(layout.updates.join("current")).is_err());
        assert_eq!(
            fs::read(layout.prefix.join("share/licenses")).unwrap(),
            b"occupied"
        );
    }

    #[test]
    fn migrates_legacy_install_without_gtk() {
        let directory = tempfile::tempdir().unwrap();
        let layout = legacy_layout(directory.path());
        fs::remove_dir_all(layout.prefix.join("share/just-speak/gtk")).unwrap();
        layout.migrate().unwrap();
        assert!(layout.prefix.join("share/just-speak/gtk").is_dir());
        assert_eq!(
            fs::read(layout.prefix.join("bin/just-speak")).unwrap(),
            b"old payload"
        );
    }
    #[test]
    fn rejects_release_storage_and_current_links_outside_installation() {
        let directory = tempfile::tempdir().unwrap();
        let layout = legacy_layout(directory.path());
        let outside = directory.path().join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, layout.updates.join("current")).unwrap();
        assert!(layout.migrate().is_err());
        fs::remove_file(layout.updates.join("current")).unwrap();
        fs::remove_dir(layout.updates.join("releases")).unwrap();
        symlink(&outside, layout.updates.join("releases")).unwrap();
        assert!(
            layout
                .apply(
                    &directory.path().join("payload"),
                    &Version::parse("0.3.0").unwrap()
                )
                .is_err()
        );
        assert_eq!(
            fs::read(layout.prefix.join("bin/just-speak")).unwrap(),
            b"old payload"
        );
        assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
    }
}
