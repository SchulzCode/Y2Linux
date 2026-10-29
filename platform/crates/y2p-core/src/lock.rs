//! The platform's flock protocol: a lock file opened `O_RDWR|O_CREAT|O_NOFOLLOW` 0600 (close-on-exec)
//! and locked with flock(2), so every holder of the same path excludes the others (plan §3.3). A lock
//! is released when the returned file is dropped or the process ends. A descriptor the caller already
//! has (one inherited from the parent, a directory opened through `openat`) is locked with [`try_fd`].
// SPDX-License-Identifier: GPL-2.0-only
use std::{
    fs::{File, OpenOptions},
    io,
    os::{
        fd::{AsRawFd, BorrowedFd},
        unix::fs::OpenOptionsExt,
    },
    path::Path,
    time::{Duration, Instant},
};

/// Who may hold the lock beside this holder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `LOCK_EX`: nobody.
    Exclusive,
    /// `LOCK_SH`: other shared holders (an activity lease).
    Shared,
}

/// `os.open(path, O_RDWR | O_CREAT | O_NOFOLLOW, 0o600)` (close-on-exec): a lock file, never through a
/// symlink.
pub fn open(path: &Path) -> io::Result<File> {
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

/// `fcntl.flock(fd, LOCK_EX | LOCK_NB)`: `Ok(None)` while another holder has it.
pub fn try_exclusive(path: &Path) -> io::Result<Option<File>> {
    settle(|| try_path(path, Kind::Exclusive))
}

/// `fcntl.flock(fd, LOCK_SH | LOCK_NB)`: a shared lease (the activity lease's holders).
pub fn try_shared(path: &Path) -> io::Result<Option<File>> {
    settle(|| try_path(path, Kind::Shared))
}

/// [`try_exclusive`] again every 0.2 s until `wait` has passed; `None` when it stayed busy (a check
/// or a repair holds the card, a short admission passes).
pub fn exclusive_within(path: &Path, wait: Duration) -> io::Result<Option<File>> {
    let deadline = Instant::now() + wait;
    loop {
        if let Some(file) = try_exclusive(path)? {
            return Ok(Some(file));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// `fcntl.flock(fd, LOCK_EX | LOCK_NB)` (or `LOCK_SH`) on a descriptor the caller holds: `Ok(false)`
/// while another holder has it. The lock lasts as long as the descriptor's open file description.
pub fn try_fd(fd: BorrowedFd<'_>, kind: Kind) -> io::Result<bool> {
    // Test builds: the same short retry as `settle` (a forked child of another test holds a copy).
    #[cfg(feature = "testing")]
    for _ in 0..20 {
        if try_fd_once(fd, kind)? {
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    try_fd_once(fd, kind)
}

fn try_fd_once(fd: BorrowedFd<'_>, kind: Kind) -> io::Result<bool> {
    let operation = match kind {
        Kind::Exclusive => libc::LOCK_EX,
        Kind::Shared => libc::LOCK_SH,
    };
    // SAFETY: flock(2) on a descriptor the borrow keeps open; no memory is passed.
    if unsafe { libc::flock(fd.as_raw_fd(), operation | libc::LOCK_NB) } == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::EWOULDBLOCK) => Ok(false),
        _ => Err(error),
    }
}

/// The busy answer of a non-blocking flock: `[Errno 11] Resource temporarily unavailable`.
pub fn would_block() -> io::Error {
    io::Error::from_raw_os_error(libc::EWOULDBLOCK)
}

fn try_path(path: &Path, kind: Kind) -> io::Result<Option<File>> {
    let file = open(path)?;
    Ok(try_fd_once(std::os::fd::AsFd::as_fd(&file), kind)?.then_some(file))
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

/// The pids (as /proc/locks prints them, in its order) that hold a flock on the file `meta`
/// describes: /proc/locks names it `<major hex>:<minor hex>:<inode>`.
pub fn flock_pids(root: &crate::Root, meta: &std::fs::Metadata) -> Vec<String> {
    use std::os::unix::fs::MetadataExt;
    let dev = meta.dev();
    let target = (
        u64::from(libc::major(dev)),
        u64::from(libc::minor(dev)),
        meta.ino(),
    );
    let locks = root.read("/proc/locks").unwrap_or_default();
    locks
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let pid = *fields.get(4)?;
            if fields[1] != "FLOCK" || pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let mut id = fields.get(5)?.split(':');
            let (a, b, c) = (id.next()?, id.next()?, id.next()?);
            let found = (
                u64::from_str_radix(a, 16).ok()?,
                u64::from_str_radix(b, 16).ok()?,
                c.parse::<u64>().ok()?,
            );
            (id.next().is_none() && found == target).then(|| pid.to_owned())
        })
        .collect()
}

/// The live holders of a flock on that file as `pid/comm` (the name with anything but word
/// characters, `.` and `-` as `_`, at most 32 characters), at most 8: never inferred from old files.
pub fn flock_holders(root: &crate::Root, meta: &std::fs::Metadata) -> Vec<String> {
    static COMM: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let comm = COMM.get_or_init(|| regex::Regex::new(r"[^\w.-]").expect("pattern"));
    flock_pids(root, meta)
        .into_iter()
        .take(8)
        .map(|pid| {
            let name = root
                .read(&format!("/proc/{pid}/comm"))
                .filter(|c| !c.is_empty())
                .unwrap_or_else(|| "unknown".into());
            let name: String = comm.replace_all(&name, "_").chars().take(32).collect();
            format!("{pid}/{name}")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::root::tests::tempdir::Dir;
    use std::os::fd::AsFd;

    #[test]
    fn another_holder_excludes_this_one() {
        let dir = Dir::new();
        let path = dir.path().join(".boot.lock");
        // Another holder as a second daemon takes it: its own open file description, flock(LOCK_EX).
        // flock excludes per open file description, in this process as in another.
        let holder = try_exclusive(&path).unwrap().expect("free");
        assert!(try_shared(&path).unwrap().is_none());
        assert!(try_exclusive(&path).unwrap().is_none());
        assert!(exclusive_within(&path, Duration::from_millis(250))
            .unwrap()
            .is_none());
        let other = open(&path).unwrap();
        assert!(!try_fd(other.as_fd(), Kind::Shared).unwrap());
        drop(holder);
        assert!(try_fd(other.as_fd(), Kind::Shared).unwrap());
        assert!(try_shared(&path).unwrap().is_some(), "shared holders share");
        assert!(try_exclusive(&path).unwrap().is_none());
        drop(other);
        let held = try_exclusive(&path)
            .unwrap()
            .expect("free after the holder ended");
        assert!(
            try_exclusive(&path).unwrap().is_none(),
            "a second open file description is excluded"
        );
        drop(held);
        assert!(try_exclusive(&path).unwrap().is_some());
        assert_eq!(
            would_block().to_string(),
            "Resource temporarily unavailable (os error 11)"
        );
    }

    #[test]
    fn flock_holders_are_read_from_proc_locks_by_device_and_inode() {
        use std::os::unix::fs::MetadataExt;
        let dir = Dir::new();
        let path = dir.path().join("held.lock");
        std::fs::write(&path, "").unwrap();
        let meta = std::fs::metadata(&path).unwrap();
        let id = format!(
            "{:02x}:{:02x}:{}",
            libc::major(meta.dev()),
            libc::minor(meta.dev()),
            meta.ino()
        );
        let root = crate::Root::fixture(dir.path());
        std::fs::create_dir_all(dir.path().join("proc/41")).unwrap();
        std::fs::write(dir.path().join("proc/41/comm"), "y2 platform\n").unwrap();
        std::fs::write(
            dir.path().join("proc/locks"),
            format!(
                "1: FLOCK  ADVISORY  WRITE 41 {id} 0 EOF\n\
                 2: POSIX  ADVISORY  WRITE 42 {id} 0 EOF\n\
                 3: FLOCK  ADVISORY  WRITE 43 00:01:1 0 EOF\n\
                 4: FLOCK  ADVISORY  WRITE 44 {id} 0 EOF\n"
            ),
        )
        .unwrap();
        assert_eq!(flock_pids(&root, &meta), ["41", "44"]);
        assert_eq!(
            flock_holders(&root, &meta),
            ["41/y2_platform", "44/unknown"]
        );
    }
}
