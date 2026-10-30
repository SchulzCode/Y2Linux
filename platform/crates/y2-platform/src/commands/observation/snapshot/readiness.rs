//! The `readiness` section: what each subsystem's state means for the product.
// SPDX-License-Identifier: GPL-2.0-only
use super::Snapshot;
use crate::{regex, reply::Failure};
use serde::Serialize;
use serde_json::Value;
use y2p_core::volume::{SpaceState, VolumeState};

/// What a subsystem's state means for the product.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Readiness {
    Ready,
    Degraded,
    Starting,
    #[default]
    Unavailable,
    Failed,
}

/// One subsystem's readiness and why.
#[derive(Serialize, Default)]
pub struct Item {
    pub state: Readiness,
    pub reason: Option<String>,
}

/// The `readiness` section (in this key order); Bluetooth's state is its section's own.
#[derive(Serialize, Default)]
pub struct Section {
    pub storage: Item,
    pub wifi: Item,
    pub bluetooth: Item,
    pub audio: Item,
    pub update: Item,
}

impl Section {
    /// Every subsystem's readiness.
    pub fn items(&self) -> [&Item; 5] {
        [
            &self.storage,
            &self.wifi,
            &self.bluetooth,
            &self.audio,
            &self.update,
        ]
    }
}

/// The audio qualification profile's fields the readiness reads (each lenient).
#[derive(serde::Deserialize, Default)]
#[serde(default)]
struct QualifiedProfile {
    #[serde(deserialize_with = "y2p_core::lenient::text")]
    schema: Option<String>,
    #[serde(deserialize_with = "y2p_core::lenient::text")]
    card: Option<String>,
    #[serde(deserialize_with = "y2p_core::lenient::or_none")]
    channels: Option<i64>,
    #[serde(deserialize_with = "y2p_core::lenient::or_default")]
    qualified_formats: Vec<String>,
    #[serde(deserialize_with = "y2p_core::lenient::or_default")]
    qualified_rates: Vec<i64>,
}

pub(super) fn readiness(status: &Snapshot) -> Result<Section, Failure> {
    let item = |state: Readiness, reason: Option<&str>| Item {
        state,
        reason: reason.map(str::to_owned),
    };
    let first = status.storage.volumes.iter().take(2);
    let storage_state = if first.clone().all(|v| v.state == VolumeState::Ready) {
        if first.clone().all(|v| v.space_state == SpaceState::Normal) {
            Readiness::Ready
        } else {
            Readiness::Degraded
        }
    } else {
        Readiness::Failed
    };
    let storage_reason = first
        .clone()
        .find(|v| v.state != VolumeState::Ready)
        .and_then(|v| v.reason);
    let wifi = &status.wifi;
    let wifi_state = match Some(wifi.state.as_str()) {
        Some("Online") => Readiness::Ready,
        Some("Off") => Readiness::Unavailable,
        Some("Failed") => Readiness::Failed,
        Some("Authenticated") => Readiness::Degraded,
        _ => Readiness::Starting,
    };
    // The installed qualification profile (shown as installed; read here for what it qualifies).
    let profile: QualifiedProfile = status
        .audio
        .qualified_profile
        .clone()
        .and_then(|p| serde_json::from_value(p).ok())
        .unwrap_or_default();
    let valid = profile.schema.as_deref() == Some("org.y2linux.audio-qualification/v1")
        && profile.card.as_deref() == Some("Y2Audio")
        && profile.channels == Some(2)
        && profile.qualified_formats.iter().any(|f| f == "S16_LE")
        && profile.qualified_rates.contains(&44100);
    let cards = status.audio.cards.as_deref().unwrap_or_default();
    let audio = regex!(r"(?m)^\s*\d+\s+\[Y2Audio\s*\]").is_match(cards) && valid;
    let update = &status.system.update;
    let update_state = match update.get("state") {
        Some(Value::Array(_) | Value::Object(_)) => {
            return Err(Failure::value("invalid_update_journal"))
        }
        Some(Value::String(s)) => match s.as_str() {
            "Idle" | "Acknowledged" => Readiness::Ready,
            "RolledBack" | "RollbackPending" => Readiness::Degraded,
            "Queued" | "PendingHealth" => Readiness::Starting,
            "Failed" | "RescueRequired" => Readiness::Failed,
            _ => Readiness::Unavailable,
        },
        _ => Readiness::Unavailable,
    };
    let bluetooth = &status.bluetooth;
    let section = Section {
        storage: item(storage_state, storage_reason),
        wifi: item(wifi_state, wifi.reason.as_deref()),
        bluetooth: item(bluetooth.state, Some(bluetooth.reason)),
        audio: item(
            if audio {
                Readiness::Ready
            } else {
                Readiness::Unavailable
            },
            Some("profile_does_not_imply_current_physical_acceptance"),
        ),
        update: item(update_state, update.get("failure").and_then(Value::as_str)),
    };
    Ok(section)
}
