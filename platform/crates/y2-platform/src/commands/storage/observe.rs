//! What the storage commands report of the volumes (`media status`, `space`): each mounted volume
//! with its controller, identity and space, the SD card's identity hash, the card a mount refused,
//! and this boot's Y2Media setup state. `status` (group observation) reports the volumes too, with
//! the controllers and block devices.
// SPDX-License-Identifier: GPL-2.0-only
use super::sys::{isdigit, read, resolve};
use serde_json::json;
use std::path::Path;
use y2p_core::{json::Separators, lenient, storage_media::sha256_hex, Root};

pub const MEDIA_MOUNT: &str = "/run/y2/media-mount.json";
pub const MEDIA_STATE: &str = "/run/y2/media.json";
pub const SETUP_STATUS: &str = "/run/y2/internal-media-setup.json";

pub use y2p_contract::storage::{Geometry, Identity};

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

pub use y2p_contract::storage::SetupStatus;

pub fn setup_status(root: &Root) -> SetupStatus {
    let unknown = || SetupStatus {
        state: Some("Unknown".into()),
        ..SetupStatus::default()
    };
    let Some(serde_json::Value::Object(note)) = root.json(SETUP_STATUS) else {
        return unknown();
    };
    let note: SetupStatus = lenient::read(note);
    if !note.boot_id.is(root.boot_id().as_deref()) {
        return unknown();
    }
    note
}

#[cfg(test)]
#[path = "observe_tests.rs"]
mod observe_tests;
