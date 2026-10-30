//! What uploads need of the storage: the verified `/data` volume (also for maintenance, diagnostics
//! and update; the volumes themselves are y2p_core::volume), the transfer admission and whether
//! Y2Media is the music volume.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::Failure;
pub use y2p_core::volume::Volume;
use y2p_core::Root;

pub const INTERNAL: &str = "/media/internal";
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
    crate::commands::storage::internal::ready(root)
}
