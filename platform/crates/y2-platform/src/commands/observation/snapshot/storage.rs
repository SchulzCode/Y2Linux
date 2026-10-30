//! The `storage` section: the four volumes (root, Y2DATA, SD card, Y2Media) with their identity,
//! generation and space, the eMMC/SD block devices and controllers.
// SPDX-License-Identifier: GPL-2.0-only
use super::device_instance;
use crate::commands::observation::util::{
    self, findall_map, int, name, read, read_limited, realpath, record_fields, text,
};
use crate::{regex, reply::Failure};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::{collections::BTreeMap, path::Path};
use y2p_core::volume::{
    controller, expected_uuid, SpaceState, VolumeState, EMMC_CONTROLLER, SD_CONTROLLER,
};
use y2p_core::{command, mounts::Mount, Root};

/// `/run/y2/media-mount.json`: what the platform mounted at /media/sd (storage group), checked
/// against the live mount before the card counts as Ready.
#[derive(Deserialize, Default)]
#[serde(default)]
struct MountClaim {
    boot_id: Value,
    mount_id: Value,
    device_id: Value,
    uuid: Value,
    source_instance: Value,
    identity: Value,
    /// Why the card is mounted read-only: `protected` (between writes), `checking` (F49),
    /// `filesystem_errors`, `check_timed_out`.
    read_only: Value,
}

/// `/run/y2/media.json`: the last media operation's answer.
#[derive(Deserialize, Default)]
#[serde(default)]
struct LastOperation {
    reason: Value,
    source_instance: Value,
    filesystem: Value,
    filesystem_check: Value,
}

/// Product capacity budget, not electrical thresholds. Never delete user data (statvfs byte counts are
/// never negative).
pub(super) fn space_state(available: i128, total: i128, readonly: bool, error: bool) -> SpaceState {
    let bytes = |n: i128| u64::try_from(n).unwrap_or(if n < 0 { 0 } else { u64::MAX });
    y2p_core::volume::space_state(bytes(available), bytes(total), readonly, error)
}

/// The driver's negotiated-mode line as typed fields; absent on older kernels.
pub fn storage_mode(text: Option<&str>) -> Value {
    let Some(text) = text.filter(|t| !t.is_empty()) else {
        return Value::Null;
    };
    let mut mode = Map::new();
    for (key, value) in util::findall2(regex!(r"([a-z0-9_]+)=(\S+)"), text) {
        let typed = if regex!(r"\A-?\d+\z").is_match(&value) && value.is_ascii() {
            int(&value).map_or(Value::Null, Value::from)
        } else if regex!(r"\A0x[0-9a-f]+\z").is_match(&value) {
            i64::from_str_radix(&value[2..], 16).map_or(Value::Null, Value::from)
        } else {
            json!(value)
        };
        mode.insert(key, typed);
    }
    Value::Object(mode)
}

/// The card still inserted that the last mount refused (its check failed), so the player can say so
/// instead of "no card" (issue #44); {} otherwise. A removed or reinserted card has another disk
/// sequence number.
fn refused_card(root: &Root) -> Option<RefusedCard> {
    let last: LastOperation = record_fields(root, "/run/y2/media.json");
    if last.reason.as_str() != Some("filesystem_check_failed") {
        return None;
    }
    // `block:<major>:<minor>:diskseq:<n>`: still this card while the same disk sequence is there.
    let instance = last.source_instance.as_str()?;
    let parts: Vec<&str> = instance.split(':').collect();
    if parts.len() != 5
        || device_instance(root, &parts[1..3].join(":")).as_deref() != Some(instance)
    {
        return None;
    }
    let findings = match &last.filesystem_check {
        Value::Object(check) => check.get("findings").cloned().unwrap_or(json!([])),
        _ => json!([]),
    };
    // ['timeout'] for a card too slow to check in 60 s, not one that needs repair (F12).
    Some(RefusedCard {
        filesystem: last.filesystem,
        findings,
    })
}

/// An SD card the storage group refused to mount after a failed filesystem check (still inserted).
#[derive(Serialize)]
pub struct RefusedCard {
    pub filesystem: Value,
    pub findings: Value,
}

