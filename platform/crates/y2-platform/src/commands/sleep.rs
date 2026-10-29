//! `y2-platform sleep status` (`sleep.status`/`sleep.product`): the product sleep receipt as this
//! boot reads it; a reset is never promoted into a resume.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::{Failure, Outcome, Reply};
use serde_json::{json, Map, Value};
use y2p_core::{
    py::{truthy, PyValue},
    Root,
};

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

/// `dict(x)` of a truthy JSON value that is no dict: a list of key/value pairs (each a 2-item list,
/// a 2-character str or a 2-key dict) builds one; anything else is Python's `TypeError` or
/// `ValueError`. The bool is true when a key is no str (a number, bool or null): such a dict exists
/// only in memory, because every sorted `json.dumps` of it raises.
fn dict(value: &Value) -> Result<(Map<String, Value>, bool), Failure> {
    let items = match value {
        Value::Array(items) => items,
        // A non-empty str: its first item is a 1-character str.
        Value::String(_) => {
            return Err(Failure::Value(
                "dictionary update sequence element #0 has length 1; 2 is required".into(),
            ))
        }
        _ => {
            return Err(Failure::type_error(format!(
                "'{}' object is not iterable",
                value.type_name()
            )))
        }
    };
    let mut result = Map::new();
    let mut odd = false;
    for (index, item) in items.iter().enumerate() {
        let pair = y2p_core::py::iterate(item).ok_or_else(|| {
            Failure::type_error(format!(
                "cannot convert dictionary update sequence element #{index} to a sequence"
            ))
        })?;
        if pair.len() != 2 {
            return Err(Failure::Value(format!(
                "dictionary update sequence element #{index} has length {}; 2 is required",
                pair.len()
            )));
        }
        match &pair[0] {
            Value::String(key) => {
                result.insert(key.clone(), pair[1].clone());
            }
            key @ (Value::Array(_) | Value::Object(_)) => {
                return Err(Failure::type_error(format!(
                    "cannot use '{}' as a dict key (unhashable type: '{}')",
                    key.type_name(),
                    key.type_name()
                )))
            }
            _ => odd = true,
        }
    }
    Ok((result, odd))
}

/// `sleep.product` for a caller that keeps the dict in memory before it is written: also whether
/// the dict holds a key that is no str (`dict()` of a list of pairs), which a later sorted
/// `json.dumps` refuses (`odd_keys`).
pub fn product_keys(
    value: &Map<String, Value>,
    boot_id: Option<&str>,
) -> Result<(Map<String, Value>, bool), Failure> {
    // dict(value.get('product') or {})
    let (mut result, mut odd) = match value.get("product") {
        Some(Value::Object(map)) => (map.clone(), false),
        Some(other) if truthy(other) => dict(other)?,
        _ => (Map::new(), false),
    };
    if let Some(state @ (Value::Array(_) | Value::Object(_))) = result.get("state") {
        // `in STATES`: unhashable.
        return Err(Failure::type_error(format!(
            "cannot use '{}' as a set element (unhashable type: '{}')",
            state.type_name(),
            state.type_name()
        )));
    }
    let state = result
        .get("state")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if !state.as_deref().is_some_and(|s| STATES.contains(&s)) {
        result = idle();
        odd = false;
    }
    Ok((finish(result, boot_id), odd))
}

/// The product of a receipt that holds no known state.
fn idle() -> Map<String, Value> {
    Map::from_iter([
        ("state".to_owned(), json!("idle")),
        ("reason".to_owned(), Value::Null),
        ("request_boot_id".to_owned(), Value::Null),
        ("kernel_completed".to_owned(), json!(false)),
        ("wake_reason".to_owned(), json!("unknown")),
    ])
}

/// The TypeError of `json.dumps(..., sort_keys=True)` on a dict with str and other keys.
pub fn odd_keys() -> Failure {
    Failure::type_error("'<' not supported between instances of 'int' and 'str'")
}

/// `sleep.product`: `Err` for Python's `ValueError` (`Failure::Value`) or `TypeError`
/// (`Failure::Crash`); a dict whose keys are not all str is the `TypeError` of its sorted dump.
pub fn product(
    value: &Map<String, Value>,
    boot_id: Option<&str>,
) -> Result<Map<String, Value>, Failure> {
    match product_keys(value, boot_id)? {
        (_, true) => Err(odd_keys()),
        (result, false) => Ok(result),
    }
}

fn finish(mut result: Map<String, Value>, boot_id: Option<&str>) -> Map<String, Value> {
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
    result
}

pub fn status(root: &Root) -> Outcome {
    let value = root.json_object(RECEIPT);
    Reply::sorted(&product(&value, root.boot_id().as_deref())?, 0)
}
