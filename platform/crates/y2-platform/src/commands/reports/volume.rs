//! What the maintenance and diagnostics commands need of other groups: the verified /data volume
//! (devices::storage, every volume's `blkid` request in the order the goldens record), the space
//! admission, the update's free-space rule, a directory opened below another without crossing a
//! mount, and the update journal's state.
// SPDX-License-Identifier: GPL-2.0-only
use super::{json_at, media};
pub use crate::commands::devices::storage::{data_volume, Volume};
use crate::reply::Failure;
use std::os::fd::AsFd;
use std::os::fd::OwnedFd;
use y2p_core::Root;

pub const RESERVE: u64 = 96 * 1024 * 1024;

// statvfs fields are 32-bit on the player (armv7), 64-bit on the host.
/// `space.admission(volume, requested, purpose)['allowed']` for the cache/diagnostic/update purposes.
pub fn admitted(volume: &Volume, requested: u64) -> bool {
    if matches!(
        volume.space_state,
        "ReadOnlyRisk" | "Failed" | "Unavailable"
    ) {
        return false;
    }
    let allowed = volume
        .available_bytes
        .is_some_and(|free| free as i128 - requested as i128 >= RESERVE as i128);
    allowed && volume.space_state == "Normal"
}

/// `update.free(fd, size)`.
pub fn free(fd: &OwnedFd, size: u64) -> Result<(), Failure> {
    let info = y2p_core::sys::fstatvfs(fd.as_fd())?;
    let available = info.bavail as u128 * info.frsize as u128;
    if info.read_only()
        || available < size as u128 + RESERVE as u128
        || (info.files != 0 && info.favail < 128)
    {
        return Err(Failure::value("update_data_space_reserve"));
    }
    Ok(())
}

/// `transfer.open_directory(parent, component)` (no create).
pub fn open_directory(parent: &OwnedFd, component: &str) -> Result<OwnedFd, Failure> {
    let fd = media::open_at(
        Some(parent),
        component,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        0,
    )
    .map_err(|e| super::os_name(e, component))?;
    let (a, b) = (media::fstat(&fd)?, media::fstat(parent)?);
    if a.dev != b.dev {
        return Err(Failure::value("transfer_submount_refused"));
    }
    Ok(fd)
}

/// The update journal's state as `update status` reads it; a journal without a state is refused.
pub fn update_state(root: &Root) -> Result<String, Failure> {
    use super::pyjson::Py;
    let path = root.path("/data/updates/state.json");
    let value = json_at(root, "/data/updates/state.json");
    let value = match value {
        None if !path.exists() && !path.is_symlink() => {
            let ready = root.read("/data/.y2data-schema").as_deref() == Some("1")
                && root.path("/data/updates").is_dir();
            return Ok(if ready { "Idle" } else { "Unavailable" }.into());
        }
        other => other,
    };
    let schema_ok = value
        .as_ref()
        .and_then(|v| v.get("schema"))
        .is_some_and(|s| s.py_eq(&Py::int(1)) || s.py_eq(&Py::int(2)));
    let Some(value @ Py::Dict(_)) = value.filter(|_| schema_ok) else {
        return Ok("Failed".into());
    };
    match value.get("state") {
        Some(Py::Str(state)) => Ok(state.clone()),
        Some(_) => Ok(String::new()), // a non-string state equals none of the names compared
        None => Err(Failure::value("update_state_unreadable")),
    }
}
