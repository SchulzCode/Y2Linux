//! What `transfer` needs of `observe.storage` and `internal_media`: the verified `/data` volume
//! (`transfer.data_volume`, also for maintenance, diagnostics and update; the volumes themselves are
//! y2p_core::volume), the transfer admission and `internal_media.ready`.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::Failure;
use serde_json::{json, Value};
use y2p_core::mounts::mounts_at;
pub use y2p_core::volume::Volume;
use y2p_core::{py::eq_int, Root};

pub const INTERNAL: &str = "/media/internal";
const MARKER: &str = "/run/y2/internal-media.json";

/// `transfer.data_volume`: the verified Y2DATA volume (Ready, with a generation this boot).
pub fn data_volume(root: &Root) -> Result<Volume, Failure> {
    y2p_core::volume::verified(root, "/data")
        .ok_or_else(|| Failure::value("verified_data_unavailable"))
}

/// `space.admission(volume, requested_bytes, 'transfer')['allowed']`.
pub fn admitted(volume: &Volume, requested: u64) -> bool {
    if matches!(
        volume.space_state,
        "ReadOnlyRisk" | "Failed" | "Unavailable"
    ) {
        return false;
    }
    let reserve = 96 * 1024 * 1024;
    volume
        .available_bytes
        .is_some_and(|free| free as i128 - requested as i128 >= reserve)
}

/// `internal_media.ready`: true only while the recorded mount is still the live, exact Y2Media mount.
pub fn internal_ready(root: &Root) -> bool {
    let claim = root.json_object(MARKER);
    let current = mounts_at(root, INTERNAL);
    let boot = root.boot_id();
    claim.get("state").and_then(Value::as_str) == Some("Ready")
        && current.len() == 1
        && claim.get("boot_id").cloned().unwrap_or(Value::Null) == json!(boot)
        && eq_int(claim.get("mount_id"), current[0].mount_id)
        && claim.get("device_id").and_then(Value::as_str) == Some(current[0].device_id.as_str())
}
