//! The storage group's copies of `observe.storage` (its `volumes`, which `media status` and `space`
//! print), `observe.refused_card`, `observe.space_state`, `media.media_identity` and
//! `internal_media.setup_status`. The `blocks`, `controllers` and the rest of `storage()` are only
//! reads for `status` (group observation) and are not needed here.
// SPDX-License-Identifier: GPL-2.0-only
use super::sys::{
    boot_id, device_instance, exists, get, get_eq, get_in, isdigit, mounts, object, py_eq, read,
    resolve, statvfs, text, Object,
};
use serde_json::{json, Value};
use std::path::Path;
use y2p_core::{
    command,
    json::{dumps_sorted, Separators},
    storage_media::sha256_hex,
    Root,
};

pub const MEDIA_MOUNT: &str = "/run/y2/media-mount.json";
pub const MEDIA_STATE: &str = "/run/y2/media.json";
pub const SETUP_STATUS: &str = "/run/y2/internal-media-setup.json";

/// `media.media_identity`: CID/partition geometry supplies identity when a legal FAT UUID is absent.
pub fn media_identity(entry: &Path, filesystem: &str, uuid: Option<&str>) -> Option<Object> {
    let mut device = resolve(entry);
    let partition = read(&entry.join("partition"));
    if partition.is_some() {
        if let Some(parent) = device.parent() {
            device = parent.to_path_buf();
        }
    }
    let cid = read(&device.join("device/cid"));
    let start = if partition.is_some() {
        read(&entry.join("start"))
    } else {
        Some("0".into())
    };
    let size = read(&entry.join("size"));
    let usable = |v: &Option<String>| v.as_deref().is_some_and(isdigit);
    if uuid.is_none_or(str::is_empty)
        && (cid.as_deref().is_none_or(str::is_empty) || !usable(&start) || !usable(&size))
    {
        return None;
    }
    let mut identity = Object::new();
    identity.insert("cid".into(), text(&cid));
    identity.insert(
        "geometry".into(),
        json!({"partition": partition, "start": start, "size": size}),
    );
    identity.insert("filesystem".into(), json!(filesystem));
    identity.insert("uuid".into(), json!(uuid));
    // json.dumps(identity, sort_keys=True): Python's default separators, byte for byte.
    let encoded = dumps_sorted(&identity, Separators::Spaced).expect("identity");
    identity.insert("id".into(), json!(sha256_hex(encoded.as_bytes())));
    Some(identity)
}

/// `observe.space_state`: product capacity budget, not electrical thresholds. Never delete user data.
pub fn space_state(
    available: Option<u64>,
    total: Option<u64>,
    readonly: bool,
    error: bool,
) -> &'static str {
    if error {
        return "Failed";
    }
    if readonly {
        return "ReadOnlyRisk";
    }
    let (Some(available), Some(total)) = (available, total) else {
        return "Unavailable";
    };
    if available < (32 << 20u64).max(total / 20) {
        return "CriticalSpace";
    }
    if available < (96 << 20u64).max(total / 10) {
        return "LowSpace";
    }
    "Normal"
}

/// `observe.refused_card`: the card still inserted that the last mount refused (its check failed),
/// so the player can say so instead of "no card" (issue #44); {} otherwise. A removed or reinserted
/// card has another disk sequence number.
pub fn refused_card(root: &Root) -> Object {
    let Some(Value::Object(last)) = root.json(MEDIA_STATE) else {
        return Object::new();
    };
    if get(&last, "reason").as_str() != Some("filesystem_check_failed") {
        return Object::new();
    }
    let instance = get(&last, "source_instance").as_str().unwrap_or_default();
    let parts: Vec<&str> = if get(&last, "source_instance").is_string() {
        instance.split(':').collect()
    } else {
        Vec::new()
    };
    if parts.len() != 5
        || device_instance(root, &parts[1..3].join(":")).as_deref() != Some(instance)
    {
        return Object::new();
    }
    let check = match last.get("filesystem_check") {
        Some(Value::Object(check)) => check.clone(),
        _ => Object::new(),
    };
    // ['timeout'] for a card too slow to check in 60 s, not one that needs repair (F12).
    let mut value = Object::new();
    value.insert("state".into(), json!("Degraded"));
    value.insert("reason".into(), json!("filesystem_check_failed"));
    value.insert("filesystem".into(), get(&last, "filesystem").clone());
    value.insert(
        "findings".into(),
        check.get("findings").cloned().unwrap_or(json!([])),
    );
    value
}

