//! The operations the reports and maintenance code apply to parsed JSON records: a record of another
//! shape than the operation takes (a list where an object belongs, a text where a number does) is refused
//! with the caller's reason.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::Failure;
use serde_json::Value;

/// A value of another shape than the operation takes: the caller refuses with the name of the record it
/// read (`or_refuse`).
#[derive(Debug, PartialEq)]
pub struct Shape;

pub trait OrRefuse<T> {
    /// `Failure::value(reason)` for a value of the wrong shape.
    fn or_refuse(self, reason: &'static str) -> Result<T, Failure>;
}

impl<T> OrRefuse<T> for Result<T, Shape> {
    fn or_refuse(self, reason: &'static str) -> Result<T, Failure> {
        self.map_err(|Shape| Failure::value(reason))
    }
}

/// An object's entry that must be there.
pub fn index<'a>(value: &'a Value, key: &str) -> Result<&'a Value, Shape> {
    value.as_object().and_then(|map| map.get(key)).ok_or(Shape)
}

/// Whether a list holds the text `item` or an object has the key `item`.
pub fn contains(container: &Value, item: &str) -> Result<bool, Shape> {
    match container {
        Value::Array(items) => Ok(items.iter().any(|v| v.as_str() == Some(item))),
        Value::Object(map) => Ok(map.contains_key(item)),
        _ => Err(Shape),
    }
}

/// A list's items.
pub fn iterate(value: &Value) -> Result<Vec<Value>, Shape> {
    value.as_array().cloned().ok_or(Shape)
}

/// Two numbers to compare.
pub fn ordered(left: &Value, right: &Value) -> Result<(f64, f64), Shape> {
    left.as_f64().zip(right.as_f64()).ok_or(Shape)
}

/// The JSON document `raw`, or why it is none.
pub fn loads(raw: &[u8]) -> Result<Value, Failure> {
    serde_json::from_slice(raw).map_err(|error| Failure::value(format!("invalid JSON: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn operations_take_the_shapes_records_are_written_in() {
        assert_eq!(index(&json!({}), "path"), Err(Shape));
        assert_eq!(index(&json!("ab"), "path"), Err(Shape));
        assert_eq!(contains(&json!(["a", "b"]), "a"), Ok(true));
        assert_eq!(contains(&json!({"schema": 1}), "schema"), Ok(true));
        assert_eq!(contains(&json!("a,b"), "a"), Err(Shape));
        assert_eq!(ordered(&json!("x"), &json!(5)), Err(Shape));
        assert_eq!(ordered(&json!(true), &json!(0.5)), Err(Shape));
        assert_eq!(ordered(&json!(2), &json!(0.5)), Ok((2.0, 0.5)));
        assert!(loads(b"{").is_err());
    }
}
