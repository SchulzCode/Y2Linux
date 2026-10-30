//! What the storage commands report of the volumes (`media status`, `space`): each mounted volume
//! with its controller, identity and space, the SD card's identity hash, the card a mount refused,
//! and this boot's Y2Media setup state. `status` (group observation) reports the volumes too, with
//! the controllers and block devices.
// SPDX-License-Identifier: GPL-2.0-only
use super::sys::{device_instance, get, isdigit, read, resolve, text, Object};
use serde_json::{json, Value};
use std::path::Path;
use y2p_core::{json::Separators, storage_media::sha256_hex, Root};

pub const MEDIA_MOUNT: &str = "/run/y2/media-mount.json";
pub const MEDIA_STATE: &str = "/run/y2/media.json";
pub const SETUP_STATUS: &str = "/run/y2/internal-media-setup.json";

/// A card's identity, its `id` the hash of the rest: CID/partition geometry supplies identity when a
/// legal FAT UUID is absent; `None` when neither can.
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
    // Python's sorted, spaced json.dumps: cards admitted by earlier versions keep their id.
    let encoded = y2p_core::json::python_dumps_sorted(&identity, Separators::Spaced).ok()?;
    identity.insert("id".into(), json!(sha256_hex(encoded.as_bytes())));
    Some(identity)
}

/// The card still inserted that the last mount refused (its check failed),
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

/// This boot's Y2Media setup state (no file names), else `{"state": "Unknown"}`.
pub fn setup_status(root: &Root) -> Object {
    match root.json(SETUP_STATUS) {
        Some(Value::Object(value)) if root.is_this_boot(value.get("boot_id")) => {
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

#[cfg(test)]
#[path = "observe_tests.rs"]
mod observe_tests;
