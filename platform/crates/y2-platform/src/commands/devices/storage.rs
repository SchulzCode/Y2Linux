//! What uploads need of the storage: the verified `/data` volume (also for maintenance, diagnostics
//! and update; the volumes themselves are y2p_core::volume), the transfer admission and whether
//! Y2Media is the music volume.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::Failure;
use serde_json::{json, Value};
use y2p_core::mounts::mounts_at;
pub use y2p_core::volume::Volume;
use y2p_core::{json::ObjectExt, py::eq_int, Root};

pub const INTERNAL: &str = "/media/internal";
/// The storage setup's claim on Y2Media (state, boot, mount and device it was mounted as).
const MARKER: &str = "/run/y2/internal-media.json";
/// What a transfer must leave free on its volume.
const RESERVE: i128 = 96 * 1024 * 1024;

/// The verified Y2DATA volume (Ready, with a generation this boot).
pub fn data_volume(root: &Root) -> Result<Volume, Failure> {
    y2p_core::volume::verified(root, "/data")
        .ok_or_else(|| Failure::value("verified_data_unavailable"))
}

/// Whether `requested` bytes fit the volume and still leave the reserve: never on a volume at risk of
/// going read-only, failed or unavailable.
pub fn admitted(volume: &Volume, requested: u64) -> bool {
    if matches!(
        volume.space_state,
        "ReadOnlyRisk" | "Failed" | "Unavailable"
    ) {
        return false;
    }
    volume
        .available_bytes
        .is_some_and(|free| i128::from(free) - i128::from(requested) >= RESERVE)
}

/// Whether Y2Media is ready: only while the recorded mount is still the live, exact mount (one mount
/// at /media/internal with the recorded mount and device id, recorded Ready this boot).
pub fn internal_ready(root: &Root) -> bool {
    let claim = root.json_object(MARKER);
    let current = mounts_at(root, INTERNAL);
    let boot = root.boot_id();
    let [mount] = &current[..] else {
        return false;
    };
    claim.get("state").and_then(Value::as_str) == Some("Ready")
        && claim.value("boot_id") == json!(boot)
        && eq_int(claim.get("mount_id"), mount.mount_id)
        && claim.get("device_id").and_then(Value::as_str) == Some(mount.device_id.as_str())
}
