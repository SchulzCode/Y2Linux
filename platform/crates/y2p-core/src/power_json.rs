//! JSON values as Python's `json` module holds them, for records whose key order is part of what is
//! printed or sent: an object keeps its keys in insertion (or file) order, a repeated key keeps its first
//! position with the last value (a `dict`), `==` is Python's (`1 == 1.0 == True`), and `str()`/`repr()`
//! spell a value as Python does. `serde_json::Value` sorts its keys, which `json.dumps` without
//! `sort_keys` (the shutdown intent, the power journal rows) does not.
// SPDX-License-Identifier: GPL-2.0-only
use crate::json::float_repr;
use serde::{
    de::{self, MapAccess, SeqAccess, Visitor},
    ser::{SerializeMap, SerializeSeq},
    Deserialize, Deserializer, Serialize, Serializer,
};
use serde_json::Number;
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Json>),
    Object(Object),
}

/// A `dict` with string keys in insertion order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Object(Vec<(String, Json)>);

impl Object {
    pub fn new() -> Self {
        Self::default()
    }

    /// `dict[key] = value`: an existing key keeps its position.
    pub fn insert(&mut self, key: &str, value: impl Into<Json>) -> &mut Self {
        let value = value.into();
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = value,
            None => self.0.push((key.to_owned(), value)),
        }
        self
    }

    pub fn with(mut self, key: &str, value: impl Into<Json>) -> Self {
        self.insert(key, value);
        self
    }

    /// `dict.get(key)`.
    pub fn get(&self, key: &str) -> Option<&Json> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Json> {
        self.0.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// `dict.get(key)` as a value (`None` for a missing key).
    pub fn value(&self, key: &str) -> Json {
        self.get(key).cloned().unwrap_or(Json::Null)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Json)> {
        self.0.iter().map(|(k, v)| (k, v))
    }
}

