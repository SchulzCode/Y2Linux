//! What the storage commands report of the volumes (`media status`, `space`): each mounted volume
//! with its controller, identity and space, the SD card's identity hash, the card a mount refused,
//! and this boot's Y2Media setup state. `status` (group observation) reports the volumes too, with
//! the controllers and block devices.
// SPDX-License-Identifier: GPL-2.0-only
use super::sys::{isdigit, read, resolve, Object};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
use y2p_core::{json::Separators, storage_media::sha256_hex, Root};

pub const MEDIA_MOUNT: &str = "/run/y2/media-mount.json";
pub const MEDIA_STATE: &str = "/run/y2/media.json";
pub const SETUP_STATUS: &str = "/run/y2/internal-media-setup.json";

/// A card's identity (`identity` of the SD records): what names it across insertions, and `id`, the
/// hash of the rest. Only exactly this shape reads as one.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub cid: Option<String>,
    pub geometry: Geometry,
    pub filesystem: String,
    pub uuid: Option<String>,
    pub id: String,
}

/// Where the filesystem sits on the card (sysfs texts).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Geometry {
    pub partition: Option<String>,
    pub start: Option<String>,
    pub size: Option<String>,
}

/// A card's identity: CID/partition geometry supplies identity when a legal FAT UUID is absent;
/// `None` when neither can.
pub fn media_identity(entry: &Path, filesystem: &str, uuid: Option<&str>) -> Option<Identity> {
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
    let geometry = Geometry {
        partition,
        start,
        size,
    };
    let hashed = json!({"cid": cid, "geometry": geometry, "filesystem": filesystem, "uuid": uuid});
    // Python's sorted, spaced json.dumps: cards admitted by earlier versions keep their id.
    let encoded = y2p_core::json::python_dumps_sorted(&hashed, Separators::Spaced).ok()?;
    Some(Identity {
        cid,
        geometry,
        filesystem: filesystem.to_owned(),
        uuid: uuid.map(str::to_owned),
        id: sha256_hex(encoded.as_bytes()),
    })
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
