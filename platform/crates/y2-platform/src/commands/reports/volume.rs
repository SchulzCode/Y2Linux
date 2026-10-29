//! What maintenance.py and diagnostics.py need of other modules: `transfer.data_volume` (observe.storage's
//! /data volume, computed with every volume's `blkid` request in consumer order so the commands asked
//! for are Python's), `space.admission`, `update.free`, `transfer.open_directory` and
//! `update.status(ctx)['state']`.
// SPDX-License-Identifier: GPL-2.0-only
use super::{json_at, media};
use crate::reply::Failure;
use std::{
    ffi::CString,
    os::{fd::AsRawFd, fd::OwnedFd, unix::ffi::OsStrExt},
    path::{Path, PathBuf},
};
use y2p_core::{command, Root};

pub const RESERVE: u64 = 96 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Volume {
    pub state: &'static str,
    pub generation: Option<String>,
    pub uuid: Option<String>,
    pub space_state: &'static str,
    pub available_bytes: Option<u64>,
}

fn space_state(available: u64, total: u64, readonly: bool, error: bool) -> &'static str {
    const MIB: u64 = 1024 * 1024;
    if error {
        "Failed"
    } else if readonly {
        "ReadOnlyRisk"
    } else if available < (32 * MIB).max(total / 20) {
        "CriticalSpace"
    } else if available < (96 * MIB).max(total / 10) {
        "LowSpace"
    } else {
        "Normal"
    }
}

/// `Path.resolve()` (strict=False).
fn resolve(path: &Path) -> PathBuf {
    let mut missing = Vec::new();
    let mut existing = path.to_path_buf();
    loop {
        if let Ok(mut real) = existing.canonicalize() {
            for part in missing.iter().rev() {
                real.push(part);
            }
            return real;
        }
        match (existing.file_name(), existing.parent()) {
            (Some(name), Some(parent)) => {
                missing.push(name.to_os_string());
                existing = parent.to_path_buf();
            }
            _ => return path.to_path_buf(),
        }
    }
}

// statvfs fields are 32-bit on the player (armv7), 64-bit on the host.
#[allow(clippy::unnecessary_cast)]
/// `observe.storage(ctx)`'s volume for `/data`; every target's blkid request is made as Python's.
fn data(root: &Root) -> Volume {
    let mounts = media::mountinfo(root.read("/proc/self/mountinfo").as_deref());
    let boot = super::boot_id(root);
    let mut result = None;
    for target in ["/", "/data", "/media/sd", "/media/internal"] {
        let found: Vec<&media::Mount> = mounts.iter().filter(|m| m.path == target).collect();
        let [item] = found[..] else {
            if target == "/data" {
                result = Some(Volume {
                    state: "Unavailable",
                    generation: None,
                    uuid: None,
                    space_state: "Unavailable",
                    available_bytes: None,
                });
            }
            continue;
        };
        let device = root.path(&format!("/sys/dev/block/{}", item.device_id));
        let expected = if target == "/media/sd" {
            "11240000.mmc"
        } else {
            "11230000.mmc"
        };
        let valid = resolve(&device).to_string_lossy().contains(expected) && device.exists();
        let mut state = if valid { "Ready" } else { "Failed" };
        let mut uuid = None;
        if valid {
            let answer = command::run(
                root,
                &["blkid", "-p", "-s", "UUID", "-o", "value", &item.source],
                command::TIMEOUT,
            );
            uuid = if answer.ok {
                answer.output.filter(|o| !o.is_empty())
            } else {
                None
            };
            let wanted = match target {
                "/" => Some("79324c69-6e75-4801-8000-000000000101"),
                "/data" => Some("79324c69-6e75-4801-8000-000000000102"),
                "/media/internal" => Some("79324c69-6e75-4801-8000-000000000103"),
                _ => None,
            };
            if wanted.is_some() && uuid.as_deref() != wanted {
                state = "Failed";
            }
        }
        if target != "/data" {
            continue; // the SD card's identity checks and the others' space run no command
        }
        let readonly =
            item.options.iter().any(|o| o == "ro") || item.super_options.iter().any(|o| o == "ro");
        let (space, available) = match statvfs(&root.path(target)) {
            Some(vfs) => {
                let frsize = vfs.f_frsize as u64;
                let (total, available) =
                    (vfs.f_blocks as u64 * frsize, vfs.f_bavail as u64 * frsize);
                let mut space = space_state(available, total, readonly, state == "Failed");
                if vfs.f_files > 0 && vfs.f_favail == 0 {
                    space = "CriticalSpace";
                }
                (space, Some(available))
            }
            None => {
                state = "Failed";
                ("Failed", None)
            }
        };
        if state == "Ready" && readonly {
            state = "Degraded";
        }
        result = Some(Volume {
            state,
            generation: boot
                .as_deref()
                .filter(|b| !b.is_empty())
                .map(|b| format!("{b}:{}:{}", item.mount_id, item.device_id)),
            uuid,
            space_state: space,
            available_bytes: available,
        });
    }
    result.expect("the /data target")
}

fn statvfs(path: &Path) -> Option<libc::statvfs> {
    let name = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: a zeroed statvfs is a valid output buffer; the path is NUL-terminated.
    let mut vfs: libc::statvfs = unsafe { std::mem::zeroed() };
    (unsafe { libc::statvfs(name.as_ptr(), &mut vfs) } == 0).then_some(vfs)
}

/// `transfer.data_volume(ctx)`.
pub fn data_volume(root: &Root) -> Result<Volume, Failure> {
    let value = data(root);
    if value.state != "Ready" || value.generation.is_none() {
        return Err(Failure::value("verified_data_unavailable"));
    }
    Ok(value)
}

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
    // SAFETY: a zeroed statvfs is a valid output buffer.
    let mut info: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: a valid descriptor and buffer.
    if unsafe { libc::fstatvfs(fd.as_raw_fd(), &mut info) } != 0 {
        return Err(super::os_plain(std::io::Error::last_os_error()));
    }
    let available = info.f_bavail as u128 * info.f_frsize as u128;
    if info.f_flag & libc::ST_RDONLY != 0
        || available < size as u128 + RESERVE as u128
        || (info.f_files != 0 && info.f_favail < 128)
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
    let (a, b) = (
        media::fstat(&fd).map_err(super::os_plain)?,
        media::fstat(parent).map_err(super::os_plain)?,
    );
    if a.st_dev != b.st_dev {
        return Err(Failure::value("transfer_submount_refused"));
    }
    Ok(fd)
}

/// `update.status(ctx)['state']`: a journal without a state is a KeyError (main.py's traceback).
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
        None => Err(Failure::key_error("state")),
    }
}
