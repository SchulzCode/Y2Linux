//! The descriptor calls of a transfer: every file operation below `/data` goes through directory
//! descriptors opened without following symlinks (`openat` and friends relative to an open
//! directory), and an OS error names the relative name the call was given.
// SPDX-License-Identifier: GPL-2.0-only
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

/// The OS error of a call without a file name (read, write, fsync, flock).
pub(super) fn unnamed(error: io::Error) -> Failure {
    let (text, class) = py::os_error(&error, None);
    Failure::Os { text, class }
}

// ---- descriptors ---------------------------------------------------------------------------------

pub(super) fn c_name(name: &str) -> CString {
    CString::new(name).expect("validated names hold no NUL")
}

/// `os.open(name, flags, mode, dir_fd=dir)` (Python adds O_CLOEXEC).
pub(super) fn openat(dir: RawFd, name: &str, flags: i32, mode: u32) -> Result<OwnedFd, Failure> {
    let c = c_name(name);
    // SAFETY: a NUL-terminated name; the result is a new descriptor this function owns.
    let fd = unsafe {
        libc::openat(
            dir,
            c.as_ptr(),
            flags | libc::O_CLOEXEC,
            mode as libc::c_uint,
        )
    };
    if fd < 0 {
        return Err(Failure::os(io::Error::last_os_error(), Path::new(name)));
    }
    // SAFETY: fd was just opened and is owned by nobody else.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// `os.open(path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW)` of an absolute (root-prefixed) path.
pub(super) fn open_absolute_directory(path: &Path) -> Result<OwnedFd, Failure> {
    let name = CString::new(path.as_os_str().as_bytes()).expect("paths hold no NUL");
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
    y2p_core::sys::fstat(unsafe { BorrowedFd::borrow_raw(fd) }).map_err(unnamed)
}

/// `f_bavail * f_frsize` of the filesystem holding `fd`.
pub(super) fn available(fd: RawFd) -> Result<u64, Failure> {
    // SAFETY: callers pass a descriptor they hold open for this call.
    let space = y2p_core::sys::fstatvfs(unsafe { BorrowedFd::borrow_raw(fd) }).map_err(unnamed)?;
    Ok(space.available())
}

pub(super) fn check(result: libc::c_int) -> Result<(), Failure> {
    if result != 0 {
        return Err(unnamed(io::Error::last_os_error()));
    }
    Ok(())
}

pub(super) fn fsync(fd: RawFd) -> Result<(), Failure> {
    // SAFETY: plain syscall on an open descriptor.
    check(unsafe { libc::fsync(fd) })
}

/// `fcntl.flock(fd, LOCK_EX | LOCK_NB)`: busy is `[Errno 11] Resource temporarily unavailable`.
pub(super) fn flock(fd: BorrowedFd<'_>) -> Result<(), Failure> {
    match lock::try_fd(fd, lock::Kind::Exclusive) {
        Ok(true) => Ok(()),
        Ok(false) => Err(unnamed(lock::would_block())),
        Err(error) => Err(unnamed(error)),
    }
}

/// `os.unlink(name, dir_fd=dir)` (`os.rmdir` with AT_REMOVEDIR); `missing_ok` as
/// `except FileNotFoundError: pass`.
pub(super) fn unlinkat(
    dir: RawFd,
    name: &str,
    flags: libc::c_int,
    missing_ok: bool,
) -> Result<(), Failure> {
    let c = c_name(name);
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

/// `os.link(source, name, src_dir_fd=…, dst_dir_fd=…, follow_symlinks=False)`: atomic, and fails
/// on an existing name (`[Errno 17] File exists: 'sealed.part' -> 'name'`).
pub(super) fn linkat(from_dir: RawFd, from: &str, to_dir: RawFd, to: &str) -> Result<(), Failure> {
    // SAFETY: NUL-terminated names relative to open directories.
    let result = unsafe {
        libc::linkat(
            from_dir,
            c_name(from).as_ptr(),
            to_dir,
            c_name(to).as_ptr(),
            0,
        )
    };
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

pub(super) fn read_chunk(fd: RawFd, limit: usize) -> Result<Vec<u8>, Failure> {
    let mut buffer = vec![0u8; limit];
    // SAFETY: the buffer is writable for `limit` bytes.
    let count = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), limit) };
    if count < 0 {
        return Err(unnamed(io::Error::last_os_error()));
    }
    buffer.truncate(count as usize);
    Ok(buffer)
}

pub(super) fn write_some(fd: RawFd, data: &[u8]) -> Result<usize, Failure> {
    // SAFETY: the slice is readable for its length.
    let count = unsafe { libc::write(fd, data.as_ptr().cast(), data.len()) };
    if count < 0 {
        return Err(unnamed(io::Error::last_os_error()));
    }
    Ok(count as usize)
}

/// `open_directory(parent, component, create)`: one component, never through a symlink, never onto
/// another filesystem.
pub(super) fn open_directory(
    parent: RawFd,
    component: &str,
    create: bool,
) -> Result<OwnedFd, Failure> {
    if create {
        // SAFETY: a NUL-terminated name relative to an open directory.
        let c = c_name(component);
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
