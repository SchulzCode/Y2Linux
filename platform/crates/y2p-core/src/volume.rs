//! The volumes of `observe.storage(ctx)['volumes']` as transfer, maintenance, diagnostics and update
//! read them (`transfer.data_volume`): root, Y2DATA, the SD card and Y2Media, in that consumer order,
//! each checked for its controller and filesystem UUID with the same `blkid` request. The SD card's
//! own claim and protected read-only mounts (`media-mount.json`) belong to the storage group's
//! observation; nothing here reads them.
// SPDX-License-Identifier: GPL-2.0-only
use crate::{command, mounts::mounts, sys::statvfs, Root};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// The volume paths in their consumer order (an index order consumers rely on).
pub const TARGETS: [&str; 4] = ["/", "/data", "/media/sd", "/media/internal"];

/// The eMMC's controller (MSDC0) and the SD card's (MSDC1), as their sysfs device paths name them.
pub const EMMC_CONTROLLER: &str = "11230000.mmc";
pub const SD_CONTROLLER: &str = "11240000.mmc";
/// The filesystem UUIDs the image's own volumes are made with.
pub const ROOT_UUID: &str = "79324c69-6e75-4801-8000-000000000101";
pub const DATA_UUID: &str = "79324c69-6e75-4801-8000-000000000102";
pub const Y2MEDIA_UUID: &str = "79324c69-6e75-4801-8000-000000000103";
/// Y2Media's partition (eMMC p8): its start and size in sectors, as sysfs `start`/`size` read.
pub const Y2MEDIA_START: &str = "3742720";
pub const Y2MEDIA_SIZE: &str = "11460608";

/// The filesystem UUID a volume target must have (none for the SD card).
pub fn expected_uuid(target: &str) -> Option<&'static str> {
    match target {
        "/" => Some(ROOT_UUID),
        "/data" => Some(DATA_UUID),
        "/media/internal" => Some(Y2MEDIA_UUID),
        _ => None,
    }
}

/// The controller a volume target's device sits on: the SD card on MSDC1, everything else on the
/// eMMC.
pub fn controller(target: &str) -> &'static str {
    if target == "/media/sd" {
        SD_CONTROLLER
    } else {
        EMMC_CONTROLLER
    }
}

/// A volume's state: `Ready`, `Degraded` (mounted read-only), `Failed` (wrong controller or
/// filesystem, or no statvfs), `Unavailable` (not mounted exactly once).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum VolumeState {
    Ready,
    Degraded,
    Failed,
    Unavailable,
}

/// A volume's capacity budget (`space_state`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum SpaceState {
    Normal,
    LowSpace,
    CriticalSpace,
    /// Mounted read-only.
    ReadOnlyRisk,
    Failed,
    Unavailable,
}

impl VolumeState {
    /// The text records and replies carry.
    pub fn as_str(self) -> &'static str {
        match self {
            VolumeState::Ready => "Ready",
            VolumeState::Degraded => "Degraded",
            VolumeState::Failed => "Failed",
            VolumeState::Unavailable => "Unavailable",
        }
    }
}

impl SpaceState {
    /// The text records and replies carry.
    pub fn as_str(self) -> &'static str {
        match self {
            SpaceState::Normal => "Normal",
            SpaceState::LowSpace => "LowSpace",
            SpaceState::CriticalSpace => "CriticalSpace",
            SpaceState::ReadOnlyRisk => "ReadOnlyRisk",
            SpaceState::Failed => "Failed",
            SpaceState::Unavailable => "Unavailable",
        }
    }
}

/// One entry of `storage(ctx)['volumes']`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    pub path: &'static str,
    pub state: VolumeState,
    /// `<boot id>:<mount id>:<device id>`: the same value means the same mount of the same device
    /// this boot.
    pub generation: Option<String>,
    pub uuid: Option<String>,
    pub space_state: SpaceState,
    pub available_bytes: Option<u64>,
}

