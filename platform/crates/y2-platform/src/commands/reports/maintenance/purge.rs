//! `reset purge`: delete a completed reset's quarantine, bounded per call (the owner asks again).
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    data_volume, fstat, fsync, open_data, open_directory, os_name, pyops, regular_bytes, stat_at,
    text, CString, Failure, Lease, OwnedFd, Root, Value,
};
use crate::commands::reports::{object, ValueExt};
use std::os::fd::AsFd;

/// The names in a directory, in readdir order (`os.scandir(fd)`).
fn entries(fd: &OwnedFd) -> Result<Vec<String>, Failure> {
    use std::os::fd::AsFd;
    Ok(y2p_core::sys::dir_names(fd.as_fd(), usize::MAX)?
        .into_iter()
        .map(|name| name.to_string_lossy().into_owned())
        .collect())
}

fn three_digits(name: &str) -> bool {
    name.len() == 3 && name.bytes().all(|b| b.is_ascii_digit())
}

/// `reset purge`: delete a completed reset's quarantine (a Maintenance workload), at most 256
/// entries or 8 s per call.
pub(super) fn purge(root: &Root, token: &str, confirm: Option<&str>) -> Result<Value, Failure> {
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
    if !record.is_object() {
        return Err(Failure::value("reset_record_unreadable"));
    }
    if !record.get("id").is_some_and(|v| v.eq(&Value::from(token)))
        || !record
            .get("state")
            .is_some_and(|v| v.eq(&Value::from("Complete")))
        || !text(record.get("purge_confirmation"))
            .eq(&confirm.map(Value::from).unwrap_or(Value::Null))
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
                y2p_core::sys::unlinkat(fd.as_fd(), &c, libc::AT_REMOVEDIR)
                    .map_err(|e| os_name(e, &name))?;
            } else {
                // Symlinks inside user content are unlinked, never followed.
                y2p_core::sys::unlinkat(fd.as_fd(), &c, 0).map_err(|e| os_name(e, &name))?;
            }
            *removed += 1;
        }
        fsync(fd)?;
        Ok(true)
    }
    let complete = walk(clock, &top, 0, device, deadline, &mut removed)?;
    Ok(object()
        .with(
            "state",
            Value::from(if complete {
                "Complete"
            } else {
                "MoreRemaining"
            }),
        )
        .with("removed", Value::from(removed))
        .with("id", Value::from(token)))
}
