//! Fields of runtime records read back by another component (`/run/y2/dhcp.json`, `ssh.json`, …), read
//! leniently: a record another build or a crash left behind may hold any JSON there, and such a field
//! reads as absent instead of failing the whole record.
// SPDX-License-Identifier: GPL-2.0-only
use crate::json::Object;
use serde::{de::DeserializeOwned, Deserialize, Deserializer};
use serde_json::Value;

/// A text field: a JSON string, anything else (null, a number, missing) none.
pub fn text<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Ok(match Value::deserialize(deserializer)? {
        Value::String(text) => Some(text),
        _ => None,
    })
}

/// A list of texts: a JSON list of strings only, anything else none.
pub fn texts<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Vec<String>>, D::Error> {
    Ok(match Value::deserialize(deserializer)? {
        Value::Array(items) => items
            .into_iter()
            .map(|item| match item {
                Value::String(text) => Some(text),
                _ => None,
            })
            .collect(),
        _ => None,
    })
}

/// A count: a JSON integer from 0 up, anything else (a bool, a float) none.
pub fn count<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u64>, D::Error> {
    Ok(Value::deserialize(deserializer)?.as_u64())
}

/// A list of text pairs (`[["default", "192.0.2.1"]]`), anything else none.
pub fn pairs<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<(String, String)>>, D::Error> {
    Ok(match Value::deserialize(deserializer)? {
        Value::Array(items) => items
            .into_iter()
            .map(|item| match item {
                Value::Array(pair) => match <[Value; 2]>::try_from(pair) {
                    Ok([Value::String(a), Value::String(b)]) => Some((a, b)),
                    _ => None,
                },
                _ => None,
            })
            .collect(),
        _ => None,
    })
}

/// A record's fields from its parsed object (every field lenient and defaulted, so this never fails
/// for an object; an empty object gives the defaults).
pub fn read<T: DeserializeOwned + Default>(object: Object) -> T {
    serde_json::from_value(Value::Object(object)).unwrap_or_default()
}
