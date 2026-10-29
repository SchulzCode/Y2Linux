//! The platform's flock protocol: a lock file opened `O_RDWR|O_CREAT|O_NOFOLLOW` 0600 and locked with
//! flock(2), so Python and Rust holders exclude each other on the same path (plan §3.3). The lock is
//! released when the returned file is dropped or the process ends.
// SPDX-License-Identifier: GPL-2.0-only
use std::{
    fs::{File, OpenOptions, TryLockError},
    io,
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

fn open(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
}

/// `fcntl.flock(fd, LOCK_EX)`: wait for the lock.
pub fn exclusive(path: &Path) -> io::Result<File> {
    let file = open(path)?;
    file.lock()?;
    Ok(file)
}

/// `fcntl.flock(fd, LOCK_EX | LOCK_NB)`: `Ok(None)` while another process holds it.
pub fn try_exclusive(path: &Path) -> io::Result<Option<File>> {
    settle(|| {
        let file = open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(file)),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(error),
        }
    })
}

/// `fcntl.flock(fd, LOCK_SH | LOCK_NB)`: a shared lease (the activity lease's holders).
pub fn try_shared(path: &Path) -> io::Result<Option<File>> {
    settle(|| {
        let file = open(path)?;
        match file.try_lock_shared() {
            Ok(()) => Ok(Some(file)),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(error),
        }
    })
}

/// One attempt on the player. In test builds (feature `testing`) a busy lock is tried again for up to
/// 0.2 s: a test thread that forks a child shares every open descriptor until the child execs, which
/// briefly holds another test's lock (O_CLOEXEC closes it only at the exec).
fn settle(mut attempt: impl FnMut() -> io::Result<Option<File>>) -> io::Result<Option<File>> {
    #[cfg(feature = "testing")]
    for _ in 0..20 {
        if let Some(file) = attempt()? {
            return Ok(Some(file));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    attempt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::root::tests::tempdir::Dir;

    #[test]
    fn another_holder_excludes_this_one() {
        let dir = Dir::new();
        let path = dir.path().join(".boot.lock");
        // Another holder as a second daemon takes it: its own open file description, flock(LOCK_EX).
        // flock excludes per open file description, in this process as in another.
        let holder = try_exclusive(&path).unwrap().expect("free");
        assert!(try_shared(&path).unwrap().is_none());
        assert!(try_exclusive(&path).unwrap().is_none());
        drop(holder);
        let held = try_exclusive(&path)
            .unwrap()
            .expect("free after the holder ended");
        assert!(
            try_exclusive(&path).unwrap().is_none(),
            "a second open file description is excluded"
        );
        drop(held);
        assert!(try_exclusive(&path).unwrap().is_some());
    }
}
