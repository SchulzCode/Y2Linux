//! The `storage` section: the four volumes (root, Y2DATA, SD card, Y2Media) with their identity,
//! generation and space, the eMMC/SD block devices and controllers.
// SPDX-License-Identifier: GPL-2.0-only
use super::device_instance;
use crate::commands::observation::util::{
    self, findall_map, int, name, number, read, read_limited, realpath, record_fields, text,
};
use crate::{regex, reply::Failure};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::path::Path;
use y2p_core::json::{dumps_sorted, Object, ObjectExt, Separators};
use y2p_core::py::PyValue;
use y2p_core::volume::{SpaceState, VolumeState};
use y2p_core::{command, py::truthy, Root};

/// The filesystem UUIDs the image's own volumes are made with.
fn expected_uuid(target: &str) -> Option<&'static str> {
    match target {
        "/" => Some("79324c69-6e75-4801-8000-000000000101"),
        "/data" => Some("79324c69-6e75-4801-8000-000000000102"),
        "/media/internal" => Some("79324c69-6e75-4801-8000-000000000103"),
        _ => None,
    }
}

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

/// Product capacity budget, not electrical thresholds. Never delete user data.
pub(super) fn space_state(available: i128, total: i128, readonly: bool, error: bool) -> SpaceState {
    if error {
        SpaceState::Failed
    } else if readonly {
        SpaceState::ReadOnlyRisk
    } else if available < (32 * 1024 * 1024).max(total / 20) {
        SpaceState::CriticalSpace
    } else if available < (96 * 1024 * 1024).max(total / 10) {
        SpaceState::LowSpace
    } else {
        SpaceState::Normal
    }
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
fn refused_card(root: &Root) -> Object {
    let last: LastOperation = record_fields(root, "/run/y2/media.json");
    if last.reason.as_str() != Some("filesystem_check_failed") {
        return Object::new();
    }
    // `block:<major>:<minor>:diskseq:<n>`: still this card while the same disk sequence is there.
    let Some(instance) = last.source_instance.as_str() else {
        return Object::new();
    };
    let parts: Vec<&str> = instance.split(':').collect();
    if parts.len() != 5
        || device_instance(root, &parts[1..3].join(":")).as_deref() != Some(instance)
    {
        return Object::new();
    }
    let findings = match &last.filesystem_check {
        Value::Object(check) => check.get("findings").cloned().unwrap_or(json!([])),
        _ => json!([]),
    };
    // ['timeout'] for a card too slow to check in 60 s, not one that needs repair (F12).
    Object::new()
        .with("state", "Degraded")
        .with("reason", "filesystem_check_failed")
        .with("filesystem", last.filesystem)
        .with("findings", findings)
}

/// The SD card's identity: its UUID, or its CID and partition geometry where FAT has no usable UUID;
/// the SHA-256 of it is the `id` the mount claim is compared by.
fn media_identity(entry: &Path, filesystem: &Value, uuid: &Value) -> Value {
    let mut device = realpath(entry);
    let partition = read(&entry.join("partition"));
    if partition.is_some() {
        device = device.parent().map(Path::to_path_buf).unwrap_or(device);
    }
    let cid = read(&device.join("device/cid"));
    let start = if partition.is_some() {
        read(&entry.join("start"))
    } else {
        Some("0".into())
    };
    let size = read(&entry.join("size"));
    let digits = |v: &Option<String>| {
        v.as_ref()
            .is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
    };
    if !truthy(uuid)
        && (cid.as_deref().is_none_or(str::is_empty) || !digits(&start) || !digits(&size))
    {
        return Value::Null;
    }
    let mut identity = json!({"cid": cid, "geometry": {"partition": partition, "start": start, "size": size},
                              "filesystem": filesystem, "uuid": uuid});
    // Texts and nulls only: nothing here a JSON text cannot hold.
    let encoded = dumps_sorted(&identity, Separators::Spaced).expect("identity");
    identity["id"] = json!(y2p_core::digest::sha256_hex(encoded.as_bytes()));
    identity
}

pub fn storage(root: &Root) -> Result<Value, Failure> {
    let mounts = y2p_core::mounts::mounts(root);
    let boot = root.boot_id();
    let boot_value = text(boot.clone());
    let mut volumes = Vec::new();
    // Index order is a consumer contract: root, Y2DATA, SD, then Y2Media (internal user media).
    for target in ["/", "/data", "/media/sd", "/media/internal"] {
        let found: Vec<_> = mounts.iter().filter(|m| m.path == target).collect();
        // Stacked mounts are ambiguous to consumers; fail closed.
        let [mount] = found[..] else {
            let mut volume = Object::new()
                .with("path", target)
                .with("state", "Unavailable")
                .with("reason", "not_uniquely_mounted")
                .with("generation", Value::Null)
                .with("space_state", "Unavailable");
            if target == "/media/sd" && found.is_empty() {
                volume.extend(refused_card(root));
            }
            volumes.push(Value::Object(volume));
            continue;
        };
        let mut item = mount.value();
        let device_id = mount.device_id.as_str();
        let mount_id = mount.mount_id;
        item.insert(
            "generation".into(),
            match boot.as_deref().filter(|b| !b.is_empty()) {
                Some(b) => json!(format!("{b}:{mount_id}:{device_id}")),
                None => Value::Null,
            },
        );
        let device = root.path(&format!("/sys/dev/block/{device_id}"));
        let real = realpath(&device).to_string_lossy().into_owned();
        // The SD card on MSDC1, everything else on the eMMC (MSDC0).
        let expected = if target == "/media/sd" {
            "11240000.mmc"
        } else {
            "11230000.mmc"
        };
        let instance = device_instance(root, device_id);
        let controller_valid = real.contains(expected) && device.exists();
        item.insert("source_instance".into(), text(instance.clone()));
        item.insert("controller_valid".into(), json!(controller_valid));
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
                let identity = media_identity(&device, &json!(mount.filesystem), &uuid);
                item.insert("identity".into(), identity.clone());
                let matches = claim.boot_id.py_eq(&boot_value)
                    && claim.mount_id.py_eq(&json!(mount_id))
                    && claim.device_id.py_eq(&json!(device_id))
                    && claim.uuid.py_eq(&uuid);
                if !matches {
                    (state, reason) = (VolumeState::Failed, Some("mount_claim_changed_or_missing"));
                }
                let claimed_other = !claim.identity.is_null() && !claim.identity.py_eq(&identity);
                if state == VolumeState::Ready && (identity.is_null() || claimed_other) {
                    (state, reason) = (
                        VolumeState::Failed,
                        Some("media_identity_changed_or_missing"),
                    );
                }
                if instance.is_none() || !claim.source_instance.py_eq(&text(instance.clone())) {
                    (state, reason) = (VolumeState::Failed, Some("block_instance_changed"));
                }
            }
        }
        item.insert("uuid".into(), uuid);
        let readonly = mount.read_only();
        // The storage group mounts a healthy card read-only between writes: it is Ready; so is one that
        // plays while its check finishes in the background (F49).
        let protected = target == "/media/sd"
            && readonly
            && matches!(claim.read_only.as_str(), Some("protected" | "checking"));
        if target == "/media/sd" {
            item.insert("writable".into(), json!(!readonly));
        }
        match y2p_core::sys::statvfs(&root.path(target)).ok() {
            Some(vfs) => {
                let frsize = i128::from(vfs.frsize);
                let total = i128::from(vfs.blocks) * frsize;
                let available = i128::from(vfs.bavail) * frsize;
                item.insert("total_bytes".into(), json!(total as u64));
                item.insert("available_bytes".into(), json!(available as u64));
                item.insert(
                    "free_bytes".into(),
                    json!((i128::from(vfs.bfree) * frsize) as u64),
                );
                item.insert("available_inodes".into(), json!(vfs.favail));
                let mut space = space_state(
                    available,
                    total,
                    readonly && !protected,
                    state == VolumeState::Failed,
                );
                if vfs.files > 0 && vfs.favail == 0 {
                    space = SpaceState::CriticalSpace;
                }
                item.insert("space_state".into(), json!(space));
                if target == "/media/sd" {
                    // A filesystem far smaller than its partition (a copied image, an old partition
                    // layout) hides most of the card; report it, never auto-fix it.
                    if let Some(sectors) = read(&device.join("size")).filter(|s| util::isdigit(s)) {
                        let partition = i128::from(util::int_or_raise(&sectors)?) * 512;
                        item.insert("partition_bytes".into(), json!(partition as u64));
                        item.insert(
                            "capacity_state".into(),
                            json!(if (total as f64) < 0.9 * partition as f64 {
                                "Undersized"
                            } else {
                                "Full"
                            }),
                        );
                    }
                }
            }
            None => {
                item.insert("space_state".into(), json!(SpaceState::Failed));
                (state, reason) = (VolumeState::Failed, Some("statvfs_failed"));
            }
        }
        if state == VolumeState::Ready && readonly && !protected {
            (state, reason) = (VolumeState::Degraded, Some("read_only"));
            if target == "/media/sd" {
                // Why the storage group mounted the card read-only: its check found errors or
                // did not finish; Repair can make it writable.
                let cause = claim.read_only.as_str();
                item.insert(
                    "read_only_reason".into(),
                    json!(cause
                        .filter(|c| matches!(*c, "filesystem_errors" | "check_timed_out"))
                        .unwrap_or("mounted_read_only")),
                );
            }
        }
        item.insert("state".into(), json!(state));
        item.insert("reason".into(), json!(reason));
        volumes.push(Value::Object(item));
    }
    let mut blocks = Vec::new();
    for path in root.glob("/sys/class/block/*") {
        let real = realpath(&path).to_string_lossy().into_owned();
        if !["11230000.mmc", "11240000.mmc"]
            .iter()
            .any(|c| real.contains(c))
        {
            continue;
        }
        blocks.push(
            json!({"name": name(&path), "stat": read(&path.join("stat")),
            "stat_unit": "Linux_block_stat_fields_sectors_512_bytes",
            "size_sectors": number(read(&path.join("size")).as_deref()),
            "type": read(&path.join("device/type")), "name_id": read(&path.join("device/name")),
            "cid": read(&path.join("device/cid")), "serial": read(&path.join("device/serial")),
            "io_error_count": number(read(&path.join("device/ioerr_cnt")).as_deref())}),
        );
    }
    let mut controllers = Vec::new();
    for path in root.glob("/sys/bus/platform/devices/*.mmc/y2_performance") {
        let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut controller = Map::new();
        controller.insert("name".into(), json!(name(&parent)));
        for (key, value) in findall_map(
            regex!(r"(cap_hz|actual_hz|transport_errors|fallbacks|clock_error)=(-?\d+)"),
            read(&path).as_deref().unwrap_or(""),
        ) {
            controller.insert(key, json!(util::int_or_raise(&value)?));
        }
        controller.insert(
            "mode".into(),
            storage_mode(read_limited(&parent.join("y2_storage"), 4096).as_deref()),
        );
        controller.insert(
            "runtime_pm".into(),
            storage_mode(read_limited(&parent.join("y2_runtime_pm"), 4096).as_deref()),
        );
        controller.insert(
            "runtime_status".into(),
            text(read(&parent.join("power/runtime_status"))),
        );
        controller.insert(
            "tuning".into(),
            text(read_limited(&parent.join("y2_tune_diag"), 4096)),
        );
        controllers.push(Value::Object(controller));
    }
    Ok(json!({
        "volumes": volumes, "blocks": blocks, "controllers": controllers,
        "internal_setup": crate::commands::observation::system::setup_status(root),
        "media": {"last_operation": root.json("/run/y2/media.json"),
                  "lifecycle": root.json("/run/y2/media-lifecycle.json")},
        "bus_parameters": root.read("/sys/kernel/debug/mmc0/ios"),
        "sd_bus_parameters": root.read("/sys/kernel/debug/mmc1/ios"),
    }))
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
