//! The `readiness` section: what each subsystem's state means for the product.
// SPDX-License-Identifier: GPL-2.0-only
use super::{section, Snapshot};
use crate::commands::observation::util::{self, get_eq};
use crate::{regex, reply::Failure};
use serde_json::{json, Value};

pub(super) fn readiness(status: &Snapshot) -> Result<Value, Failure> {
    let item = |state: &str, reason: Value| json!({"state": state, "reason": reason});
    let empty = Vec::new();
    let volumes = section(status, "storage")["volumes"]
        .as_array()
        .unwrap_or(&empty);
    let first: Vec<&Value> = volumes.iter().take(2).collect();
    let all_ready = first.iter().all(|v| v["state"] == "Ready");
    let storage_state = if all_ready {
        if first.iter().all(|v| v["space_state"] == "Normal") {
            "Ready"
        } else {
            "Degraded"
        }
    } else {
        "Failed"
    };
    let storage_reason = first
        .iter()
        .find(|v| v["state"] != "Ready")
        .map_or(Value::Null, |v| {
            v.get("reason").cloned().unwrap_or(Value::Null)
        });
    let wifi = section(status, "wifi");
    let wifi_state = match wifi["state"].as_str() {
        Some("Online") => "Ready",
        Some("Off") => "Unavailable",
        Some("Failed") => "Failed",
        Some("Authenticated") => "Degraded",
        _ => "Starting",
    };
    let profile = &section(status, "audio")["qualified_profile"];
    let valid = profile.as_object().is_some_and(|p| {
        p.get("schema").and_then(Value::as_str) == Some("org.y2linux.audio-qualification/v1")
            && p.get("card").and_then(Value::as_str) == Some("Y2Audio")
            && get_eq(p, "channels", &json!(2))
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
    let cards = section(status, "audio")["cards"]
        .as_str()
        .unwrap_or_default();
    let audio = regex!(r"(?m)^\s*\d+\s+\[Y2Audio\s*\]").is_match(cards) && valid;
    let update = &section(status, "system")["update"];
    let update_state = match update.get("state") {
        Some(Value::Array(_) | Value::Object(_)) => {
            return Err(Failure::value("invalid_update_journal"))
        }
        Some(Value::String(s)) => match s.as_str() {
            "Idle" | "Acknowledged" => "Ready",
            "RolledBack" | "RollbackPending" => "Degraded",
            "Queued" | "PendingHealth" => "Starting",
            "Failed" | "RescueRequired" => "Failed",
            _ => "Unavailable",
        },
        _ => "Unavailable",
    };
    let bluetooth = section(status, "bluetooth");
    Ok(json!({
        "storage": item(storage_state, storage_reason),
        "wifi": item(wifi_state, wifi["reason"].clone()),
        "bluetooth": {"state": bluetooth["state"], "reason": bluetooth.get("reason").cloned().unwrap_or(Value::Null)},
        "audio": item(if audio { "Ready" } else { "Unavailable" }, json!("profile_does_not_imply_current_physical_acceptance")),
        "update": item(update_state, update.get("failure").cloned().unwrap_or(Value::Null)),
    }))
}
