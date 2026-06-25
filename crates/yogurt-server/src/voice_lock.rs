//! One yogurt per Voice shortcut on this machine, whatever its data dir.
//! Every yogurt with Voice on installs its own keyboard tap, so without this one press of a shared shortcut records in all of them.
//! The lock is an `flock` the OS frees when the process exits or crashes, so a stale lock cannot outlive its owner.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use yogurt_voice::Hotkey;

/// Held while this yogurt answers the shortcut. Dropping it frees the shortcut for the next yogurt.
#[derive(Debug)]
pub struct ShortcutLock {
    id: String,
    _file: File,
}

impl ShortcutLock {
    pub fn is_for(&self, hotkey: &Hotkey) -> bool {
        self.id == hotkey.id()
    }
}

/// `~/.yogurt/voice-shortcuts`, shared by every data dir.
pub fn default_dir() -> anyhow::Result<PathBuf> {
    Ok(yogurt_db::paths::yogurt_dir()?.join("voice-shortcuts"))
}

/// `Ok(Err(port))` when another yogurt holds the shortcut, with the port it wrote, if it got that far.
pub fn claim(
    dir: &Path,
    hotkey: &Hotkey,
    port: u16,
) -> std::io::Result<Result<ShortcutLock, Option<u16>>> {
    std::fs::create_dir_all(dir)?;
    let id = hotkey.id();
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(format!("{id}.lock")))?;
    match file.try_lock() {
        Ok(()) => {
            file.set_len(0)?;
            file.write_all(port.to_string().as_bytes())?;
            Ok(Ok(ShortcutLock { id, _file: file }))
        }
        Err(TryLockError::WouldBlock) => {
            let mut owner = String::new();
            file.read_to_string(&mut owner)?;
            Ok(Err(owner.trim().parse().ok()))
        }
        Err(TryLockError::Error(e)) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_yogurt_waits_for_the_first_to_let_go() {
        let dir = tempfile::tempdir().unwrap();
        let key: Hotkey = "right_option+m".parse().unwrap();
        let first = claim(dir.path(), &key, 7879).unwrap().unwrap();
        assert!(first.is_for(&key));
        assert_eq!(
            claim(dir.path(), &key, 7880).unwrap().unwrap_err(),
            Some(7879)
        );
        let other: Hotkey = "option+a".parse().unwrap();
        assert!(claim(dir.path(), &other, 7880).unwrap().is_ok());
        drop(first);
        let second = claim(dir.path(), &key, 7880).unwrap().unwrap();
        drop(second);
        let third = claim(dir.path(), &key, 7881).unwrap().unwrap();
        let mut owner = String::new();
        File::open(dir.path().join(format!("{}.lock", key.id())))
            .unwrap()
            .read_to_string(&mut owner)
            .unwrap();
        assert_eq!(owner, "7881");
        drop(third);
    }
}
