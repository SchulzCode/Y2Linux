//! `power_journal.append_bounded`: one complete row appended to a private journal file on Y2DATA under
//! its lock, with one `.previous` rotation at the size limit. Used by the power journal
//! (`power-journal.jsonl`), the sleep diagnostics (`sleep-diagnostics.jsonl`) and the Wi-Fi journal.
// SPDX-License-Identifier: GPL-2.0-only
use crate::{lock, record, Root};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

/// What Python raises from `append_bounded`.
#[derive(Debug)]
pub enum AppendError {
    /// `ValueError(text)`.
    Value(&'static str),
    /// An `OSError` from a call given `path` (`None`: the error names no file, e.g. a busy flock).
    Os {
        error: io::Error,
        path: Option<PathBuf>,
    },
    /// `OSError(text)` raised by the code itself.
    OsMessage(&'static str),
}

impl AppendError {
    /// Python's `str(error)` and class name.
    pub fn python(&self) -> (String, &'static str) {
        match self {
            AppendError::Value(text) => ((*text).into(), "ValueError"),
            AppendError::OsMessage(text) => ((*text).into(), "OSError"),
            AppendError::Os { error, path } => crate::py::os_error(
                error,
                path.as_deref().map(|p| p.to_string_lossy()).as_deref(),
            ),
        }
    }
}

fn os(error: io::Error, path: &Path) -> AppendError {
    AppendError::Os {
        error,
        path: Some(path.to_owned()),
    }
}

/// `append_bounded(ctx, name, encoded, limit, durable)`: `name` is the device path of the journal; the
/// lock is `.<stem>.lock` beside it (flock `LOCK_EX|LOCK_NB`: a busy lock is `BlockingIOError`).
pub fn append_bounded(
    root: &Root,
    name: &str,
    encoded: &[u8],
    limit: u64,
    durable: bool,
) -> Result<(), AppendError> {
    let path = root.path(name);
    let parent = path.parent().unwrap_or(Path::new("/")).to_owned();
    record::private_directory(&parent).map_err(|e| match e.kind() {
        io::ErrorKind::InvalidData => AppendError::Value("state_directory_not_directory"),
        _ => os(e, &parent),
    })?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let lock_path = parent.join(format!(".{stem}.lock"));
    let _lock = match lock::try_exclusive(&lock_path) {
        Ok(Some(file)) => file,
        Ok(None) => {
            return Err(AppendError::Os {
                error: io::Error::from_raw_os_error(libc::EAGAIN),
                path: None,
            })
        }
        Err(error) => return Err(os(error, &lock_path)),
    };
    append(&path, encoded, limit, durable)
}

fn open(path: &Path, nonblock: bool) -> Result<File, AppendError> {
    OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(
            libc::O_NOFOLLOW | libc::O_CLOEXEC | if nonblock { libc::O_NONBLOCK } else { 0 },
        )
        .open(path)
        .map_err(|e| os(e, path))
}

fn append(path: &Path, encoded: &[u8], limit: u64, durable: bool) -> Result<(), AppendError> {
    let mut file = open(path, true)?;
    let meta = file.metadata().map_err(|e| AppendError::Os {
        error: e,
        path: None,
    })?;
    if !meta.file_type().is_file() {
        return Err(AppendError::Value("power_journal_not_regular"));
    }
    if meta.len() + encoded.len() as u64 > limit {
        drop(file);
        // `path.with_suffix('.previous.jsonl')`.
        let previous = path.with_extension("previous.jsonl");
        fs::rename(path, &previous).map_err(|e| os(e, path))?;
        file = open(path, false)?;
    }
    // Regular-file append is bounded; handle short writes without dropping a row.
    let mut remaining = encoded;
    while !remaining.is_empty() {
        match file.write(remaining) {
            Ok(0) => return Err(AppendError::OsMessage("power_journal_short_write")),
            Ok(n) => remaining = &remaining[n..],
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => {
                return Err(AppendError::Os {
                    error: e,
                    path: None,
                })
            }
        }
    }
    if durable {
        file.sync_all().map_err(|e| AppendError::Os {
            error: e,
            path: None,
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::root::tests::tempdir::Dir;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn rows_append_and_rotate_once() {
        let dir = Dir::new();
        let root = Root::new(dir.path());
        let name = "/data/system/platform/power-journal.jsonl";
        // A process another test forks keeps the lock's file description for a moment.
        let ok = |root: &Root, row: &[u8]| {
            for _ in 0..100 {
                match append_bounded(root, name, row, 20, true) {
                    Err(AppendError::Os { path: None, .. }) => {
                        std::thread::sleep(std::time::Duration::from_millis(10))
                    }
                    other => return other.unwrap(),
                }
            }
            panic!("journal lock stayed busy");
        };
        ok(&root, b"{\"a\":1}\n");
        ok(&root, b"{\"b\":2}\n");
        let journal = dir.path().join("data/system/platform/power-journal.jsonl");
        assert_eq!(
            fs::read_to_string(&journal).unwrap(),
            "{\"a\":1}\n{\"b\":2}\n"
        );
        assert_eq!(fs::metadata(&journal).unwrap().mode() & 0o777, 0o600);
        ok(&root, b"{\"c\":3}\n");
        assert_eq!(fs::read_to_string(&journal).unwrap(), "{\"c\":3}\n");
        assert_eq!(
            fs::read_to_string(
                dir.path()
                    .join("data/system/platform/power-journal.previous.jsonl")
            )
            .unwrap(),
            "{\"a\":1}\n{\"b\":2}\n"
        );
        assert!(dir
            .path()
            .join("data/system/platform/.power-journal.lock")
            .exists());
        let lock_path = dir.path().join("data/system/platform/.power-journal.lock");
        let held = (0..100)
            .find_map(|_| {
                let held = lock::try_exclusive(&lock_path).unwrap();
                if held.is_none() {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                held
            })
            .expect("lock free");
        let busy = append_bounded(&root, name, b"x\n", 20, false).unwrap_err();
        assert_eq!(
            busy.python(),
            (
                "[Errno 11] Resource temporarily unavailable".into(),
                "BlockingIOError"
            )
        );
        drop(held);
        fs::remove_file(&journal).unwrap();
        fs::create_dir(&journal).unwrap();
        assert!(append_bounded(&root, name, b"x\n", 20, false).is_err());
    }
}