/// `internal_media.setup_status`: this boot's setup state, else `{'state': 'Unknown'}`.
pub fn setup_status(root: &Root) -> Object {
    match root.json(SETUP_STATUS) {
        Some(Value::Object(value)) if get_eq(&value, "boot_id", &boot_id(root)) => {
            ["state", "reason", "files", "bytes", "serial", "kept"]
                .iter()
                .filter_map(|k| value.get(*k).map(|v| (k.to_string(), v.clone())))
                .collect()
        }
        _ => [("state".to_string(), json!("Unknown"))]
            .into_iter()
            .collect(),
    }
}

/// `observe.storage(ctx)['volumes']`: root, Y2DATA, SD, then Y2Media (index order is a consumer
/// contract). Runs `blkid` for every mount on its controller, in this order.
// statvfs field widths differ between the host and the player's 32-bit ARM: the casts are needed.
#[allow(clippy::unnecessary_cast)]
pub fn volumes(root: &Root) -> Vec<Object> {
    let mounts = mounts(root);
    let boot = root.read("/proc/sys/kernel/random/boot_id");
    let mut volumes = Vec::new();
    for target in ["/", "/data", "/media/sd", "/media/internal"] {
        let sd = target == "/media/sd";
        let found: Vec<_> = mounts.iter().filter(|m| m.path == target).collect();
        // Stacked mounts are ambiguous to consumers; fail closed.
        if found.len() != 1 {
            let mut volume = Object::new();
            volume.insert("path".into(), json!(target));
            volume.insert("state".into(), json!("Unavailable"));
            volume.insert("reason".into(), json!("not_uniquely_mounted"));
            volume.insert("generation".into(), Value::Null);
            volume.insert("space_state".into(), json!("Unavailable"));
            if sd && found.is_empty() {
                volume.extend(refused_card(root));
            }
            volumes.push(volume);
            continue;
        }
        let mount = found[0];
        let mut item = mount.value();
        let set = |item: &mut Object, key: &str, value: Value| {
            item.insert(key.into(), value);
        };
        set(
            &mut item,
            "generation",
            match boot.as_deref() {
                Some(b) if !b.is_empty() => {
                    json!(format!("{b}:{}:{}", mount.mount_id, mount.device_id))
                }
                _ => Value::Null,
            },
        );
        let device = root.path(&format!("/sys/dev/block/{}", mount.device_id));
        let real = resolve(&device).to_string_lossy().into_owned();
        let expected = if sd { "11240000.mmc" } else { "11230000.mmc" };
        let device_exists = exists(&device);
        let instance = device_instance(root, &mount.device_id);
        set(&mut item, "source_instance", text(&instance));
        let controller_valid = real.contains(expected) && device_exists;
        set(&mut item, "controller_valid", json!(controller_valid));
        let failed = |item: &mut Object, reason: &str| {
            item.insert("state".into(), json!("Failed"));
            item.insert("reason".into(), json!(reason));
        };
        if controller_valid {
            set(&mut item, "state", json!("Ready"));
            set(&mut item, "reason", Value::Null);
        } else {
            failed(&mut item, "source_missing_or_wrong_controller");
        }
        set(&mut item, "uuid", Value::Null);
        if controller_valid {
            let answer = command::run(
                root,
                &["blkid", "-p", "-s", "UUID", "-o", "value", &mount.source],
                command::TIMEOUT,
            );
            let uuid = answer
                .output
                .filter(|o| answer.ok && !o.is_empty())
                .map_or(Value::Null, Value::String);
            set(&mut item, "uuid", uuid.clone());
            let expected_uuid = match target {
                "/" => Some("79324c69-6e75-4801-8000-000000000101"),
                "/data" => Some("79324c69-6e75-4801-8000-000000000102"),
                "/media/internal" => Some("79324c69-6e75-4801-8000-000000000103"),
                _ => None,
            };
            if expected_uuid.is_some_and(|e| uuid.as_str() != Some(e)) {
                failed(&mut item, "filesystem_identity_invalid");
            }
            if sd {
                let identity = media_identity(&device, &mount.filesystem, uuid.as_str());
                let identity_value = identity.clone().map_or(Value::Null, Value::Object);
                set(&mut item, "identity", identity_value.clone());
                let claim = object(root, MEDIA_MOUNT);
                let matches = [
                    ("boot_id", text(&boot)),
                    ("mount_id", json!(mount.mount_id)),
                    ("device_id", json!(mount.device_id)),
                    ("uuid", uuid),
                ]
                .iter()
                .all(|(k, v)| get_eq(&claim, k, v));
                if !matches {
                    failed(&mut item, "mount_claim_changed_or_missing");
                }
                let claimed = get(&claim, "identity");
                if get(&item, "state") == "Ready"
                    && (identity.is_none()
                        || (!claimed.is_null() && !py_eq(claimed, &identity_value)))
                {
                    failed(&mut item, "media_identity_changed_or_missing");
                }
                if instance.is_none() || !py_eq(get(&claim, "source_instance"), &text(&instance)) {
                    failed(&mut item, "block_instance_changed");
                }
            }
        }
        let readonly = mount.read_only();
        let mut protected = false;
        if sd && readonly {
            // media.py mounts a healthy card read-only between writes: it is Ready;
            // so is one that plays while its check finishes in the background (F49).
            protected = get_in(
                &object(root, MEDIA_MOUNT),
                "read_only",
                &["protected", "checking"],
            );
        }
        if sd {
            set(&mut item, "writable", json!(!readonly));
        }
        match statvfs(&root.path(target)) {
            Ok(vfs) => {
                let frsize = vfs.f_frsize as u64;
                let total = vfs.f_blocks as u64 * frsize;
                let available = vfs.f_bavail as u64 * frsize;
                set(&mut item, "total_bytes", json!(total));
                set(&mut item, "available_bytes", json!(available));
                set(&mut item, "free_bytes", json!(vfs.f_bfree as u64 * frsize));
                set(&mut item, "available_inodes", json!(vfs.f_favail as u64));
                let error = get(&item, "state") == "Failed";
                let mut state =
                    space_state(Some(available), Some(total), readonly && !protected, error);
                if vfs.f_files > 0 && vfs.f_favail == 0 {
                    state = "CriticalSpace";
                }
                set(&mut item, "space_state", json!(state));
                if sd {
                    // A filesystem far smaller than its partition (a copied image, an old
                    // partition layout) hides most of the card; report it, never auto-fix it.
                    if let Some(sectors) = read(&device.join("size"))
                        .filter(|s| isdigit(s))
                        .and_then(|s| s.parse::<u64>().ok())
                    {
                        let bytes = sectors.saturating_mul(512);
                        set(&mut item, "partition_bytes", json!(bytes));
                        let undersized = (total as f64) < 0.9 * bytes as f64;
                        set(
                            &mut item,
                            "capacity_state",
                            json!(if undersized { "Undersized" } else { "Full" }),
                        );
                    }
                }
            }
            Err(_) => {
                set(&mut item, "space_state", json!("Failed"));
                failed(&mut item, "statvfs_failed");
            }
        }
        if get(&item, "state") == "Ready" && readonly && !protected {
            set(&mut item, "state", json!("Degraded"));
            set(&mut item, "reason", json!("read_only"));
            if sd {
                // Why the platform mounted the card read-only (media.py): its
                // check found errors or did not finish; Repair can make it writable.
                let claim = object(root, MEDIA_MOUNT);
                let cause = get(&claim, "read_only");
                let reason = if get_in(
                    &claim,
                    "read_only",
                    &["filesystem_errors", "check_timed_out"],
                ) {
                    cause.clone()
                } else {
                    json!("mounted_read_only")
                };
                set(&mut item, "read_only_reason", reason);
            }
        }
        volumes.push(item);
    }
    volumes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_states_follow_the_budget() {
        let gib = 1u64 << 30;
        assert_eq!(
            space_state(Some(gib), Some(4 * gib), false, false),
            "Normal"
        );
        assert_eq!(
            space_state(Some(300 << 20), Some(4 * gib), false, false),
            "LowSpace"
        );
        assert_eq!(
            space_state(Some(100 << 20), Some(4 * gib), false, false),
            "CriticalSpace"
        );
        assert_eq!(space_state(None, Some(gib), false, false), "Unavailable");
        assert_eq!(
            space_state(Some(gib), Some(gib), true, false),
            "ReadOnlyRisk"
        );
        assert_eq!(space_state(Some(gib), Some(gib), true, true), "Failed");
    }
}
