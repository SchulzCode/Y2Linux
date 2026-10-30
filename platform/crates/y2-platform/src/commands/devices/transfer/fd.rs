//! The descriptor calls of a transfer: every file operation below `/data` goes through directory
//! descriptors opened without following symlinks (`openat` and friends relative to an open
//! directory), and an OS error names the relative name the call was given.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::At;
use crate::reply::Failure;
use std::{
    ffi::CString,
    io,
    os::{
        fd::{AsFd, BorrowedFd, OwnedFd},
        unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    },
    path::Path,
};
use y2p_core::{lock, py};

// ---- errors ---------------------------------------------------------------------------------------

// ---- descriptors ---------------------------------------------------------------------------------

/// A name for a descriptor call. The names are validated media names, stage ids and fixed names, none
/// of which can hold a NUL; one that did is not a media name.
fn c_name(name: &str) -> Result<CString, Failure> {
    CString::new(name).map_err(|_| Failure::value("invalid_media_name"))
}

/// Open `name` relative to the directory `dir` (always close-on-exec).
pub(super) fn openat(
    dir: BorrowedFd<'_>,
    name: &str,
    flags: i32,
    mode: u32,
) -> Result<OwnedFd, Failure> {
    let c = c_name(name)?;
    y2p_core::sys::openat(Some(dir), &c, flags | libc::O_CLOEXEC, mode as libc::mode_t)
        .at(Path::new(name))
}

/// Open an absolute (root-prefixed) directory path, never through a final symlink.
pub(super) fn open_absolute_directory(path: &Path) -> Result<OwnedFd, Failure> {
    if path.as_os_str().as_bytes().contains(&0) {
        return Err(Failure::os(io::ErrorKind::InvalidInput.into(), path));
    }
    // std opens with O_CLOEXEC (and the large-file open on 32-bit ARM).
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)
        .map(OwnedFd::from)
        .at(path)
}

pub(super) fn fstat(fd: BorrowedFd<'_>) -> Result<y2p_core::sys::Stat, Failure> {
    y2p_core::sys::fstat(fd).map_err(Failure::from)
}

/// The bytes available to unprivileged writers on the filesystem holding `fd`.
pub(super) fn available(fd: BorrowedFd<'_>) -> Result<u64, Failure> {
    let space = y2p_core::sys::fstatvfs(fd)?;
    Ok(space.available())
}

/// The result of a libc call that returns 0 on success.
pub(super) fn check(result: libc::c_int) -> Result<(), Failure> {
    if result != 0 {
        return Err(Failure::from(io::Error::last_os_error()));
    }
    Ok(())
}

pub(super) fn fsync(fd: BorrowedFd<'_>) -> Result<(), Failure> {
    Ok(y2p_core::sys::fsync(fd)?)
}

/// An exclusive lock without waiting: busy is `[Errno 11] Resource temporarily unavailable`.
pub(super) fn flock(fd: BorrowedFd<'_>) -> Result<(), Failure> {
    match lock::try_fd(fd, lock::Kind::Exclusive) {
        Ok(true) => Ok(()),
        Ok(false) => Err(Failure::from(lock::would_block())),
        Err(error) => Err(Failure::from(error)),
    }
}

/// Remove `name` from the directory `dir` (a directory with AT_REMOVEDIR); with `missing_ok` a name
/// that is already gone is fine.
pub(super) fn unlinkat(
    dir: BorrowedFd<'_>,
    name: &str,
    flags: libc::c_int,
    missing_ok: bool,
) -> Result<(), Failure> {
    let c = c_name(name)?;
    match y2p_core::sys::unlinkat(dir, &c, flags) {
        Err(error) if missing_ok && error.raw_os_error() == Some(libc::ENOENT) => Ok(()),
        result => result.at(Path::new(name)),
    }
}

/// A hard link without following symlinks: atomic, and fails on an existing name (`[Errno 17] File
/// exists: 'sealed.part' -> 'name'`), so it never replaces music.
pub(super) fn linkat(
    from_dir: BorrowedFd<'_>,
    from: &str,
    to_dir: BorrowedFd<'_>,
    to: &str,
) -> Result<(), Failure> {
    let (source, target) = (c_name(from)?, c_name(to)?);
    if let Err(error) = y2p_core::sys::linkat(from_dir, &source, to_dir, &target) {
        let (text, class) = py::os_error(&error, None);
        return Err(Failure::Os {
            text: format!("{text}: {} -> {}", py::repr(from), py::repr(to)),
            class,
        });
    }
    Ok(())
}

/// One read of at most `limit` bytes (empty at the end of the file).
pub(super) fn read_chunk(fd: BorrowedFd<'_>, limit: usize) -> Result<Vec<u8>, Failure> {
    let mut buffer = vec![0u8; limit];
    let count = y2p_core::sys::read(fd, &mut buffer)?;
    buffer.truncate(count);
    Ok(buffer)
}

/// One write; how many bytes it took.
pub(super) fn write_some(fd: BorrowedFd<'_>, data: &[u8]) -> Result<usize, Failure> {
    Ok(y2p_core::sys::write(fd, data)?)
}

/// One directory component below `parent` (made 0755 first with `create`): never through a symlink,
/// never onto another filesystem.
pub(super) fn open_directory(
    parent: BorrowedFd<'_>,
    component: &str,
    create: bool,
) -> Result<OwnedFd, Failure> {
    if create {
        let c = c_name(component)?;
        match y2p_core::sys::mkdirat(parent, &c, 0o755) {
            Ok(()) => fsync(parent)?,
            Err(error) if error.raw_os_error() == Some(libc::EEXIST) => {}
            Err(error) => return Err(Failure::os(error, Path::new(component))),
        }
    }
    let fd = openat(
        parent,
        component,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        0o777,
    )?;
    if fstat(fd.as_fd())?.dev != fstat(parent)?.dev {
        return Err(Failure::value("transfer_submount_refused"));
    }
    Ok(fd)
}
