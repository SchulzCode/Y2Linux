//! The `readiness` section: what each subsystem's state means for the product.
// SPDX-License-Identifier: GPL-2.0-only
use super::Snapshot;
use crate::commands::observation::util;
use crate::{regex, reply::Failure};
use serde::Serialize;
use serde_json::{json, Value};

/// What a subsystem's state means for the product.
#[derive(Serialize, Clone, Copy)]
enum Readiness {
    Ready,
    Degraded,
    Starting,
    Unavailable,
    Failed,
}

/// One subsystem's readiness and why.
#[derive(Serialize)]
struct Item<S> {
    state: S,
    reason: Value,
}

/// The `readiness` section (in this key order); Bluetooth's state is its section's own.
#[derive(Serialize)]
struct Section {
    storage: Item<Readiness>,
    wifi: Item<Readiness>,
    bluetooth: Item<Value>,
    audio: Item<Readiness>,
    update: Item<Readiness>,
}

pub(super) fn readiness(status: &Snapshot) -> Result<Value, Failure> {
    let item = |state: Readiness, reason: Value| Item { state, reason };
    let empty = Vec::new();
    let volumes = status.storage["volumes"].as_array().unwrap_or(&empty);
    let first: Vec<&Value> = volumes.iter().take(2).collect();
    let all_ready = first.iter().all(|v| v["state"] == "Ready");
    let storage_state = if all_ready {
        if first.iter().all(|v| v["space_state"] == "Normal") {
            Readiness::Ready
        } else {
            Readiness::Degraded
        }
    } else {
        Readiness::Failed
    };
    let storage_reason = first
        .iter()
        .find(|v| v["state"] != "Ready")
        .map_or(Value::Null, |v| {
            v.get("reason").cloned().unwrap_or(Value::Null)
        });
    let wifi = &status.wifi;
    let wifi_state = match wifi["state"].as_str() {
        Some("Online") => Readiness::Ready,
        Some("Off") => Readiness::Unavailable,
        Some("Failed") => Readiness::Failed,
        Some("Authenticated") => Readiness::Degraded,
        _ => Readiness::Starting,
    };
    let profile = status.audio.qualified_profile.as_ref();
    let valid = profile.and_then(Value::as_object).is_some_and(|p| {
        p.get("schema").and_then(Value::as_str) == Some("org.y2linux.audio-qualification/v1")
            && p.get("card").and_then(Value::as_str) == Some("Y2Audio")
            && p.get("channels").and_then(Value::as_i64) == Some(2)
            && p.get("qualified_formats")
                .and_then(Value::as_array)
                .is_some_and(|f| util::contains(f, &json!("S16_LE")))
            && p.get("qualified_rates")
                .and_then(Value::as_array)
                .is_some_and(|r| {
                    r.iter()
                        .any(|v| y2p_core::py::exact_int(Some(v)) == Some(44100))
                })
    });
    let cards = status.audio.cards.as_deref().unwrap_or_default();
    let audio = regex!(r"(?m)^\s*\d+\s+\[Y2Audio\s*\]").is_match(cards) && valid;
    let update = &status.system["update"];
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
        wifi: item(wifi_state, wifi["reason"].clone()),
        bluetooth: Item {
            state: bluetooth["state"].clone(),
            reason: bluetooth.get("reason").cloned().unwrap_or(Value::Null),
        },
        audio: item(
            if audio {
                Readiness::Ready
            } else {
                Readiness::Unavailable
            },
            json!("profile_does_not_imply_current_physical_acceptance"),
        ),
        update: item(
            update_state,
            update.get("failure").cloned().unwrap_or(Value::Null),
        ),
    };
    serde_json::to_value(section).map_err(Failure::value)
}