/// The volume at `target` (one of the four), probed alone: no other volume's `blkid` or `statvfs`.
pub fn volume_at(root: &Root, target: &str) -> Result<Volume, Failure> {
    volume(
        root,
        &y2p_core::mounts::mounts(root),
        &root.boot_id(),
        target,
    )
}

/// One volume of the section: its mount, controller, identity, claim, space and state.
fn volume(
    root: &Root,
    mounts: &[Mount],
    boot: &Option<String>,
    target: &str,
) -> Result<Volume, Failure> {
    let boot_value = text(boot.clone());
    let found: Vec<_> = mounts.iter().filter(|m| m.path == target).collect();
    // Stacked mounts are ambiguous to consumers; fail closed.
    let [mount] = found[..] else {
        let mut volume = Volume::new(None);
        volume.path = Some(target.to_owned());
        volume.space_state = SpaceState::Unavailable;
        (volume.state, volume.reason) = (VolumeState::Unavailable, Some("not_uniquely_mounted"));
        if target == "/media/sd" && found.is_empty() {
            volume.refused = refused_card(root);
            if volume.refused.is_some() {
                (volume.state, volume.reason) =
                    (VolumeState::Degraded, Some("filesystem_check_failed"));
            }
        }
        return Ok(volume);
    };
    let device_id = mount.device_id.as_str();
    let mount_id = mount.mount_id;
    let mut item = Volume::new(Some(mount.clone()));
    item.generation = boot
        .as_deref()
        .filter(|b| !b.is_empty())
        .map(|b| format!("{b}:{mount_id}:{device_id}"));
    let device = root.path(&format!("/sys/dev/block/{device_id}"));
    let real = realpath(&device).to_string_lossy().into_owned();
    // The SD card on MSDC1, everything else on the eMMC (MSDC0).
    let expected = controller(target);
    let instance = device_instance(root, device_id);
    let controller_valid = real.contains(expected) && device.exists();
    item.source_instance = instance.clone();
    item.controller_valid = controller_valid;
    let mut state = if controller_valid {
        VolumeState::Ready
    } else {
        VolumeState::Failed
    };
    let mut reason: Option<&str> =
        (!controller_valid).then_some("source_missing_or_wrong_controller");
    let mut uuid = Value::Null;
    let claim: MountClaim = record_fields(root, "/run/y2/media-mount.json");
    if controller_valid {
        let answer = command::run(
            root,
            &["blkid", "-p", "-s", "UUID", "-o", "value", &mount.source],
            command::TIMEOUT,
        );
        if answer.ok {
            uuid = answer
                .output
                .filter(|o| !o.is_empty())
                .map_or(Value::Null, Value::String);
        }
        if expected_uuid(target).is_some_and(|expected| uuid.as_str() != Some(expected)) {
            (state, reason) = (VolumeState::Failed, Some("filesystem_identity_invalid"));
        }
        if target == "/media/sd" {
            let identity = crate::commands::storage::observe::media_identity(
                &device,
                &mount.filesystem,
                uuid.as_str(),
            )
            .map_or(Value::Null, Value::Object);
            item.identity = Some(identity.clone());
            let matches = claim.boot_id == boot_value
                && claim.mount_id == json!(mount_id)
                && claim.device_id == json!(device_id)
                && claim.uuid == uuid;
            if !matches {
                (state, reason) = (VolumeState::Failed, Some("mount_claim_changed_or_missing"));
            }
            let claimed_other = !claim.identity.is_null() && claim.identity != identity;
            if state == VolumeState::Ready && (identity.is_null() || claimed_other) {
                (state, reason) = (
                    VolumeState::Failed,
                    Some("media_identity_changed_or_missing"),
                );
            }
            if instance.is_none() || claim.source_instance != text(instance.clone()) {
                (state, reason) = (VolumeState::Failed, Some("block_instance_changed"));
            }
        }
    }
    item.uuid = uuid;
    let readonly = mount.read_only();
    // The storage group mounts a healthy card read-only between writes: it is Ready; so is one that
    // plays while its check finishes in the background (F49).
    let protected = target == "/media/sd"
        && readonly
        && matches!(claim.read_only.as_str(), Some("protected" | "checking"));
    if target == "/media/sd" {
        item.writable = Some(!readonly);
    }
    match y2p_core::sys::statvfs(&root.path(target)).ok() {
        Some(vfs) => {
            let frsize = i128::from(vfs.frsize);
            let total = i128::from(vfs.blocks) * frsize;
            let available = i128::from(vfs.bavail) * frsize;
            item.total_bytes = Some(total as u64);
            item.available_bytes = Some(available as u64);
            item.free_bytes = Some((i128::from(vfs.bfree) * frsize) as u64);
            item.available_inodes = Some(vfs.favail);
            let mut space = space_state(
                available,
                total,
                readonly && !protected,
                state == VolumeState::Failed,
            );
            if vfs.files > 0 && vfs.favail == 0 {
                space = SpaceState::CriticalSpace;
            }
            item.space_state = space;
            if target == "/media/sd" {
                // A filesystem far smaller than its partition (a copied image, an old partition
                // layout) hides most of the card; report it, never auto-fix it.
                if let Some(sectors) = read(&device.join("size")).filter(|s| util::isdigit(s)) {
                    let partition = i128::from(util::int_or_raise(&sectors)?) * 512;
                    item.partition_bytes = Some(partition as u64);
                    item.capacity_state = Some(if (total as f64) < 0.9 * partition as f64 {
                        "Undersized"
                    } else {
                        "Full"
                    });
                }
            }
        }
        None => {
            item.space_state = SpaceState::Failed;
            (state, reason) = (VolumeState::Failed, Some("statvfs_failed"));
        }
    }
    if state == VolumeState::Ready && readonly && !protected {
        (state, reason) = (VolumeState::Degraded, Some("read_only"));
        if target == "/media/sd" {
            // Why the storage group mounted the card read-only: its check found errors or
            // did not finish; Repair can make it writable.
            let cause = claim.read_only.as_str();
            item.read_only_reason = Some(
                cause
                    .filter(|c| matches!(*c, "filesystem_errors" | "check_timed_out"))
                    .unwrap_or("mounted_read_only")
                    .to_owned(),
            );
        }
    }
    item.state = state;
    item.reason = reason;
    Ok(item)
}

