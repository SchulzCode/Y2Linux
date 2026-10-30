//! `reset purge`: delete a completed reset's quarantine, bounded per call (the owner asks again).
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    data_volume, dup, fstat, fsync, open_data, open_directory, os_name, pyops, regular_bytes,
    stat_at, text, CString, Failure, Lease, OwnedFd, Py, Root,
};
use std::os::fd::AsFd;
use std::os::fd::AsRawFd;

/// The names in a directory, in readdir order (`os.scandir(fd)`).
fn entries(fd: &OwnedFd) -> Result<Vec<String>, Failure> {
    let copy = dup(fd)?;
    let raw = copy.as_raw_fd();
    std::mem::forget(copy);
    // SAFETY: fdopendir takes ownership of the duplicated descriptor; closedir releases it.
    let dir = unsafe { libc::fdopendir(raw) };
    if dir.is_null() {
        return Err(Failure::from(std::io::Error::last_os_error()));
    }
    // SAFETY: rewind to the start as scandir's fresh stream.
    unsafe { libc::rewinddir(dir) };
    let mut names = Vec::new();
    loop {
        // SAFETY: a valid DIR stream.
        let entry = unsafe { libc::readdir(dir) };
        if entry.is_null() {
            break;
        }
        // SAFETY: d_name is NUL-terminated.
        let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        if name != "." && name != ".." {
            names.push(name);
        }
    }
    // SAFETY: the stream is closed once.
    unsafe { libc::closedir(dir) };
    Ok(names)
}

fn three_digits(name: &str) -> bool {
    name.len() == 3 && name.bytes().all(|b| b.is_ascii_digit())
}

/// `reset purge`: delete a completed reset's quarantine (a Maintenance workload), at most 256
/// entries or 8 s per call.
pub(super) fn purge(root: &Root, token: &str, confirm: Option<&str>) -> Result<Py, Failure> {
    let id_ok = token.len() == 32
        && token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    let _lease = Lease::take(root, "Maintenance")?;
    if !id_ok {
        return Err(Failure::value("invalid_reset_id"));
    }
    data_volume(root)?;
    let data = open_data(root)?;
    let parent = open_directory(&data, "reset-quarantine")?;
    let top = open_directory(&parent, token)?;
    drop((parent, data));
    // `json.loads(...)` (its JSONDecodeError is a Failed answer); a record that is no object is
    // refused.
    let record = pyops::loads(&regular_bytes(&top, "record.json", 16384)?)?;
    if !record.is_dict() {
        return Err(Failure::value("reset_record_unreadable"));
    }
    if !record.get("id").is_some_and(|v| v.py_eq(&Py::str(token)))
        || !record
            .get("state")
            .is_some_and(|v| v.py_eq(&Py::str("Complete")))
        || !text(record.get("purge_confirmation")).py_eq(&confirm.map(Py::str).unwrap_or(Py::None))
    {
        return Err(Failure::value("completed_reset_confirmation_required"));
    }
    if !y2p_core::lock::try_fd(top.as_fd(), y2p_core::lock::Kind::Exclusive)? {
        return Err(Failure::from(y2p_core::lock::would_block()));
    }
    let device = fstat(&top)?.dev;
    let clock = root.clock();
    let deadline = clock.monotonic() + 8.0;
    let mut removed = 0usize;
    fn walk(
        clock: &dyn y2p_core::clock::Clock,
        fd: &OwnedFd,
        depth: usize,
        device: u64,
        deadline: f64,
        removed: &mut usize,
    ) -> Result<bool, Failure> {
        if depth > 16 {
            return Err(Failure::value("purge_depth_limit"));
        }
        for name in entries(fd)? {
            if depth == 0 && !three_digits(&name) {
                continue;
            }
            if *removed >= 256 || clock.monotonic() >= deadline {
                return Ok(false);
            }
            let meta = stat_at(fd, &name).map_err(|e| os_name(e, &name))?;
            if meta.dev != device {
                return Err(Failure::value("purge_submount_refused"));
            }
            let c = CString::new(name.as_str()).expect("no NUL");
            if meta.mode & libc::S_IFMT == libc::S_IFDIR {
                let child = open_directory(fd, &name)?;
                let complete = walk(clock, &child, depth + 1, device, deadline, removed)?;
                drop(child);
                if !complete {
                    return Ok(false);
                }
                // SAFETY: a valid fd and name.
                if unsafe { libc::unlinkat(fd.as_raw_fd(), c.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
                    return Err(os_name(std::io::Error::last_os_error(), &name));
                }
            } else {
                // Symlinks inside user content are unlinked, never followed.
                // SAFETY: a valid fd and name.
                if unsafe { libc::unlinkat(fd.as_raw_fd(), c.as_ptr(), 0) } != 0 {
                    return Err(os_name(std::io::Error::last_os_error(), &name));
                }
            }
            *removed += 1;
        }
        fsync(fd)?;
        Ok(true)
    }
    let complete = walk(clock, &top, 0, device, deadline, &mut removed)?;
    Ok(Py::dict()
        .with(
            "state",
            Py::str(if complete {
                "Complete"
            } else {
                "MoreRemaining"
            }),
        )
        .with("removed", Py::from(removed))
        .with("id", Py::str(token)))
}
