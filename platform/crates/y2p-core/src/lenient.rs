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

/// A number as Python's `isinstance(value, (int, float))` takes it: a JSON number, or a bool as 0/1;
/// anything else none.
pub fn number<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    Ok(crate::py::number(Some(&Value::deserialize(deserializer)?)))
}

/// A flag as Python's truth test takes any value (`bool(value)`): a missing field is false.
pub fn truthy<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    Ok(crate::py::truthy(&Value::deserialize(deserializer)?))
}

/// A record's `boot_id` field, compared with this boot's id as Python's `==` did: a missing or null
/// field equals an unreadable boot id, a text equals the same text, anything else equals nothing.
#[derive(Debug, Default, Clone, PartialEq)]
pub enum BootId {
    #[default]
    Null,
    Text(String),
    Other,
}

impl BootId {
    /// Whether the field names `boot` (this boot's id, `None` when it is unreadable).
    pub fn is(&self, boot: Option<&str>) -> bool {
        match (self, boot) {
            (BootId::Null, None) => true,
            (BootId::Text(text), Some(boot)) => text == boot,
            _ => false,
        }
    }
}

impl<'de> Deserialize<'de> for BootId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Value::deserialize(deserializer)? {
            Value::Null => BootId::Null,
            Value::String(text) => BootId::Text(text),
            _ => BootId::Other,
        })
    }
}

#[cfg(test)]
#[path = "lenient_tests.rs"]
mod lenient_tests;