pub fn storage(root: &Root) -> Result<Storage, Failure> {
    let mounts = y2p_core::mounts::mounts(root);
    let boot = root.boot_id();
    let mut volumes = Vec::new();
    // Index order is a consumer contract: root, Y2DATA, SD, then Y2Media (internal user media).
    for target in ["/", "/data", "/media/sd", "/media/internal"] {
        volumes.push(volume(root, &mounts, &boot, target)?);
    }
    let mut blocks = Vec::new();
    for path in root.glob("/sys/class/block/*") {
        let real = realpath(&path).to_string_lossy().into_owned();
        if ![EMMC_CONTROLLER, SD_CONTROLLER]
            .iter()
            .any(|c| real.contains(c))
        {
            continue;
        }
        blocks.push(Block {
            name: name(&path),
            stat: read(&path.join("stat")),
            stat_unit: "Linux_block_stat_fields_sectors_512_bytes",
            size_sectors: read(&path.join("size")).as_deref().and_then(util::int),
            kind: read(&path.join("device/type")),
            name_id: read(&path.join("device/name")),
            cid: read(&path.join("device/cid")),
            serial: read(&path.join("device/serial")),
            io_error_count: read(&path.join("device/ioerr_cnt"))
                .as_deref()
                .and_then(util::int),
        });
    }
    let mut controllers = Vec::new();
    for path in root.glob("/sys/bus/platform/devices/*.mmc/y2_performance") {
        let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut performance = BTreeMap::new();
        for (key, value) in findall_map(
            regex!(r"(cap_hz|actual_hz|transport_errors|fallbacks|clock_error)=(-?[0-9]+)"),
            read(&path).as_deref().unwrap_or(""),
        ) {
            performance.insert(key, util::int_or_raise(&value)?);
        }
        controllers.push(Controller {
            name: name(&parent),
            performance,
            mode: storage_mode(read_limited(&parent.join("y2_storage"), 4096).as_deref()),
            runtime_pm: storage_mode(read_limited(&parent.join("y2_runtime_pm"), 4096).as_deref()),
            runtime_status: read(&parent.join("power/runtime_status")),
            tuning: read_limited(&parent.join("y2_tune_diag"), 4096),
        });
    }
    Ok(Storage {
        volumes,
        blocks,
        controllers,
        internal_setup: crate::commands::storage::observe::setup_status(root),
        media: Media {
            last_operation: root.json("/run/y2/media.json"),
            lifecycle: root.json("/run/y2/media-lifecycle.json"),
        },
        bus_parameters: root.read("/sys/kernel/debug/mmc0/ios"),
        sd_bus_parameters: root.read("/sys/kernel/debug/mmc1/ios"),
    })
}

