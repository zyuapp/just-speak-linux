//! The last ten completed transcripts, stored locally with private permissions.

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const RETAIN: usize = 10;
const MAX_TEXT_BYTES: usize = 64 * 1024;
const MAX_FILE_BYTES: u64 = 1024 * 1024;
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub text: String,
    pub created_at: u64,
}

pub struct History {
    path: PathBuf,
    entries: Vec<Entry>,
}

impl History {
    pub fn load() -> Result<Self> {
        let state = env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
            .context("HOME or an absolute XDG_STATE_HOME is required for transcript history")?;
        Self::load_from(&state.join("just-speak/history.json"))
    }

    pub(crate) fn load_from(path: &Path) -> Result<Self> {
        let directory = path.parent().context("history path has no parent")?;
        fs::create_dir_all(directory).context("create private transcript history directory")?;
        let metadata = fs::symlink_metadata(directory)?;
        ensure!(
            metadata.is_dir() && metadata.uid() == unsafe { libc::geteuid() },
            "history directory must be owned by this user and cannot be a symlink"
        );
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    path: path.to_owned(),
                    entries: Vec::new(),
                });
            }
            Err(error) => return Err(error).context("open transcript history"),
        };
        let metadata = file.metadata()?;
        ensure!(
            metadata.is_file() && metadata.uid() == unsafe { libc::geteuid() },
            "history must be a regular file owned by this user"
        );
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        let entries = if bytes.len() as u64 <= MAX_FILE_BYTES {
            serde_json::from_slice::<Vec<Entry>>(&bytes)
                .ok()
                .filter(|entries| valid_entries(entries))
        } else {
            None
        };
        match entries {
            Some(entries) => Ok(Self {
                path: path.to_owned(),
                entries,
            }),
            None => {
                // Preserve recovery data without including transcript text in logs
                // or error messages, then let the next successful addition rebuild.
                let backup = directory.join(format!("history.corrupt-{}.json", unique_id()?));
                fs::rename(path, backup).context("preserve unreadable transcript history")?;
                Ok(Self {
                    path: path.to_owned(),
                    entries: Vec::new(),
                })
            }
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn add(&mut self, text: &str) -> Result<()> {
        if text.trim().is_empty() {
            return Ok(());
        }
        ensure!(
            text.len() <= MAX_TEXT_BYTES,
            "transcript is too large for recent history"
        );
        let mut entries = self.entries.clone();
        entries.insert(
            0,
            Entry {
                id: unique_id()?,
                text: text.to_owned(),
                created_at: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            },
        );
        entries.truncate(RETAIN);
        self.persist(&entries)?;
        self.entries = entries;
        Ok(())
    }

    pub fn clear(&mut self) -> Result<()> {
        self.persist(&[])?;
        self.entries.clear();
        // Clearing history also removes preserved recovery copies of transcripts.
        for entry in fs::read_dir(self.path.parent().context("history path has no parent")?)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("history.corrupt-") && name.ends_with(".json") {
                fs::remove_file(entry.path()).context("remove saved history recovery copy")?;
            }
        }
        Ok(())
    }

    fn persist(&self, entries: &[Entry]) -> Result<()> {
        let directory = self.path.parent().context("history path has no parent")?;
        let encoded = serde_json::to_vec(entries)?;
        ensure!(
            (encoded.len() as u64) < MAX_FILE_BYTES,
            "recent transcript history exceeds its size limit"
        );
        let mut temporary = tempfile::Builder::new()
            .prefix(".history-")
            .tempfile_in(directory)?;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
        temporary.write_all(&encoded)?;
        temporary.write_all(b"\n")?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(&self.path)
            .context("save recent transcript history")?;
        File::open(directory)?.sync_all()?;
        Ok(())
    }
}

fn unique_id() -> Result<String> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
    Ok(format!(
        "{:x}-{:x}",
        now.as_nanos(),
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    ))
}

fn valid_entries(entries: &[Entry]) -> bool {
    let mut ids = HashSet::new();
    entries.len() <= RETAIN
        && entries.iter().all(|entry| {
            !entry.id.is_empty()
                && entry.id.len() <= 128
                && entry
                    .id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && ids.insert(entry.id.as_str())
                && !entry.text.trim().is_empty()
                && entry.text.len() <= MAX_TEXT_BYTES
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_ten_newest_and_round_trips_unicode_with_private_permissions() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("state/history.json");
        let mut history = History::load_from(&path).unwrap();
        for index in 0..12 {
            history.add(&format!("Hello 世界 {index}")).unwrap();
        }
        assert_eq!(history.entries().len(), 10);
        assert_eq!(history.entries()[0].text, "Hello 世界 11");
        assert_eq!(history.entries()[9].text, "Hello 世界 2");
        let loaded = History::load_from(&path).unwrap();
        assert_eq!(loaded.entries(), history.entries());
        assert!(loaded.get(&loaded.entries()[0].id).is_some());
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(root.path().join("state"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }

    #[test]
    fn corruption_is_preserved_without_blocking_new_history() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("history.json");
        fs::write(&path, b"unfinished private transcript").unwrap();
        let mut history = History::load_from(&path).unwrap();
        assert!(history.entries().is_empty());
        assert!(!path.exists());
        let backups: Vec<_> = fs::read_dir(root.path()).unwrap().collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            fs::read(backups[0].as_ref().unwrap().path()).unwrap(),
            b"unfinished private transcript"
        );
        history.add("Recovered").unwrap();
        history.clear().unwrap();
        assert!(History::load_from(&path).unwrap().entries().is_empty());
    }

    #[test]
    fn invalid_or_unsaved_additions_leave_current_history_unchanged() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("history.json");
        let mut history = History::load_from(&path).unwrap();
        history.add(" ").unwrap();
        assert!(!path.exists());
        assert!(history.add(&"x".repeat(MAX_TEXT_BYTES + 1)).is_err());
        history.add("Keep me").unwrap();
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(history.add("Cannot persist").is_err());
        assert_eq!(history.entries()[0].text, "Keep me");
    }

    #[test]
    fn history_symlink_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("unrelated");
        fs::write(&target, "secret").unwrap();
        let path = root.path().join("history.json");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(History::load_from(&path).is_err());
        assert_eq!(fs::read_to_string(target).unwrap(), "secret");
    }
}
