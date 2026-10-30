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
        fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd},
        unix::ffi::OsStrExt,
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
pub(super) fn openat(dir: RawFd, name: &str, flags: i32, mode: u32) -> Result<OwnedFd, Failure> {
    let c = c_name(name)?;
    // SAFETY: the caller's directory descriptor is open for this call.
    let dir = unsafe { BorrowedFd::borrow_raw(dir) };
    y2p_core::sys::openat(Some(dir), &c, flags | libc::O_CLOEXEC, mode as libc::mode_t)
        .at(Path::new(name))
}

/// Open an absolute (root-prefixed) directory path, never through a final symlink.
pub(super) fn open_absolute_directory(path: &Path) -> Result<OwnedFd, Failure> {
    let name = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| Failure::os(io::ErrorKind::InvalidInput.into(), path))?;
    // SAFETY: as openat.
    let fd = unsafe {
        libc::open64(
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(Failure::os(io::Error::last_os_error(), path));
    }
    // SAFETY: as openat.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

pub(super) fn fstat(fd: RawFd) -> Result<y2p_core::sys::Stat, Failure> {
    // SAFETY: callers pass a descriptor they hold open for this call.
    y2p_core::sys::fstat(unsafe { BorrowedFd::borrow_raw(fd) }).map_err(Failure::from)
}

/// The bytes available to unprivileged writers on the filesystem holding `fd`.
pub(super) fn available(fd: RawFd) -> Result<u64, Failure> {
    // SAFETY: callers pass a descriptor they hold open for this call.
    let space = y2p_core::sys::fstatvfs(unsafe { BorrowedFd::borrow_raw(fd) })?;
    Ok(space.available())
}

/// The result of a libc call that returns 0 on success.
pub(super) fn check(result: libc::c_int) -> Result<(), Failure> {
    if result != 0 {
        return Err(Failure::from(io::Error::last_os_error()));
    }
    Ok(())
}

pub(super) fn fsync(fd: RawFd) -> Result<(), Failure> {
    // SAFETY: plain syscall on an open descriptor.
    check(unsafe { libc::fsync(fd) })
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
    dir: RawFd,
    name: &str,
    flags: libc::c_int,
    missing_ok: bool,
) -> Result<(), Failure> {
    let c = c_name(name)?;
    // SAFETY: a NUL-terminated name relative to an open directory.
    if unsafe { libc::unlinkat(dir, c.as_ptr(), flags) } != 0 {
        let error = io::Error::last_os_error();
        if missing_ok && error.raw_os_error() == Some(libc::ENOENT) {
            return Ok(());
        }
        return Err(Failure::os(error, Path::new(name)));
    }
    Ok(())
}

/// A hard link without following symlinks: atomic, and fails on an existing name (`[Errno 17] File
/// exists: 'sealed.part' -> 'name'`), so it never replaces music.
pub(super) fn linkat(from_dir: RawFd, from: &str, to_dir: RawFd, to: &str) -> Result<(), Failure> {
    let (source, target) = (c_name(from)?, c_name(to)?);
    // SAFETY: NUL-terminated names relative to open directories.
    let result = unsafe { libc::linkat(from_dir, source.as_ptr(), to_dir, target.as_ptr(), 0) };
    if result != 0 {
        let error = io::Error::last_os_error();
        let (text, class) = py::os_error(&error, None);
        return Err(Failure::Os {
            text: format!("{text}: {} -> {}", py::repr(from), py::repr(to)),
            class,
        });
    }
    Ok(())
}

/// One read of at most `limit` bytes (empty at the end of the file).
pub(super) fn read_chunk(fd: RawFd, limit: usize) -> Result<Vec<u8>, Failure> {
    let mut buffer = vec![0u8; limit];
    // SAFETY: the buffer is writable for `limit` bytes.
    let count = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), limit) };
    if count < 0 {
        return Err(Failure::from(io::Error::last_os_error()));
    }
    buffer.truncate(count as usize);
    Ok(buffer)
}

/// One write; how many bytes it took.
pub(super) fn write_some(fd: RawFd, data: &[u8]) -> Result<usize, Failure> {
    // SAFETY: the slice is readable for its length.
    let count = unsafe { libc::write(fd, data.as_ptr().cast(), data.len()) };
    if count < 0 {
        return Err(Failure::from(io::Error::last_os_error()));
    }
    Ok(count as usize)
}

/// One directory component below `parent` (made 0755 first with `create`): never through a symlink,
/// never onto another filesystem.
pub(super) fn open_directory(
    parent: RawFd,
    component: &str,
    create: bool,
) -> Result<OwnedFd, Failure> {
    if create {
        let c = c_name(component)?;
        // SAFETY: a NUL-terminated name relative to an open directory.
        if unsafe { libc::mkdirat(parent, c.as_ptr(), 0o755) } == 0 {
            fsync(parent)?;
        } else {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::EEXIST) {
                return Err(Failure::os(error, Path::new(component)));
            }
        }
    }
    let fd = openat(
        parent,
        component,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        0o777,
    )?;
    if fstat(fd.as_raw_fd())?.dev != fstat(parent)?.dev {
        return Err(Failure::value("transfer_submount_refused"));
    }
    Ok(fd)
}