/// The `storage` section, in this key order.
#[derive(Serialize)]
pub struct Storage {
    /// Root, Y2DATA, SD, then Y2Media: the index order is a consumer contract.
    pub volumes: Vec<Volume>,
    blocks: Vec<Block>,
    controllers: Vec<Controller>,
    internal_setup: Map<String, Value>,
    media: Media,
    bus_parameters: Option<String>,
    sd_bus_parameters: Option<String>,
}

#[derive(Serialize)]
pub struct Media {
    last_operation: Option<Value>,
    lifecycle: Option<Value>,
}

/// One uniquely mounted volume: the mount's own fields, then what the snapshot checked (a key left
/// out where the snapshot does not show it: identity and writable only for the SD card, the space
/// figures only with statvfs, the capacity only for an SD card whose partition size reads).
#[derive(Serialize)]
pub struct Volume {
    /// The mount's own fields (none for a volume not mounted exactly once).
    #[serde(flatten)]
    pub mount: Option<Mount>,
    /// The expected path of a volume not mounted exactly once.
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(flatten)]
    pub refused: Option<RefusedCard>,
    generation: Option<String>,
    source_instance: Option<String>,
    controller_valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    identity: Option<Value>,
    pub uuid: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    writable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub available_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    free_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    available_inodes: Option<u64>,
    pub space_state: SpaceState,
    #[serde(skip_serializing_if = "Option::is_none")]
    partition_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    capacity_state: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    read_only_reason: Option<String>,
    pub state: VolumeState,
    pub reason: Option<&'static str>,
}

/// One block device on the eMMC or SD controller.
impl Volume {
    /// Nothing checked yet: the checks set space and state (failed until they do).
    fn new(mount: Option<Mount>) -> Volume {
        Volume {
            mount,
            path: None,
            refused: None,
            generation: None,
            source_instance: None,
            controller_valid: false,
            identity: None,
            uuid: Value::Null,
            writable: None,
            total_bytes: None,
            available_bytes: None,
            free_bytes: None,
            available_inodes: None,
            space_state: SpaceState::Failed,
            partition_bytes: None,
            capacity_state: None,
            read_only_reason: None,
            state: VolumeState::Failed,
            reason: None,
        }
    }

    /// Where the volume is (or should be) mounted.
    pub fn path(&self) -> &str {
        self.mount
            .as_ref()
            .map_or(self.path.as_deref().unwrap_or_default(), |m| {
                m.path.as_str()
            })
    }
}

#[derive(Serialize)]
pub struct Block {
    name: String,
    stat: Option<String>,
    stat_unit: &'static str,
    size_sectors: Option<i64>,
    #[serde(rename = "type")]
    kind: Option<String>,
    name_id: Option<String>,
    cid: Option<String>,
    serial: Option<String>,
    io_error_count: Option<i64>,
}

/// One MMC controller: its performance counters (by name), modes and runtime state.
#[derive(Serialize)]
pub struct Controller {
    name: String,
    #[serde(flatten)]
    performance: BTreeMap<String, i64>,
    mode: Value,
    runtime_pm: Value,
    runtime_status: Option<String>,
    tuning: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_lines_carry_every_field() {
        let mounts = y2p_core::mounts::mountinfo(
            "30 22 179:9 / /data rw,noatime - ext4 /dev/mmcblk0p9 rw\nx\n31 1 0:1 / /my\\040dir rw - tmpfs t rw",
        );
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0].value()["options"], json!(["rw", "noatime"]));
        assert_eq!(mounts[1].path, "/my dir");
    }
}
