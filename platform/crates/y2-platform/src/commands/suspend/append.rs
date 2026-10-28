//! `power_journal.append_bounded`: one complete row appended to a private journal on Y2DATA under the
//! journal's own flock, rotated once (`<stem>.previous.jsonl`) when the row would take it past `limit`.
//! The suspend group's own copy (power-journal.jsonl, sleep-diagnostics.jsonl); the power group's
//! journal and the Wi-Fi journal follow the same protocol.
// SPDX-License-Identifier: GPL-2.0-only
use y2p_core::record::private_directory;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

/// What Python raised: an `OSError` (with its errno), or the `ValueError`s of the journal
/// (`power_journal_not_regular`, `state_directory_not_directory`), as `ErrorKind::InvalidData`.
pub type Result = io::Result<()>;

/// `.<stem>.lock` next to the journal (`power-journal.jsonl` -> `.power-journal.lock`).
pub fn lock_path(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    path.with_file_name(format!(".{stem}.lock"))
}

/// `path.with_suffix('.previous.jsonl')`.
pub fn previous_path(path: &Path) -> PathBuf {
    path.with_extension("previous.jsonl")
}

/// Append `encoded` (one JSON line with its newline) to `path`: `LOCK_EX|LOCK_NB` on the journal's lock
/// file (busy is `EWOULDBLOCK`, an `OSError` the callers count and drop), then `_append`.
pub fn append_bounded(path: &Path, encoded: &[u8], limit: u64, durable: bool) -> Result {
    let parent = path.parent().unwrap_or(Path::new("/"));
    private_directory(parent)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(lock_path(path))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(fs::TryLockError::WouldBlock) => {
            return Err(io::Error::from_raw_os_error(libc::EWOULDBLOCK))
        }
        Err(fs::TryLockError::Error(error)) => return Err(error),
    }
    let result = append(path, encoded, limit, durable);
    drop(lock);
    result
}

fn open(path: &Path, nonblock: bool) -> io::Result<File> {
    OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(
            libc::O_NOFOLLOW | libc::O_CLOEXEC | if nonblock { libc::O_NONBLOCK } else { 0 },
        )
        .open(path)
}

fn append(path: &Path, encoded: &[u8], limit: u64, durable: bool) -> Result {
    let mut file = open(path, true)?;
    let meta = file.metadata()?;
    if !meta.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "power_journal_not_regular",
        ));
    }
    if meta.len() + encoded.len() as u64 > limit {
        drop(file);
        fs::rename(path, previous_path(path))?;
        file = open(path, false)?;
    }
    // Regular-file append is bounded; a short write continues, never drops part of a row.
    let mut remaining = encoded;
    while !remaining.is_empty() {
        match file.write(remaining) {
            Ok(0) => return Err(io::Error::other("power_journal_short_write")),
            Ok(n) => remaining = &remaining[n..],
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    if durable {
        file.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tests::Dir;

    #[test]
    fn rows_append_and_rotate_once() {
        let dir = Dir::new();
        let path = dir.path().join("platform/power-journal.jsonl");
        append_bounded(&path, b"{\"a\":1}\n", 16, true).unwrap();
        append_bounded(&path, b"{\"b\":2}\n", 16, false).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{\"a\":1}\n{\"b\":2}\n");
        assert!(dir.path().join("platform/.power-journal.lock").exists());
        append_bounded(&path, b"{\"c\":3}\n", 16, false).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{\"c\":3}\n");
        assert_eq!(
            fs::read(dir.path().join("platform/power-journal.previous.jsonl")).unwrap(),
            b"{\"a\":1}\n{\"b\":2}\n"
        );
        let held = File::open(lock_path(&path)).unwrap();
        held.lock().unwrap();
        let busy = append_bounded(&path, b"x\n", 16, false).unwrap_err();
        assert_eq!(busy.raw_os_error(), Some(libc::EWOULDBLOCK));
    }
}
