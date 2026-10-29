//! The `storage` section: the four volumes (root, Y2DATA, SD card, Y2Media) with their identity,
//! generation and space, the eMMC/SD block devices and controllers.
// SPDX-License-Identifier: GPL-2.0-only
use super::device_instance;
use crate::commands::observation::util::{
    self, findall_map, get_eq, int, name, number, read, read_limited, realpath, text,
};
use crate::{regex, reply::Failure};
use serde_json::{json, Map, Value};
use std::path::Path;
use y2p_core::mounts::Mount;
use y2p_core::py::PyValue;
use y2p_core::{command, py::truthy, Root};
use y2p_core::{
    json::{dumps_sorted, Separators},
    observation_support::sha2::{Digest, Sha256},
};

pub fn field<'a>(mount: &'a Map<String, Value>, key: &str) -> &'a str {
    mount.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn has_option(mount: &Map<String, Value>, key: &str, option: &str) -> bool {
    mount
        .get(key)
        .and_then(Value::as_array)
        .is_some_and(|list| list.iter().any(|o| o == option))
}

/// Product capacity budget, not electrical thresholds. Never delete user data.
pub(super) fn space_state(
    available: i128,
    total: i128,
    readonly: bool,
    error: bool,
) -> &'static str {
    if error {
        "Failed"
    } else if readonly {
        "ReadOnlyRisk"
    } else if available < (32 * 1024 * 1024).max(total / 20) {
        "CriticalSpace"
    } else if available < (96 * 1024 * 1024).max(total / 10) {
        "LowSpace"
    } else {
        "Normal"
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
fn refused_card(root: &Root) -> Map<String, Value> {
    let Some(Value::Object(last)) = root.json("/run/y2/media.json") else {
        return Map::new();
    };
    if last.get("reason").and_then(Value::as_str) != Some("filesystem_check_failed") {
        return Map::new();
    }
    let instance = last
        .get("source_instance")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let parts: Vec<&str> = if last.get("source_instance").is_some_and(Value::is_string) {
        instance.split(':').collect()
    } else {
        Vec::new()
    };
    if parts.len() != 5
        || device_instance(root, &parts[1..3].join(":")).as_deref() != Some(instance)
    {
        return Map::new();
    }
    let findings = match last.get("filesystem_check") {
        Some(Value::Object(check)) => check.get("findings").cloned().unwrap_or(json!([])),
        _ => json!([]),
    };
    // ['timeout'] for a card too slow to check in 60 s, not one that needs repair (F12).
    serde_json::from_value(json!({"state": "Degraded", "reason": "filesystem_check_failed",
        "filesystem": last.get("filesystem").cloned().unwrap_or(Value::Null), "findings": findings}))
    .expect("literal")
}

/// `media.media_identity`: CID/partition geometry supplies identity when a legal FAT UUID is absent.
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
    let encoded = dumps_sorted(&identity, Separators::Spaced).expect("identity");
    let digest: String = Sha256::digest(encoded.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    identity["id"] = json!(digest);
    identity
}

pub fn storage(root: &Root) -> Result<Value, Failure> {
    let mounts = y2p_core::mounts::mounts(root);
    let boot = root.boot_id();
    let boot_value = text(boot.clone());
    let mut volumes = Vec::new();
    // Index order is a consumer contract: root, Y2DATA, SD, then Y2Media (internal user media).
    for target in ["/", "/data", "/media/sd", "/media/internal"] {
        let found: Vec<&Mount> = mounts.iter().filter(|m| m.path == target).collect();
        // Stacked mounts are ambiguous to consumers; fail closed.
        if found.len() != 1 {
            let mut volume: Map<String, Value> = serde_json::from_value(json!({"path": target,
                "state": "Unavailable", "reason": "not_uniquely_mounted", "generation": null,
                "space_state": "Unavailable"}))
            .expect("literal");
            if target == "/media/sd" && found.is_empty() {
                volume.extend(refused_card(root));
            }
            volumes.push(Value::Object(volume));
            continue;
        }
        let mut item = found[0].value();
        let device_id = field(&item, "device_id").to_owned();
        let mount_id = item.get("mount_id").cloned().unwrap_or(Value::Null);
        item.insert(
            "generation".into(),
            match boot.as_deref().filter(|b| !b.is_empty()) {
                Some(b) => json!(format!("{b}:{mount_id}:{device_id}")),
                None => Value::Null,
            },
        );
        let device = root.path(&format!("/sys/dev/block/{device_id}"));
        let real = realpath(&device).to_string_lossy().into_owned();
        let expected = if target == "/media/sd" {
            "11240000.mmc"
        } else {
            "11230000.mmc"
        };
        let instance = device_instance(root, &device_id);
        let controller_valid = real.contains(expected) && device.exists();
        item.insert("source_instance".into(), text(instance.clone()));
        item.insert("controller_valid".into(), json!(controller_valid));
        let mut state = if controller_valid { "Ready" } else { "Failed" };
        let mut reason: Option<&str> =
            (!controller_valid).then_some("source_missing_or_wrong_controller");
        let mut uuid = Value::Null;
        let claim = root.json_object("/run/y2/media-mount.json");
        if controller_valid {
            let source = field(&item, "source").to_owned();
            let answer = command::run(
                root,
                &["blkid", "-p", "-s", "UUID", "-o", "value", &source],
                command::TIMEOUT,
            );
            if answer.ok {
                uuid = answer
                    .output
                    .filter(|o| !o.is_empty())
                    .map_or(Value::Null, Value::String);
            }
            let expected_uuid = match target {
                "/" => Some("79324c69-6e75-4801-8000-000000000101"),
                "/data" => Some("79324c69-6e75-4801-8000-000000000102"),
                "/media/internal" => Some("79324c69-6e75-4801-8000-000000000103"),
                _ => None,
            };
            if let Some(expected) = expected_uuid {
                if uuid.as_str() != Some(expected) {
                    (state, reason) = ("Failed", Some("filesystem_identity_invalid"));
                }
            }
            if target == "/media/sd" {
                let filesystem = item.get("filesystem").cloned().unwrap_or(Value::Null);
                let identity = media_identity(&device, &filesystem, &uuid);
                item.insert("identity".into(), identity.clone());
                let matches = [
                    ("boot_id", boot_value.clone()),
                    ("mount_id", mount_id.clone()),
                    ("device_id", json!(device_id)),
                    ("uuid", uuid.clone()),
                ]
                .iter()
                .all(|(k, v)| get_eq(&claim, k, v));
                if !matches {
                    (state, reason) = ("Failed", Some("mount_claim_changed_or_missing"));
                }
                let claimed = claim.get("identity").filter(|v| !v.is_null());
                if state == "Ready"
                    && (identity.is_null() || claimed.is_some_and(|c| !c.py_eq(&identity)))
                {
                    (state, reason) = ("Failed", Some("media_identity_changed_or_missing"));
                }
                if instance.is_none() || !get_eq(&claim, "source_instance", &text(instance.clone()))
                {
                    (state, reason) = ("Failed", Some("block_instance_changed"));
                }
            }
        }
        item.insert("uuid".into(), uuid);
        let readonly =
            has_option(&item, "options", "ro") || has_option(&item, "super_options", "ro");
        let mut protected = false;
        if target == "/media/sd" && readonly {
            // media.py mounts a healthy card read-only between writes: it is Ready; so is one that
            // plays while its check finishes in the background (F49).
            protected = matches!(
                claim.get("read_only").and_then(Value::as_str),
                Some("protected" | "checking")
            );
        }
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
                let mut space =
                    space_state(available, total, readonly && !protected, state == "Failed");
                if vfs.files > 0 && vfs.favail == 0 {
                    space = "CriticalSpace";
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
                item.insert("space_state".into(), json!("Failed"));
                (state, reason) = ("Failed", Some("statvfs_failed"));
            }
        }
        if state == "Ready" && readonly && !protected {
            (state, reason) = ("Degraded", Some("read_only"));
            if target == "/media/sd" {
                // Why the platform mounted the card read-only (media.py): its check found errors or
                // did not finish; Repair can make it writable.
                let cause = claim.get("read_only").and_then(Value::as_str);
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