impl Json {
    /// Parse one JSON document (as `json.loads`, keeping key order).
    pub fn parse(text: &[u8]) -> serde_json::Result<Json> {
        serde_json::from_slice(text)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Json::Object(o) => Some(o),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    /// `type(value) is int`.
    pub fn exact_int(&self) -> Option<i64> {
        match self {
            Json::Number(n) if n.is_i64() || n.is_u64() => n.as_i64(),
            _ => None,
        }
    }

    /// `dict.get(key)` on a value that is a dict (`None` for anything else).
    pub fn get(&self, key: &str) -> Option<&Json> {
        self.as_object().and_then(|o| o.get(key))
    }

    /// Python truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            Json::Null => false,
            Json::Bool(b) => *b,
            Json::Number(n) => n.as_f64() != Some(0.0),
            Json::String(s) => !s.is_empty(),
            Json::Array(a) => !a.is_empty(),
            Json::Object(o) => !o.is_empty(),
        }
    }

    /// `type(value).__name__`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Json::Null => "NoneType",
            Json::Bool(_) => "bool",
            Json::Number(n) if n.is_f64() => "float",
            Json::Number(_) => "int",
            Json::String(_) => "str",
            Json::Array(_) => "list",
            Json::Object(_) => "dict",
        }
    }

    /// Whether the value can be a dict key or a set member (`TypeError: unhashable` otherwise).
    pub fn hashable(&self) -> bool {
        !matches!(self, Json::Array(_) | Json::Object(_))
    }

    /// Python `a == b`: numbers and bools by value (`True == 1 == 1.0`), containers element-wise,
    /// dicts by keys and values regardless of order.
    pub fn py_eq(&self, other: &Json) -> bool {
        match (self, other) {
            (Json::Null, Json::Null) => true,
            (Json::String(a), Json::String(b)) => a == b,
            (Json::Array(a), Json::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.py_eq(y))
            }
            (Json::Object(a), Json::Object(b)) => {
                a.len() == b.len() && a.iter().all(|(k, v)| b.get(k).is_some_and(|w| v.py_eq(w)))
            }
            (a, b) => match (a.numeric(), b.numeric()) {
                (Some(x), Some(y)) => x.same(&y),
                _ => false,
            },
        }
    }

    /// `value == text` for a str.
    pub fn is_str(&self, text: &str) -> bool {
        self.as_str() == Some(text)
    }

    fn numeric(&self) -> Option<Numeric> {
        match self {
            Json::Bool(b) => Some(Numeric::Int(i128::from(*b))),
            Json::Number(n) => Some(if let Some(i) = n.as_i64() {
                Numeric::Int(i128::from(i))
            } else if let Some(u) = n.as_u64() {
                Numeric::Int(i128::from(u))
            } else {
                Numeric::Float(n.as_f64().unwrap_or(f64::NAN))
            }),
            _ => None,
        }
    }

    /// Python `str(value)` (`'%s' % value`, `str(ValueError(value))`).
    pub fn py_str(&self) -> String {
        match self {
            Json::String(s) => s.clone(),
            other => other.py_repr(),
        }
    }

    /// Python `repr(value)`.
    pub fn py_repr(&self) -> String {
        match self {
            Json::Null => "None".into(),
            Json::Bool(true) => "True".into(),
            Json::Bool(false) => "False".into(),
            Json::Number(n) => match n.as_f64() {
                Some(f) if n.is_f64() => float_repr(f),
                _ => n.to_string(),
            },
            Json::String(s) => crate::py::repr(s),
            Json::Array(a) => format!(
                "[{}]",
                a.iter().map(Json::py_repr).collect::<Vec<_>>().join(", ")
            ),
            Json::Object(o) => format!(
                "{{{}}}",
                o.iter()
                    .map(|(k, v)| format!("{}: {}", crate::py::repr(k), v.py_repr()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

enum Numeric {
    Int(i128),
    Float(f64),
}

impl Numeric {
    fn same(&self, other: &Numeric) -> bool {
        match (self, other) {
            (Numeric::Int(a), Numeric::Int(b)) => a == b,
            (Numeric::Float(a), Numeric::Float(b)) => a == b,
            // Python compares an int with a float exactly, not through a conversion.
            (Numeric::Int(i), Numeric::Float(f)) | (Numeric::Float(f), Numeric::Int(i)) => {
                f.is_finite() && f.fract() == 0.0 && f.abs() < 1.8e38 && *f as i128 == *i
            }
        }
    }
}

impl From<&str> for Json {
    fn from(value: &str) -> Self {
        Json::String(value.to_owned())
    }
}

impl From<String> for Json {
    fn from(value: String) -> Self {
        Json::String(value)
    }
}

impl From<bool> for Json {
    fn from(value: bool) -> Self {
        Json::Bool(value)
    }
}

impl From<i64> for Json {
    fn from(value: i64) -> Self {
        Json::Number(value.into())
    }
}

impl From<u64> for Json {
    fn from(value: u64) -> Self {
        Json::Number(value.into())
    }
}

impl From<f64> for Json {
    fn from(value: f64) -> Self {
        Number::from_f64(value).map_or(Json::Null, Json::Number)
    }
}

impl From<Object> for Json {
    fn from(value: Object) -> Self {
        Json::Object(value)
    }
}

impl From<Vec<Json>> for Json {
    fn from(value: Vec<Json>) -> Self {
        Json::Array(value)
    }
}

impl<T: Into<Json>> From<Option<T>> for Json {
    fn from(value: Option<T>) -> Self {
        value.map_or(Json::Null, Into::into)
    }
}

/// A `serde_json::Value` (its objects sorted) as a `Json`.
impl From<serde_json::Value> for Json {
    fn from(value: serde_json::Value) -> Self {
        match crate::json::sorted(value) {
            serde_json::Value::Null => Json::Null,
            serde_json::Value::Bool(b) => Json::Bool(b),
            serde_json::Value::Number(n) => Json::Number(n),
            serde_json::Value::String(s) => Json::String(s),
            serde_json::Value::Array(a) => Json::Array(a.into_iter().map(Json::from).collect()),
            serde_json::Value::Object(o) => Json::Object(Object(
                o.into_iter().map(|(k, v)| (k, Json::from(v))).collect(),
            )),
        }
    }
}

impl Serialize for Json {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Json::Null => serializer.serialize_unit(),
            Json::Bool(b) => serializer.serialize_bool(*b),
            Json::Number(n) => n.serialize(serializer),
            Json::String(s) => serializer.serialize_str(s),
            Json::Array(a) => {
                let mut seq = serializer.serialize_seq(Some(a.len()))?;
                for item in a {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Json::Object(o) => o.serialize(serializer),
        }
    }
}

impl Serialize for Object {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = Json;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON value")
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Json, E> {
        Ok(Json::Bool(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Json, E> {
        Ok(Json::Number(v.into()))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Json, E> {
        Ok(Json::Number(v.into()))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Json, E> {
        Number::from_f64(v)
            .map(Json::Number)
            .ok_or_else(|| E::custom("non-finite number"))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Json, E> {
        Ok(Json::String(v.to_owned()))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<Json, E> {
        Ok(Json::String(v))
    }

    fn visit_unit<E: de::Error>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Json, E> {
        Ok(Json::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Json, D::Error> {
        Json::deserialize(d)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Json::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
        let mut object = Object::new();
        while let Some((key, value)) = map.next_entry::<String, Json>()? {
            object.insert(&key, value);
        }
        Ok(Json::Object(object))
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Json, D::Error> {
        d.deserialize_any(JsonVisitor)
    }
}

/// `ctx.json(name)`: the parsed document in file order, `None` when it cannot be read or parsed.
pub fn read(root: &crate::Root, name: &str) -> Option<Json> {
    Json::parse(root.read(name)?.as_bytes()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::{dumps, dumps_sorted, Separators};

    #[test]
    fn objects_keep_python_dict_order() {
        let value =
            Json::parse(br#"{"state": "a", "id": 1, "state": "b", "n": {"z": 1, "a": 2.5}}"#)
                .unwrap();
        assert_eq!(
            dumps(&value, Separators::Spaced).unwrap(),
            r#"{"state": "b", "id": 1, "n": {"z": 1, "a": 2.5}}"#
        );
        assert_eq!(
            dumps_sorted(&value, Separators::Compact).unwrap(),
            r#"{"id":1,"n":{"a":2.5,"z":1},"state":"b"}"#
        );
    }

    #[test]
    fn equality_and_spelling_follow_python() {
        let parse = |t: &str| Json::parse(t.as_bytes()).unwrap();
        assert!(parse("1").py_eq(&parse("1.0")) && parse("true").py_eq(&parse("1")));
        assert!(!parse("1.5").py_eq(&parse("1")) && !parse("\"1\"").py_eq(&parse("1")));
        assert!(parse(r#"{"a": 1, "b": [2]}"#).py_eq(&parse(r#"{"b": [2.0], "a": true}"#)));
        assert_eq!(
            parse(r#"[null, true, 1.0, "it's", {"k": "v"}]"#).py_repr(),
            r#"[None, True, 1.0, "it's", {'k': 'v'}]"#
        );
        assert_eq!(parse("\"x\"").py_str(), "x");
        assert_eq!(parse("[]").type_name(), "list");
        assert_eq!(parse("2.0").type_name(), "float");
        assert!(Numeric::Int(3).same(&Numeric::Float(3.0)));
    }
}
