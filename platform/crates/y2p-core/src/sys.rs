//! Safe wrappers for the libc calls the platform repeats: filesystem space (`statvfs64`,
//! `fstatvfs64`), file status (`fstat64`, `fstatat64`), positioned I/O (`pread64`, `pwrite64`) and
//! signal dispositions (`sigaction`), rtnetlink sockets and pidfds. The 64-bit calls are used throughout: the plain ones fail with
//! EOVERFLOW above 2 GiB on the player's 32-bit ARM, whose fields are also narrower; each figure is
//! widened here once.
// SPDX-License-Identifier: GPL-2.0-only
use std::{
    ffi::{CStr, CString},
    io,
    mem::MaybeUninit,
    os::{
        fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd},
        unix::ffi::OsStrExt,
    },
    path::Path,
};

/// `os.statvfs()` figures (`f_frsize`, `f_blocks`, …), widened to 64 bits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Space {
    pub frsize: u64,
    pub blocks: u64,
    pub bfree: u64,
    pub bavail: u64,
    pub files: u64,
    pub ffree: u64,
    pub favail: u64,
    pub flag: u64,
}

impl Space {
    #[allow(clippy::unnecessary_cast)] // the field widths differ on the player's 32-bit ARM
    fn of(vfs: &libc::statvfs64) -> Space {
        Space {
            frsize: vfs.f_frsize as u64,
            blocks: vfs.f_blocks as u64,
            bfree: vfs.f_bfree as u64,
            bavail: vfs.f_bavail as u64,
            files: vfs.f_files as u64,
            ffree: vfs.f_ffree as u64,
            favail: vfs.f_favail as u64,
            flag: vfs.f_flag as u64,
        }
    }

    /// `f_blocks * f_frsize`.
    pub fn total(&self) -> u64 {
        self.blocks * self.frsize
    }

    /// `f_bavail * f_frsize`: what an unprivileged writer may still use.
    pub fn available(&self) -> u64 {
        self.bavail * self.frsize
    }

    /// `f_flag & ST_RDONLY`.
    #[allow(clippy::unnecessary_cast)] // a c_ulong: 32 bits on the player
    pub fn read_only(&self) -> bool {
        self.flag & libc::ST_RDONLY as u64 != 0
    }
}

