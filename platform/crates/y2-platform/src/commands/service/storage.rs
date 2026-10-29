//! `transfer.data_volume`: the verified Y2DATA volume as `observe.storage` reports it. Only the
//! `/data` entry is computed in full; the other volumes are walked for the identity queries
//! (`blkid`) `observe.storage` makes on the way, in its order, so the subprocesses asked for are the
//! same.
// SPDX-License-Identifier: GPL-2.0-only
use super::python::realpath;
use crate::reply::Failure;
use y2p_core::{command, Root};

const TARGETS: [&str; 4] = ["/", "/data", "/media/sd", "/media/internal"];

/// The `/data` volume's state and generation.
pub struct Volume {
    pub state: &'static str,
    pub generation: Option<String>,
}

/// `observe.storage(ctx)['volumes']`, the `/data` entry.
pub fn data(root: &Root) -> Volume {
    let mounts = y2p_core::mounts::mounts(root);
    let boot_id = root.boot_id();
    let mut result = Volume {
        state: "Unavailable",
        generation: None,
    };
    for target in TARGETS {
        let found: Vec<_> = mounts.iter().filter(|m| m.path == target).collect();
        // Stacked mounts are ambiguous to consumers; fail closed.
        let [item] = found.as_slice() else {
            continue;
        };
        let generation = boot_id
            .as_ref()
            .filter(|b| !b.is_empty())
            .map(|b| format!("{b}:{}:{}", item.mount_id, item.device_id));
        let device = root.path(&format!("/sys/dev/block/{}", item.device_id));
        let real = realpath(&device).to_string_lossy().into_owned();
        let expected = if target == "/media/sd" {
            "11240000.mmc"
        } else {
            "11230000.mmc"
        };
        let valid = real.contains(expected) && device.exists();
        let mut state = if valid { "Ready" } else { "Failed" };
        if valid {
            let answer = command::run(
                root,
                &["blkid", "-p", "-s", "UUID", "-o", "value", &item.source],
                command::TIMEOUT,
            );
            let uuid = if answer.ok {
                answer.output.filter(|o| !o.is_empty())
            } else {
                None
            };
            let expected_uuid = match target {
                "/" => Some("79324c69-6e75-4801-8000-000000000101"),
                "/data" => Some("79324c69-6e75-4801-8000-000000000102"),
                "/media/internal" => Some("79324c69-6e75-4801-8000-000000000103"),
                _ => None,
            };
            if expected_uuid.is_some() && uuid.as_deref() != expected_uuid {
                state = "Failed";
            }
        }
        if target != "/data" {
            continue;
        }
        let readonly =
            item.options.iter().any(|o| o == "ro") || item.super_options.iter().any(|o| o == "ro");
        if statvfs(&root.path(target)).is_err() {
            state = "Failed";
        }
        if state == "Ready" && readonly {
            state = "Degraded";
        }
        result = Volume { state, generation };
    }
    result
}

fn statvfs(path: &std::path::Path) -> std::io::Result<()> {
    let text = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    // SAFETY: a valid C string and a zeroed, writable statvfs.
    let mut info: libc::statvfs64 = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs64(text.as_ptr(), &mut info) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// `transfer.data_volume`.
pub fn data_volume(root: &Root) -> Result<Volume, Failure> {
    let value = data(root);
    if value.state != "Ready" || value.generation.is_none() {
        return Err(Failure::value("verified_data_unavailable"));
    }
    Ok(value)
}
