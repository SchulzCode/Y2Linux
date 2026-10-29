//! `sleep.status` as a journal row holds it: the suspend receipt's product, read without promoting a
//! reset into a resume.
// SPDX-License-Identifier: GPL-2.0-only
use super::Dropped;
use serde_json::Value;
use y2p_core::{
    json::{Object, ObjectExt},
    py::PyValue,
};

const SLEEP_SCHEMA: &str = "org.y2linux.sleep/v1";
const SLEEP_STATES: [&str; 8] = [
    "requested",
    "preparing",
    "refused",
    "sleeping",
    "restoring",
    "waking",
    "restored",
    "restore_failed",
];

/// `sleep.status(ctx)` on the receipt: durable evidence interpreted without promoting a reset into a
/// resume.
pub(super) fn sleep_status(receipt: &Object, boot_id: Option<&str>) -> Result<Object, Dropped> {
    let mut result = match receipt.value("product") {
        Value::Object(o) => o,
        // A product that is set but no dict (only a damaged receipt has one): the row is dropped
        // and counted, the daemon goes on.
        other if other.truthy() => return Err(Dropped),
        _ => Object::new(),
    };
    let state = result.value("state");
    if !state.hashable() {
        return Err(Dropped); // `in STATES` (a set): TypeError
    }
    if !SLEEP_STATES.iter().any(|s| state.is_str(s)) {
        result = Object::new()
            .with("state", "idle")
            .with("reason", Value::Null)
            .with("request_boot_id", Value::Null)
            .with("kernel_completed", false)
            .with("wake_reason", "unknown");
    }
    result.set("schema", SLEEP_SCHEMA);
    result.set("boot_id", boot_id);
    let request = result.value("request_boot_id");
    let same = match boot_id {
        Some(boot) if request.truthy() && !boot.is_empty() => Value::from(request.is_str(boot)),
        _ => Value::Null,
    };
    result.set("same_boot", same.clone());
    let state = result.value("state");
    let state = state.as_str().unwrap_or_default();
    if matches!(state, "restored" | "refused") && same == Value::Bool(false) {
        result
            .set("state", "idle")
            .set("reason", Value::Null)
            .set("request_boot_id", Value::Null)
            .set("same_boot", Value::Null)
            .set("kernel_completed", false)
            .set("wake_reason", "unknown");
    }
    if matches!(
        state,
        "requested" | "preparing" | "sleeping" | "restoring" | "waking"
    ) && same == Value::Bool(false)
    {
        result
            .set("state", "restore_failed")
            .set("reason", "boot_changed_before_restoration")
            .set("kernel_completed", false);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use y2p_core::json::{dumps, Separators};

    #[test]
    fn sleep_status_follows_sleep_py() {
        let parse = |t: &str| match serde_json::from_slice::<Value>(t.as_bytes()).unwrap() {
            Value::Object(o) => o,
            _ => unreachable!(),
        };
        let text = |o: &Object| dumps(o, Separators::Spaced).unwrap();
        assert_eq!(
            text(&sleep_status(&parse("{}"), Some("b")).unwrap()),
            "{\"state\": \"idle\", \"reason\": null, \"request_boot_id\": null, \"kernel_completed\": false, \
             \"wake_reason\": \"unknown\", \"schema\": \"org.y2linux.sleep/v1\", \"boot_id\": \"b\", \"same_boot\": null}"
        );
        let other = parse(r#"{"product": {"state": "sleeping", "request_boot_id": "a"}}"#);
        let status = sleep_status(&other, Some("b")).unwrap();
        assert!(status.value("state").is_str("restore_failed"));
        assert!(sleep_status(&parse(r#"{"product": {"state": []}}"#), Some("b")).is_err());
        assert!(sleep_status(&parse(r#"{"product": 5}"#), Some("b")).is_err());
        assert!(sleep_status(&parse(r#"{"product": [1]}"#), Some("b")).is_err());
        // A list of pairs: dropped as well (the daemon goes on).
        assert!(sleep_status(&parse(r#"{"product": [["state", "idle"]]}"#), Some("b")).is_err());
    }
}
