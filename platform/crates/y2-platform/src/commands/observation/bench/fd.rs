//! Descriptor-relative calls the benchmarks make (the volume, its run directory, a file): never a
//! path through a mountpoint that could be replaced.
// SPDX-License-Identifier: GPL-2.0-only
use super::{Error, Result};
use std::{
    ffi::CString,
    fs::File,
    io,
    os::fd::{BorrowedFd, RawFd},
};

pub(super) fn check(result: libc::c_int) -> io::Result<libc::c_int> {
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

/// A name for a call: the run's own names (`run-<hex>`, `f<n>`, …) and directory entries, which never
/// hold a NUL.
pub(super) fn cname(name: &str) -> CString {
    CString::new(name).expect("scratch names have no NUL")
}

/// Open `name` relative to `directory` (close-on-exec).
pub(super) fn openat(directory: RawFd, name: &str, flags: i32, mode: libc::mode_t) -> Result<File> {
    let path = cname(name);
    y2p_core::sys::openat(
        Some(borrowed(directory)),
        &path,
        flags | libc::O_CLOEXEC,
        mode,
    )
    .map(File::from)
    .map_err(|e| Error::os(e, Some(name)))
}

/// A descriptor of this run (the volume, its run directory, a file) for one call.
pub(super) fn borrowed(fd: RawFd) -> BorrowedFd<'static> {
    // SAFETY: every descriptor a measurement passes stays open until the measurement ends, longer
    // than any one call it is borrowed for.
    unsafe { BorrowedFd::borrow_raw(fd) }
}

pub(super) fn fstat(fd: RawFd) -> Result<y2p_core::sys::Stat> {
    y2p_core::sys::fstat(borrowed(fd)).map_err(|e| Error::os(e, None))
}

pub(super) fn fstatvfs(fd: RawFd) -> Result<y2p_core::sys::Space> {
    y2p_core::sys::fstatvfs(borrowed(fd)).map_err(|e| Error::os(e, None))
}

pub(super) fn sync(fd: RawFd, data_only: bool) -> Result<()> {
    // SAFETY: a descriptor this process holds.
    let result = unsafe {
        if data_only {
            libc::fdatasync(fd)
        } else {
            libc::fsync(fd)
        }
    };
    check(result).map(drop).map_err(|e| Error::os(e, None))
}
