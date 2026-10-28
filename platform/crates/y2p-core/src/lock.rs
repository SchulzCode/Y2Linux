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
    let file = open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

/// `fcntl.flock(fd, LOCK_SH | LOCK_NB)`: a shared lease (the activity lease's holders).
pub fn try_shared(path: &Path) -> io::Result<Option<File>> {
    let file = open(path)?;
    match file.try_lock_shared() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::root::tests::tempdir::Dir;
    use std::process::Command;

    #[test]
    fn a_python_holder_excludes_rust() {
        let dir = Dir::new();
        let path = dir.path().join(".boot.lock");
        // A Python holder as the platform takes it (`os.open` + `fcntl.flock(fd, LOCK_EX)`).
        let Ok(mut holder) = Command::new("python3")
            .args([
                "-I",
                "-c",
                "import fcntl, os, sys, time\n\
                    fd = os.open(sys.argv[1], os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)\n\
                    fcntl.flock(fd, fcntl.LOCK_EX)\ntime.sleep(5)",
            ])
            .arg(&path)
            .spawn()
        else {
            return; // no host Python
        };
        let started = std::time::Instant::now();
        while try_shared(&path).unwrap().is_some() {
            assert!(
                started.elapsed().as_secs() < 3,
                "holder never took the lock"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(try_exclusive(&path).unwrap().is_none());
        holder.kill().unwrap();
        holder.wait().unwrap();
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
