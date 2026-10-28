//! `y2-platform sleep status` (`sleep.status`/`sleep.product`): the product sleep receipt as this
//! boot reads it; a reset is never promoted into a resume.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::{Failure, Outcome, Reply};
use serde_json::{json, Map, Value};
use y2p_core::{py::truthy, Root};

const RECEIPT: &str = "/data/system/platform/suspend-last.json";
const STATES: [&str; 8] = [
    "requested",
    "preparing",
    "refused",
    "sleeping",
    "restoring",
    "waking",
    "restored",
    "restore_failed",
];

pub fn product(
    value: &Map<String, Value>,
    boot_id: Option<&str>,
) -> Result<Map<String, Value>, Failure> {
    let mut result = match value.get("product") {
        Some(Value::Object(map)) => map.clone(),
        Some(other) if truthy(other) => return Err(Failure::Fallback), // dict() of a non-dict
        _ => Map::new(),
    };
    if matches!(
        result.get("state"),
        Some(Value::Array(_) | Value::Object(_))
    ) {
        return Err(Failure::Fallback); // unhashable: `in STATES` raises
    }
    let state = result
        .get("state")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if !state.as_deref().is_some_and(|s| STATES.contains(&s)) {
        result = serde_json::from_value(json!({"state": "idle", "reason": null, "request_boot_id": null,
                                               "kernel_completed": false, "wake_reason": "unknown"})).expect("literal");
    }
    result.insert("schema".into(), json!(y2p_contract::schema::SLEEP));
    result.insert("boot_id".into(), json!(boot_id));
    let request = result
        .get("request_boot_id")
        .cloned()
        .unwrap_or(Value::Null);
    let same = match boot_id {
        Some(boot) if truthy(&request) && !boot.is_empty() => json!(request == json!(boot)),
        _ => Value::Null,
    };
    result.insert("same_boot".into(), same.clone());
    let state = result
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if matches!(state.as_str(), "restored" | "refused") && same == json!(false) {
        for (k, v) in [
            ("state", json!("idle")),
            ("reason", Value::Null),
            ("request_boot_id", Value::Null),
            ("same_boot", Value::Null),
            ("kernel_completed", json!(false)),
            ("wake_reason", json!("unknown")),
        ] {
            result.insert(k.into(), v);
        }
    }
    if matches!(
        state.as_str(),
        "requested" | "preparing" | "sleeping" | "restoring" | "waking"
    ) && same == json!(false)
    {
        result.insert("state".into(), json!("restore_failed"));
        result.insert("reason".into(), json!("boot_changed_before_restoration"));
        result.insert("kernel_completed".into(), json!(false));
    }
    Ok(result)
}

pub fn status(root: &Root) -> Outcome {
    let value = root.json_object(RECEIPT);
    Reply::sorted(
        &product(
            &value,
            root.read("/proc/sys/kernel/random/boot_id").as_deref(),
        )?,
        0,
    )
}
