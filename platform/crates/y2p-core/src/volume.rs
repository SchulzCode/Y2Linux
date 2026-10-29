//! The volumes of `observe.storage(ctx)['volumes']` as transfer, maintenance, diagnostics and update
//! read them (`transfer.data_volume`): root, Y2DATA, the SD card and Y2Media, in that consumer order,
//! each checked for its controller and filesystem UUID with the same `blkid` request. The SD card's
//! own claim and protected read-only mounts (`media-mount.json`) belong to the storage group's
//! observation; nothing here reads them.
// SPDX-License-Identifier: GPL-2.0-only
use crate::{command, mounts::mounts, sys::statvfs, Root};
use std::path::{Path, PathBuf};

/// The volume paths in their consumer order (an index order consumers rely on).
pub const TARGETS: [&str; 4] = ["/", "/data", "/media/sd", "/media/internal"];

/// One entry of `storage(ctx)['volumes']`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    pub path: &'static str,
    pub state: &'static str,
    /// `<boot id>:<mount id>:<device id>`: the same value means the same mount of the same device
    /// this boot.
    pub generation: Option<String>,
    pub uuid: Option<String>,
    pub space_state: &'static str,
    pub available_bytes: Option<u64>,
}

/// `observe.space_state`: product capacity budget, not electrical thresholds. Never delete user data.
pub fn space_state(available: u64, total: u64, readonly: bool, error: bool) -> &'static str {
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

/// `Path.resolve()` (strict=False): symlinks resolved as far as the path exists.
fn resolve(path: &Path) -> PathBuf {
    if let Ok(real) = path.canonicalize() {
        return real;
    }
    let mut missing = Vec::new();
    let mut existing = path.to_path_buf();
    while !existing.as_os_str().is_empty() {
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
            _ => break,
        }
    }
    path.to_path_buf()
}

/// `observe.storage(ctx)['volumes']`. Stacked mounts are ambiguous to consumers: a target that is not
/// mounted exactly once is Unavailable.
pub fn volumes(root: &Root) -> Vec<Volume> {
    let mounts = mounts(root);
    let boot_id = root.boot_id();
    let mut volumes = Vec::new();
    for target in TARGETS {
        let found: Vec<_> = mounts.iter().filter(|m| m.path == target).collect();
        let [item] = found[..] else {
            volumes.push(Volume {
                path: target,
                state: "Unavailable",
                generation: None,
                uuid: None,
                space_state: "Unavailable",
                available_bytes: None,
            });
            continue;
        };
        let generation = boot_id
            .as_deref()
            .filter(|b| !b.is_empty())
            .map(|b| format!("{b}:{}:{}", item.mount_id, item.device_id));
        let device = root.path(&format!("/sys/dev/block/{}", item.device_id));
        let expected = if target == "/media/sd" {
            "11240000.mmc"
        } else {
            "11230000.mmc"
        };
        let controller_valid =
            resolve(&device).to_string_lossy().contains(expected) && device.exists();
        let mut state = if controller_valid { "Ready" } else { "Failed" };
        let mut uuid = None;
        if controller_valid {
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
        let readonly = item.read_only();
        let (space_state, available_bytes) = match statvfs(&root.path(target)) {
            Ok(space) => {
                let mut space_state = space_state(
                    space.available(),
                    space.total(),
                    readonly,
                    state == "Failed",
                );
                if space.files > 0 && space.favail == 0 {
                    space_state = "CriticalSpace";
                }
                (space_state, Some(space.available()))
            }
            Err(_) => {
                state = "Failed";
                ("Failed", None)
            }
        };
        if state == "Ready" && readonly {
            state = "Degraded";
        }
        volumes.push(Volume {
            path: target,
            state,
            generation,
            uuid,
            space_state,
            available_bytes,
        });
    }
    volumes
}

/// The volume at `path` (one of [`TARGETS`]; every volume is computed, in order, for the same
/// `blkid` requests).
pub fn volume(root: &Root, path: &str) -> Volume {
    volumes(root)
        .into_iter()
        .find(|v| v.path == path)
        .expect("one of the four volume targets")
}

/// The volume at `path` when it is verified: Ready, with a generation this boot.
pub fn verified(root: &Root, path: &str) -> Option<Volume> {
    Some(volume(root, path)).filter(|v| v.state == "Ready" && v.generation.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_states_follow_the_budget() {
        assert_eq!(space_state(10, 100, false, false), "CriticalSpace");
        assert_eq!(space_state(1 << 40, 1 << 41, false, false), "Normal");
        assert_eq!(space_state(80 << 20, 100 << 20, false, false), "LowSpace");
        assert_eq!(space_state(1 << 40, 1 << 41, true, false), "ReadOnlyRisk");
        assert_eq!(space_state(1 << 40, 1 << 41, true, true), "Failed");
    }
}
