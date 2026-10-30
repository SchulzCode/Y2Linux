//! Y2DATA admission policy and bounded cleanup of explicitly disposable files
//! (`y2-platform space [--cleanup]`).
// SPDX-License-Identifier: GPL-2.0-only
use super::observe::volumes;
use super::sys::{get, mkdir, observation_record, wall, Object};
use crate::reply::At;
use crate::reply::{atomic_json, Failure};
use serde_json::{json, Value};
use std::os::fd::BorrowedFd;
use std::{ffi::CString, io, os::unix::ffi::OsStrExt};
use y2p_core::{storage_media::disposable_name, Root};

const PURPOSES: [&str; 6] = [
    "cache",
    "diagnostic",
    "update",
    "user_state",
    "database",
    "transfer",
];

fn admission(volume: &Object, purpose: &str) -> Value {
    let state = volume
        .get("space_state")
        .cloned()
        .unwrap_or(json!("Unavailable"));
    if matches!(
        state.as_str(),
        Some("ReadOnlyRisk" | "Failed" | "Unavailable")
    ) {
        return json!({"allowed": false, "reason": state});
    }
    let reserve: i64 = if matches!(purpose, "user_state" | "database") {
        32 << 20
    } else {
        96 << 20
    };
    // A flag counts as 0 or 1 bytes, as older records had it.
    let free = match get(volume, "available_bytes") {
        Value::Bool(b) => Some(i64::from(*b)),
        Value::Number(n) => n.as_i64(),
        _ => None,
    };
    let mut allowed = free.is_some_and(|f| f >= reserve);
    if matches!(purpose, "cache" | "diagnostic" | "update") && state.as_str() != Some("Normal") {
        allowed = false;
    }
    json!({"allowed": allowed, "reason": if allowed { "budget_available" } else { "space_reserved" },
           "reserve_bytes": reserve, "requested_bytes": 0})
}

/// An open directory descriptor, closed when dropped.
struct Fd(libc::c_int);

impl Drop for Fd {
    fn drop(&mut self) {
        // SAFETY: a descriptor this module opened and owns.
        unsafe { libc::close(self.0) };
    }
}

fn last_error() -> io::Error {
    io::Error::last_os_error()
}

fn cstring(text: &str) -> io::Result<CString> {
    CString::new(text).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))
}

fn open_directory(at: libc::c_int, name: &CString) -> io::Result<Fd> {
    // SAFETY: a NUL-terminated name relative to an open directory (or AT_FDCWD).
    let fd = unsafe {
        libc::openat(
            at,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        Err(last_error())
    } else {
        Ok(Fd(fd))
    }
}

fn fstat(fd: libc::c_int) -> io::Result<y2p_core::sys::Stat> {
    // SAFETY: the caller's descriptor (an Fd it owns) is open for this call.
    y2p_core::sys::fstat(unsafe { BorrowedFd::borrow_raw(fd) })
}

fn lstat_at(fd: libc::c_int, name: &CString) -> io::Result<y2p_core::sys::Stat> {
    // SAFETY: as fstat, for a name in an open directory, not following a symlink.
    y2p_core::sys::fstatat(
        unsafe { BorrowedFd::borrow_raw(fd) },
        name,
        libc::AT_SYMLINK_NOFOLLOW,
    )
}

fn regular(meta: &y2p_core::sys::Stat) -> bool {
    meta.mode & libc::S_IFMT == libc::S_IFREG
}

/// The names of an open directory in its order (without `.` and `..`), at most `limit`.
fn names(fd: libc::c_int, limit: usize) -> io::Result<Vec<CString>> {
    // SAFETY: the caller's descriptor (an Fd it owns) is open for this call.
    y2p_core::sys::dir_names(unsafe { BorrowedFd::borrow_raw(fd) }, limit)
}

/// Never walk arbitrary trees or delete media, settings, databases or stages.
fn disposable_cleanup(root: &Root, max_files: u64, max_bytes: u64) -> Result<Value, Failure> {
    let now = wall();
    let (mut files, mut bytes, mut errors) = (0u64, 0u64, Vec::new());
    let data = root.path("/data");
    let top = CString::new(data.as_os_str().as_bytes()).map_err(Failure::value)?;
    let rootfd = open_directory(libc::AT_FDCWD, &top).at(&data)?;
    for (parts, category, keep) in [
        (["reborn", "cache"], "cache", 0usize),
        (["reborn", "diagnostics"], "diagnostics", 2),
        (["cache", "platform"], "platform", 0),
    ] {
        let mut attempt = || -> io::Result<()> {
            let [parent, child] = parts;
            let parent = open_directory(rootfd.0, &cstring(parent)?)?;
            let fd = open_directory(parent.0, &cstring(child)?)?;
            if fstat(fd.0)?.dev != fstat(rootfd.0)?.dev {
                return Err(io::Error::from_raw_os_error(libc::EXDEV)); // cleanup_submount_refused
            }
            // A bounded scan, not an unbounded in-memory directory listing.
            let mut entries: Vec<(f64, String, u64, u64)> = Vec::new();
            for name in names(fd.0, 4096)? {
                let text = name.to_string_lossy().into_owned();
                if !disposable_name(category, &text) {
                    continue;
                }
                let meta = lstat_at(fd.0, &name)?;
                if regular(&meta) && meta.nlink == 1 {
                    let mtime = meta.mtime as f64 + meta.mtime_nsec as f64 * 1e-9;
                    entries.push((mtime, text, meta.size as u64, meta.ino));
                }
            }
            entries.sort_by(|a, b| {
                b.0.total_cmp(&a.0)
                    .then_with(|| b.1.cmp(&a.1))
                    .then_with(|| b.2.cmp(&a.2))
                    .then_with(|| b.3.cmp(&a.3))
            });
            for (mtime, name, size, inode) in entries.iter().skip(keep).rev() {
                if now - mtime < 86_400.0 || files >= max_files || bytes + size > max_bytes {
                    continue;
                }
                let name = cstring(name)?;
                let current = lstat_at(fd.0, &name)?;
                if current.ino != *inode || current.size as u64 != *size || !regular(&current) {
                    continue;
                }
                // SAFETY: a NUL-terminated name in the open directory.
                if unsafe { libc::unlinkat(fd.0, name.as_ptr(), 0) } != 0 {
                    return Err(last_error());
                }
                files += 1;
                bytes += size;
            }
            Ok(())
        };
        match attempt() {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => errors.push(json!({"category": category, "errno": error.raw_os_error()})),
        }
    }
    Ok(json!({"files": files, "bytes": bytes, "errors": errors}))
}

pub fn policy(root: &Root, cleanup: bool) -> Result<Value, Failure> {
    let [_, data, _, _] = volumes(root);
    let admission: Object = PURPOSES
        .iter()
        .map(|p| (p.to_string(), admission(&data, p)))
        .collect();
    let space_state = get(&data, "space_state").clone();
    let mut result = json!({
        "schema": "org.y2linux.space/v1", "record": observation_record(root, "space-policy"),
        "state": space_state, "volume": data.clone(), "admission": admission, "cleanup": null,
    });
    if cleanup
        && get(&data, "state").as_str() == Some("Ready")
        && matches!(space_state.as_str(), Some("LowSpace" | "CriticalSpace"))
    {
        result["cleanup"] = disposable_cleanup(root, 64, 8 << 20)?;
    }
    let directory = root.path("/run/y2");
    mkdir(&directory, 0o700)?;
    atomic_json(&directory.join("storage.json"), &result, false)?;
    Ok(result)
}

#[cfg(test)]
#[path = "space_tests.rs"]
mod space_tests;