fn c_path(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

fn check(result: libc::c_int) -> io::Result<()> {
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// `os.statvfs(path)`.
pub fn statvfs(path: &Path) -> io::Result<Space> {
    let name = c_path(path)?;
    let mut vfs = MaybeUninit::<libc::statvfs64>::uninit();
    // SAFETY: a NUL-terminated path and a buffer the call fills on success.
    check(unsafe { libc::statvfs64(name.as_ptr(), vfs.as_mut_ptr()) })?;
    // SAFETY: statvfs64 returned 0, so it filled the buffer.
    Ok(Space::of(&unsafe { vfs.assume_init() }))
}

/// `os.fstatvfs(fd)`.
pub fn fstatvfs(fd: BorrowedFd<'_>) -> io::Result<Space> {
    let mut vfs = MaybeUninit::<libc::statvfs64>::uninit();
    // SAFETY: an open descriptor (the borrow) and a buffer the call fills on success.
    check(unsafe { libc::fstatvfs64(fd.as_raw_fd(), vfs.as_mut_ptr()) })?;
    // SAFETY: fstatvfs64 returned 0, so it filled the buffer.
    Ok(Space::of(&unsafe { vfs.assume_init() }))
}

/// `os.stat_result` fields the platform reads, widened to 64 bits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stat {
    pub dev: u64,
    pub ino: u64,
    pub mode: u32,
    pub nlink: u64,
    pub uid: u32,
    pub gid: u32,
    pub size: i64,
    pub mtime: i64,
    pub mtime_nsec: i64,
}

impl Stat {
    #[allow(clippy::unnecessary_cast, clippy::useless_conversion)] // narrower on 32-bit ARM
    fn of(meta: &libc::stat64) -> Stat {
        Stat {
            dev: meta.st_dev as u64,
            ino: meta.st_ino as u64,
            mode: meta.st_mode as u32,
            nlink: meta.st_nlink as u64,
            uid: meta.st_uid as u32,
            gid: meta.st_gid as u32,
            size: meta.st_size as i64,
            mtime: crate::wide(meta.st_mtime),
            mtime_nsec: crate::wide(meta.st_mtime_nsec),
        }
    }

    /// `stat.S_ISDIR(mode)`.
    pub fn is_dir(&self) -> bool {
        self.mode & libc::S_IFMT == libc::S_IFDIR
    }

    /// `stat.S_ISREG(mode)`.
    pub fn is_file(&self) -> bool {
        self.mode & libc::S_IFMT == libc::S_IFREG
    }
}

/// `os.open(name, flags, mode, dir_fd=dir)`: openat(2) relative to `dir` (the working directory when
/// `None`), with `flags` exactly as given; the new descriptor is owned by the caller.
pub fn openat(
    dir: Option<BorrowedFd<'_>>,
    name: &CStr,
    flags: libc::c_int,
    mode: libc::mode_t,
) -> io::Result<OwnedFd> {
    let base = dir.map_or(libc::AT_FDCWD, |d| d.as_raw_fd());
    // SAFETY: an open directory descriptor (the borrow) or AT_FDCWD, and a NUL-terminated name.
    let fd = unsafe { libc::openat(base, name.as_ptr(), flags, libc::c_uint::from(mode)) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat returned a new descriptor nobody else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// `os.fstat(fd)`.
pub fn fstat(fd: BorrowedFd<'_>) -> io::Result<Stat> {
    let mut meta = MaybeUninit::<libc::stat64>::uninit();
    // SAFETY: an open descriptor (the borrow) and a buffer the call fills on success.
    check(unsafe { libc::fstat64(fd.as_raw_fd(), meta.as_mut_ptr()) })?;
    // SAFETY: fstat64 returned 0, so it filled the buffer.
    Ok(Stat::of(&unsafe { meta.assume_init() }))
}

/// `os.stat(name, dir_fd=dir, follow_symlinks=…)`: `flags` as fstatat(2) takes them
/// (`AT_SYMLINK_NOFOLLOW`).
pub fn fstatat(dir: BorrowedFd<'_>, name: &CStr, flags: libc::c_int) -> io::Result<Stat> {
    let mut meta = MaybeUninit::<libc::stat64>::uninit();
    // SAFETY: an open directory descriptor, a NUL-terminated name and a buffer the call fills on
    // success.
    check(unsafe { libc::fstatat64(dir.as_raw_fd(), name.as_ptr(), meta.as_mut_ptr(), flags) })?;
    // SAFETY: fstatat64 returned 0, so it filled the buffer.
    Ok(Stat::of(&unsafe { meta.assume_init() }))
}

/// `os.pread(fd, len(buffer), offset)` into `buffer`: the bytes read.
pub fn pread(fd: BorrowedFd<'_>, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    let offset =
        libc::off64_t::try_from(offset).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?;
    // SAFETY: the buffer is valid for writes of its length; the descriptor is open (the borrow).
    let done = unsafe {
        libc::pread64(
            fd.as_raw_fd(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            offset,
        )
    };
    usize::try_from(done).map_err(|_| io::Error::last_os_error())
}

/// `os.pwrite(fd, data, offset)`: the bytes written.
pub fn pwrite(fd: BorrowedFd<'_>, data: &[u8], offset: u64) -> io::Result<usize> {
    let offset =
        libc::off64_t::try_from(offset).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?;
    // SAFETY: the data is valid for reads of its length; the descriptor is open (the borrow).
    let done = unsafe { libc::pwrite64(fd.as_raw_fd(), data.as_ptr().cast(), data.len(), offset) };
    usize::try_from(done).map_err(|_| io::Error::last_os_error())
}

/// A signal's disposition as sigaction(2) reports it, to put back later.
#[derive(Clone, Copy)]
pub struct Disposition(libc::sigaction);

/// What a signal does once [`set_signal`] installed it.
#[derive(Clone, Copy, Debug)]
pub enum Handler {
    /// `SIG_DFL`.
    Default,
    /// `SIG_IGN`.
    Ignore,
    /// A handler function.
    Call(extern "C" fn(libc::c_int)),
}

/// `sigaction(signal, {handler, flags, empty mask})`: the disposition it replaced.
///
/// # Safety
/// A [`Handler::Call`] function runs in signal context: it must only do async-signal-safe things
/// (store to atomics, write(2) to a pipe), never allocate or lock.
pub unsafe fn set_signal(
    signal: libc::c_int,
    handler: Handler,
    flags: libc::c_int,
) -> io::Result<Disposition> {
    // SAFETY: an all-zero sigaction is valid (no handler, no flags); the mask is emptied below.
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = match handler {
        Handler::Default => libc::SIG_DFL,
        Handler::Ignore => libc::SIG_IGN,
        Handler::Call(function) => function as libc::sighandler_t,
    };
    action.sa_flags = flags;
    // SAFETY: both structs are valid and owned here; the caller vouches for the handler.
    unsafe {
        libc::sigemptyset(&mut action.sa_mask);
        let mut old: libc::sigaction = std::mem::zeroed();
        check(libc::sigaction(signal, &action, &mut old))?;
        Ok(Disposition(old))
    }
}

/// `signal.getsignal(signal)`: the current disposition, to put back later.
pub fn signal_disposition(signal: libc::c_int) -> io::Result<Disposition> {
    // SAFETY: an all-zero sigaction is a valid buffer; a null new action only reads.
    unsafe {
        let mut old: libc::sigaction = std::mem::zeroed();
        check(libc::sigaction(signal, std::ptr::null(), &mut old))?;
        Ok(Disposition(old))
    }
}

impl Disposition {
    /// Put this disposition back for `signal` (what `signal.signal(signal, saved)` restores).
    pub fn restore(&self, signal: libc::c_int) {
        // SAFETY: re-installing an action the kernel returned for this process.
        unsafe { libc::sigaction(signal, &self.0, std::ptr::null_mut()) };
    }
}

/// `unlinkat(dir, name, flags)`: a file (flags 0) or an empty directory (`AT_REMOVEDIR`) by its name
/// in an open directory; a symlink is removed, never followed.
pub fn unlinkat(dir: BorrowedFd<'_>, name: &CStr, flags: libc::c_int) -> io::Result<()> {
    // SAFETY: a NUL-terminated name relative to an open directory.
    check(unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), flags) })
}

/// `mkdirat(dir, name, mode)`.
pub fn mkdirat(dir: BorrowedFd<'_>, name: &CStr, mode: libc::mode_t) -> io::Result<()> {
    // SAFETY: a NUL-terminated name relative to an open directory.
    check(unsafe { libc::mkdirat(dir.as_raw_fd(), name.as_ptr(), mode) })
}

/// `renameat(from_dir, from, to_dir, to)`.
pub fn renameat(
    from_dir: BorrowedFd<'_>,
    from: &CStr,
    to_dir: BorrowedFd<'_>,
    to: &CStr,
) -> io::Result<()> {
    // SAFETY: NUL-terminated names relative to open directories.
    check(unsafe {
        libc::renameat(
            from_dir.as_raw_fd(),
            from.as_ptr(),
            to_dir.as_raw_fd(),
            to.as_ptr(),
        )
    })
}

/// `fsync(fd)`: a file's data and metadata, or a directory's entries, on the device.
pub fn fsync(fd: BorrowedFd<'_>) -> io::Result<()> {
    // SAFETY: a plain call on an open descriptor.
    check(unsafe { libc::fsync(fd.as_raw_fd()) })
}

/// The names in an open directory in readdir order from its start (`os.scandir(fd)`), without `.`
/// and `..`, at most `limit`. The directory's own descriptor stays open and unmoved: a duplicate is
/// read. A failed read is an error, never a shorter list.
pub fn dir_names(directory: BorrowedFd<'_>, limit: usize) -> io::Result<Vec<CString>> {
    use std::os::fd::IntoRawFd;
    let copy = directory.try_clone_to_owned()?.into_raw_fd();
    // SAFETY: fdopendir takes over the duplicate on success; on failure it is closed here.
    let stream = unsafe { libc::fdopendir(copy) };
    if stream.is_null() {
        let error = io::Error::last_os_error();
        // SAFETY: the duplicate fdopendir did not take.
        unsafe { libc::close(copy) };
        return Err(error);
    }
    // SAFETY: `stream` is a valid DIR stream until the closedir below; d_name is NUL-terminated.
    unsafe {
        libc::rewinddir(stream);
        let mut found = Vec::new();
        let result = loop {
            if found.len() >= limit {
                break Ok(found);
            }
            *libc::__errno_location() = 0;
            let entry = libc::readdir(stream);
            if entry.is_null() {
                let errno = *libc::__errno_location();
                break if errno == 0 {
                    Ok(found)
                } else {
                    Err(io::Error::from_raw_os_error(errno))
                };
            }
            let name = CStr::from_ptr((*entry).d_name.as_ptr()).to_owned();
            if name.as_bytes() != b"." && name.as_bytes() != b".." {
                found.push(name);
            }
        };
        libc::closedir(stream);
        result
    }
}

/// `kill(pid, signal)`.
pub fn kill(pid: libc::pid_t, signal: libc::c_int) -> io::Result<()> {
    // SAFETY: a plain signal; a pid that is gone answers ESRCH.
    if unsafe { libc::kill(pid, signal) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Whether `pid` names a live process (`kill(pid, 0)`); never a group or every process (pid ≤ 0).
pub fn alive(pid: i64) -> bool {
    libc::pid_t::try_from(pid).is_ok_and(|pid| pid > 0 && kill(pid, 0).is_ok())
}

/// A stream `AF_UNIX` socket (close-on-exec, non-blocking when asked).
pub fn unix_stream(nonblock: bool) -> io::Result<OwnedFd> {
    let flags =
        libc::SOCK_STREAM | libc::SOCK_CLOEXEC | if nonblock { libc::SOCK_NONBLOCK } else { 0 };
    // SAFETY: a plain socket call; the descriptor is owned right after it is created.
    let fd = unsafe { libc::socket(libc::AF_UNIX, flags, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the descriptor was just created and is owned once.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// A filesystem `AF_UNIX` address and its length; `None` for a path longer than `sun_path`.
pub fn unix_address(path: &Path) -> Option<(libc::sockaddr_un, libc::socklen_t)> {
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: an all-zero sockaddr_un is valid.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.len() > address.sun_path.len() {
        return None;
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (slot, byte) in address.sun_path.iter_mut().zip(bytes) {
        *slot = *byte as libc::c_char;
    }
    let length = std::mem::size_of::<libc::sa_family_t>() + bytes.len();
    Some((address, length as libc::socklen_t))
}

/// `connect(fd, address)`: on a non-blocking socket `EINPROGRESS` is the error to wait out (for
/// `POLLOUT`, then [`socket_error`]).
pub fn connect_unix(
    fd: BorrowedFd<'_>,
    (address, length): &(libc::sockaddr_un, libc::socklen_t),
) -> io::Result<()> {
    // SAFETY: `address` is an initialised sockaddr_un of `length` bytes.
    check(unsafe {
        libc::connect(
            fd.as_raw_fd(),
            (address as *const libc::sockaddr_un).cast(),
            *length,
        )
    })
}

/// `SO_ERROR`: how a non-blocking connect ended (`Ok` when it connected).
pub fn socket_error(fd: BorrowedFd<'_>) -> io::Result<()> {
    let mut code: libc::c_int = 0;
    let mut size = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: SO_ERROR into one c_int of its size.
    check(unsafe {
        libc::getsockopt(
            fd.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_ERROR,
            (&mut code as *mut libc::c_int).cast(),
            &mut size,
        )
    })?;
    match code {
        0 => Ok(()),
        code => Err(io::Error::from_raw_os_error(code)),
    }
}

/// An rtnetlink socket (close-on-exec, non-blocking when asked), bound to the multicast `groups`
/// (`RTMGRP_*`) when there are any; without groups the kernel binds it on its first send.
pub fn netlink_route(groups: u32, nonblock: bool) -> io::Result<OwnedFd> {
    netlink(libc::NETLINK_ROUTE, groups, nonblock)
}

/// A netlink socket of `protocol` (`NETLINK_ROUTE`, `NETLINK_GENERIC`), as [`netlink_route`].
pub fn netlink(protocol: libc::c_int, groups: u32, nonblock: bool) -> io::Result<OwnedFd> {
    let flags =
        libc::SOCK_RAW | libc::SOCK_CLOEXEC | if nonblock { libc::SOCK_NONBLOCK } else { 0 };
    // SAFETY: a plain socket call; the descriptor is owned right after it is created.
    let fd = unsafe { libc::socket(libc::AF_NETLINK, flags, protocol) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the descriptor was just created and is owned once.
    let sock = unsafe { OwnedFd::from_raw_fd(fd) };
    if groups != 0 {
        // SAFETY: sockaddr_nl is plain data; zeroed is the kernel's "assign a port" address.
        let mut address: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
        address.nl_family = libc::AF_NETLINK as libc::sa_family_t;
        address.nl_groups = groups;
        // SAFETY: a valid sockaddr_nl of its own size.
        let bound = unsafe {
            libc::bind(
                sock.as_raw_fd(),
                (&address as *const libc::sockaddr_nl).cast(),
                std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
            )
        };
        if bound != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(sock)
}

/// `pidfd_open(pid, 0)`: a descriptor that stays this process even after its pid is reused.
pub fn pidfd_open(pid: libc::pid_t) -> io::Result<OwnedFd> {
    // SAFETY: pidfd_open(2) takes a pid and flags and returns a new descriptor or -1.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the descriptor was just created and is owned once.
    Ok(unsafe { OwnedFd::from_raw_fd(fd as libc::c_int) })
}

/// `pidfd_send_signal(pidfd, signal, NULL, 0)`.
pub fn pidfd_send_signal(pidfd: BorrowedFd<'_>, signal: libc::c_int) -> io::Result<()> {
    // SAFETY: a pidfd, a signal number, no siginfo, no flags.
    let sent = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            pidfd.as_raw_fd(),
            signal,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        )
    };
    if sent != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::root::tests::tempdir::Dir;
    use std::{fs::File, os::fd::AsFd};

    #[test]
    fn space_status_and_positioned_io() {
        let dir = Dir::new();
        let path = dir.path().join("f");
        std::fs::write(&path, b"hello world").unwrap();
        let space = statvfs(dir.path()).unwrap();
        assert!(space.frsize > 0 && space.total() >= space.available());
        let file = File::options().read(true).write(true).open(&path).unwrap();
        assert_eq!(fstatvfs(file.as_fd()).unwrap().frsize, space.frsize);
        let meta = fstat(file.as_fd()).unwrap();
        assert!(meta.is_file() && !meta.is_dir() && meta.size == 11 && meta.nlink == 1);
        let directory = File::open(dir.path()).unwrap();
        let name = CString::new("f").unwrap();
        assert_eq!(
            fstatat(directory.as_fd(), &name, libc::AT_SYMLINK_NOFOLLOW)
                .unwrap()
                .ino,
            meta.ino
        );
        assert_eq!(pwrite(file.as_fd(), b"W", 6).unwrap(), 1);
        let mut buffer = [0u8; 5];
        assert_eq!(pread(file.as_fd(), &mut buffer, 6).unwrap(), 5);
        assert_eq!(&buffer, b"World");
        assert_eq!(
            statvfs(&dir.path().join("missing"))
                .unwrap_err()
                .raw_os_error(),
            Some(libc::ENOENT)
        );
    }

    #[test]
    fn signal_dispositions_restore() {
        let before = signal_disposition(libc::SIGUSR2).unwrap();
        // SAFETY: SIG_IGN runs no code.
        let saved = unsafe { set_signal(libc::SIGUSR2, Handler::Ignore, 0) }.unwrap();
        saved.restore(libc::SIGUSR2);
        assert_eq!(
            signal_disposition(libc::SIGUSR2).unwrap().0.sa_sigaction,
            before.0.sa_sigaction
        );
    }

    #[test]
    fn directory_names_come_from_a_duplicate_and_honour_the_limit() {
        let dir = Dir::new();
        for name in ["a", "b", "c"] {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
        let directory = File::open(dir.path()).unwrap();
        let mut names = dir_names(directory.as_fd(), usize::MAX).unwrap();
        names.sort();
        assert_eq!(names, [c"a".to_owned(), c"b".to_owned(), c"c".to_owned()]);
        assert_eq!(dir_names(directory.as_fd(), 2).unwrap().len(), 2);
        // The caller's descriptor is still open and readable from the start.
        assert_eq!(dir_names(directory.as_fd(), usize::MAX).unwrap().len(), 3);
        let file = File::open(dir.path().join("a")).unwrap();
        assert!(dir_names(file.as_fd(), usize::MAX).is_err(), "no directory");
    }
}