/// `observe.space_state`: product capacity budget, not electrical thresholds. Never delete user data.
pub fn space_state(available: u64, total: u64, readonly: bool, error: bool) -> SpaceState {
    const MIB: u64 = 1024 * 1024;
    if error {
        SpaceState::Failed
    } else if readonly {
        SpaceState::ReadOnlyRisk
    } else if available < (32 * MIB).max(total / 20) {
        SpaceState::CriticalSpace
    } else if available < (96 * MIB).max(total / 10) {
        SpaceState::LowSpace
    } else {
        SpaceState::Normal
    }
}

/// `Path.resolve()` (strict=False): symlinks resolved as far as the path exists.
fn resolve(path: &Path) -> PathBuf {
    if let Ok(real) = crate::sys::real_path(path) {
        return real;
    }
    let mut missing = Vec::new();
    let mut existing = path.to_path_buf();
    while !existing.as_os_str().is_empty() {
        if let Ok(mut real) = crate::sys::real_path(&existing) {
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
    TARGETS
        .into_iter()
        .map(|target| probe(root, &mounts, boot_id.as_deref(), target))
        .collect()
}

/// One target's entry: its single mount's controller, filesystem UUID (`blkid`) and space.
fn probe(
    root: &Root,
    mounts: &[crate::mounts::Mount],
    boot_id: Option<&str>,
    target: &'static str,
) -> Volume {
    {
        let found: Vec<_> = mounts.iter().filter(|m| m.path == target).collect();
        let [item] = found[..] else {
            return Volume {
                path: target,
                state: VolumeState::Unavailable,
                generation: None,
                uuid: None,
                space_state: SpaceState::Unavailable,
                available_bytes: None,
            };
        };
        let generation = boot_id
            .filter(|b| !b.is_empty())
            .map(|b| format!("{b}:{}:{}", item.mount_id, item.device_id));
        let device = root.path(&format!("/sys/dev/block/{}", item.device_id));
        let expected = controller(target);
        let controller_valid =
            resolve(&device).to_string_lossy().contains(expected) && device.exists();
        let mut state = if controller_valid {
            VolumeState::Ready
        } else {
            VolumeState::Failed
        };
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
            let expected_uuid = expected_uuid(target);
            if expected_uuid.is_some() && uuid.as_deref() != expected_uuid {
                state = VolumeState::Failed;
            }
        }
        let readonly = item.read_only();
        let (space_state, available_bytes) = match statvfs(&root.path(target)) {
            Ok(space) => {
                let mut space_state = space_state(
                    space.available(),
                    space.total(),
                    readonly,
                    state == VolumeState::Failed,
                );
                if space.files > 0 && space.favail == 0 {
                    space_state = SpaceState::CriticalSpace;
                }
                (space_state, Some(space.available()))
            }
            Err(_) => {
                state = VolumeState::Failed;
                (SpaceState::Failed, None)
            }
        };
        if state == VolumeState::Ready && readonly {
            state = VolumeState::Degraded;
        }
        Volume {
            path: target,
            state,
            generation,
            uuid,
            space_state,
            available_bytes,
        }
    }
}

/// The volume at `path` (one of [`TARGETS`]), probed alone: the other volumes (the SD card's
/// `blkid` and `statvfs` among them) are not touched.
pub fn volume(root: &Root, path: &str) -> Volume {
    let target = TARGETS
        .into_iter()
        .find(|t| *t == path)
        .expect("one of the four volume targets");
    probe(root, &mounts(root), root.boot_id().as_deref(), target)
}

/// The volume at `path` when it is verified: Ready, with a generation this boot.
pub fn verified(root: &Root, path: &str) -> Option<Volume> {
    Some(volume(root, path)).filter(|v| v.state == VolumeState::Ready && v.generation.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_states_follow_the_budget() {
        assert_eq!(
            space_state(10, 100, false, false),
            SpaceState::CriticalSpace
        );
        assert_eq!(
            space_state(1 << 40, 1 << 41, false, false),
            SpaceState::Normal
        );
        assert_eq!(
            space_state(80 << 20, 100 << 20, false, false),
            SpaceState::LowSpace
        );
        assert_eq!(
            space_state(1 << 40, 1 << 41, true, false),
            SpaceState::ReadOnlyRisk
        );
        assert_eq!(
            space_state(1 << 40, 1 << 41, true, true),
            SpaceState::Failed
        );
    }
}
