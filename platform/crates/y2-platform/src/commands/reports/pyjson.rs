//! Python's `json.loads` and `json.dumps` exactly, for the values the report scrubs and the reset plan
//! hashes: integers of any size stay digits, floats parse correctly rounded and print as `repr`, `NaN`
//! and `Infinity` are read and written as Python does, objects keep their insertion order (a later
//! duplicate key replaces the value in place, as a dict does) and print sorted (`sort_keys=True`).
//! serde_json would read some long floats one ulp off (its default parser is not correctly rounded),
//! which changes a hash over a plan Python wrote.
// SPDX-License-Identifier: GPL-2.0-only
use serde::Serialize;
use y2p_core::json::float_repr;

#[derive(Clone, Debug, PartialEq)]
pub enum Py {
    None,
    Bool(bool),
    /// An int as its decimal digits (`-` for negative), never a float.
    Int(String),
    Float(f64),
    Str(String),
    List(Vec<Py>),
    Dict(Vec<(String, Py)>),
}

impl Py {
    pub fn int(value: i64) -> Py {
        Py::Int(value.to_string())
    }

    pub fn str(value: impl Into<String>) -> Py {
        Py::Str(value.into())
    }

    pub fn dict() -> Py {
        Py::Dict(Vec::new())
    }

    /// `dict.get(key)`.
    pub fn get(&self, key: &str) -> Option<&Py> {
        match self {
            Py::Dict(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Py> {
        match self {
            Py::Dict(items) => items.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// `dict[key] = value` (an existing key keeps its place).
    pub fn set(&mut self, key: &str, value: Py) {
        if let Py::Dict(items) = self {
            match items.iter_mut().find(|(k, _)| k == key) {
                Some(entry) => entry.1 = value,
                None => items.push((key.to_owned(), value)),
            }
        }
    }

    pub fn with(mut self, key: &str, value: Py) -> Py {
        self.set(key, value);
        self
    }

    pub fn remove(&mut self, key: &str) -> Option<Py> {
        if let Py::Dict(items) = self {
            if let Some(index) = items.iter().position(|(k, _)| k == key) {
                return Some(items.remove(index).1);
            }
        }
        None
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Py::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn is_dict(&self) -> bool {
        matches!(self, Py::Dict(_))
    }

    /// `type(value) is int` as an i64 (None beyond 64 bits).
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Py::Int(digits) => digits.parse().ok(),
            _ => None,
        }
    }

    /// `isinstance(value, (int, float))` (a bool is an int).
    pub fn number(&self) -> Option<f64> {
        match self {
            Py::Int(digits) => digits.parse().ok(),
            Py::Float(f) => Some(*f),
            Py::Bool(b) => Some(f64::from(u8::from(*b))),
            _ => None,
        }
    }

    /// Python truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            Py::None => false,
            Py::Bool(b) => *b,
            Py::Int(d) => d.trim_start_matches('-').trim_start_matches('0') != "",
            Py::Float(f) => *f != 0.0,
            Py::Str(s) => !s.is_empty(),
            Py::List(l) => !l.is_empty(),
            Py::Dict(d) => !d.is_empty(),
        }
    }

    /// `a == b` as Python compares parsed JSON (1 == 1.0 == True).
    pub fn py_eq(&self, other: &Py) -> bool {
        match (self, other) {
            (Py::Str(a), Py::Str(b)) => a == b,
            (Py::None, Py::None) => true,
            (Py::List(a), Py::List(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.py_eq(y))
            }
            (Py::Dict(a), Py::Dict(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .all(|(k, v)| other.get(k).is_some_and(|w| v.py_eq(w)))
                    && b.iter().all(|(k, _)| self.get(k).is_some())
            }
            (Py::Int(a), Py::Int(b)) => a == b,
            _ => match (self.number(), other.number()) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            },
        }
    }

    /// From a serde value (records the platform itself wrote).
    pub fn from_serde(value: &serde_json::Value) -> Py {
        match value {
            serde_json::Value::Null => Py::None,
            serde_json::Value::Bool(b) => Py::Bool(*b),
            serde_json::Value::Number(n) => match (n.as_i64(), n.as_u64()) {
                (Some(i), _) => Py::Int(i.to_string()),
                (_, Some(u)) => Py::Int(u.to_string()),
                _ => Py::Float(n.as_f64().unwrap_or(0.0)),
            },
            serde_json::Value::String(s) => Py::Str(s.clone()),
            serde_json::Value::Array(a) => Py::List(a.iter().map(Py::from_serde).collect()),
            serde_json::Value::Object(o) => Py::Dict(
                o.iter()
                    .map(|(k, v)| (k.clone(), Py::from_serde(v)))
                    .collect(),
            ),
        }
    }
}

impl From<&str> for Py {
    fn from(value: &str) -> Py {
        Py::Str(value.to_owned())
    }
}

impl From<String> for Py {
    fn from(value: String) -> Py {
        Py::Str(value)
    }
}

impl From<bool> for Py {
    fn from(value: bool) -> Py {
        Py::Bool(value)
    }
}

impl From<i64> for Py {
    fn from(value: i64) -> Py {
        Py::int(value)
    }
}

impl From<usize> for Py {
    fn from(value: usize) -> Py {
        Py::Int(value.to_string())
    }
}

impl From<u64> for Py {
    fn from(value: u64) -> Py {
        Py::Int(value.to_string())
    }
}

impl From<f64> for Py {
    fn from(value: f64) -> Py {
        Py::Float(value)
    }
}

impl<T: Into<Py>> From<Option<T>> for Py {
    fn from(value: Option<T>) -> Py {
        value.map_or(Py::None, Into::into)
    }
}

impl Serialize for Py {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{SerializeMap, SerializeSeq};
        match self {
            Py::None => serializer.serialize_unit(),
            Py::Bool(b) => serializer.serialize_bool(*b),
            Py::Int(digits) => match (digits.parse::<i64>(), digits.parse::<u64>()) {
                (Ok(i), _) => serializer.serialize_i64(i),
                (_, Ok(u)) => serializer.serialize_u64(u),
                _ => serializer.serialize_f64(digits.parse().unwrap_or(0.0)),
            },
            Py::Float(f) => serializer.serialize_f64(*f),
            Py::Str(s) => serializer.serialize_str(s),
            Py::List(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Py::Dict(items) => {
                let mut map = serializer.serialize_map(Some(items.len()))?;
                for (k, v) in items {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
        }
    }
}

// ---- loads ---------------------------------------------------------------------------------------

/// `json.loads(text)`; `None` where Python raises (a ValueError, or a lone surrogate this port cannot
/// hold, or nesting beyond Python's recursion limit).
pub fn loads(text: &str) -> Option<Py> {
    let mut parser = Parser {
        text: text.as_bytes(),
        at: 0,
        depth: 0,
    };
    parser.space();
    let value = parser.value()?;
    parser.space();
    (parser.at == parser.text.len()).then_some(value)
}

/// `json.loads(bytes)`: UTF-8 (Python also detects UTF-16/32 by the first bytes; not used here).
pub fn loads_bytes(data: &[u8]) -> Option<Py> {
    let data = data.strip_prefix(b"\xef\xbb\xbf").map_or(Some(data), |_| None)?;
    loads(std::str::from_utf8(data).ok()?)
}

struct Parser<'a> {
    text: &'a [u8],
    at: usize,
    depth: usize,
}

impl Parser<'_> {
    fn space(&mut self) {
        while matches!(self.text.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, literal: &[u8]) -> bool {
        if self.text[self.at..].starts_with(literal) {
            self.at += literal.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Option<Py> {
        match *self.text.get(self.at)? {
            b'{' => self.nested(|p| p.object()),
            b'[' => self.nested(|p| p.array()),
            b'"' => self.string().map(Py::Str),
            b'n' if self.eat(b"null") => Some(Py::None),
            b't' if self.eat(b"true") => Some(Py::Bool(true)),
            b'f' if self.eat(b"false") => Some(Py::Bool(false)),
            b'N' if self.eat(b"NaN") => Some(Py::Float(f64::NAN)),
            b'I' if self.eat(b"Infinity") => Some(Py::Float(f64::INFINITY)),
            b'-' if self.text[self.at..].starts_with(b"-Infinity") => {
                self.at += 9;
                Some(Py::Float(f64::NEG_INFINITY))
            }
            b'-' | b'0'..=b'9' => self.number(),
            _ => None,
        }
    }

    fn nested(&mut self, inner: impl FnOnce(&mut Self) -> Option<Py>) -> Option<Py> {
        // CPython's recursion limit (1000 frames) ends deeper documents with RecursionError.
        self.depth += 1;
        if self.depth > 900 {
            return None;
        }
        let value = inner(self);
        self.depth -= 1;
        value
    }

    fn object(&mut self) -> Option<Py> {
        self.at += 1;
        let mut items: Vec<(String, Py)> = Vec::new();
        self.space();
        if self.eat(b"}") {
            return Some(Py::Dict(items));
        }
        loop {
            self.space();
            if self.text.get(self.at) != Some(&b'"') {
                return None;
            }
            let key = self.string()?;
            self.space();
            if !self.eat(b":") {
                return None;
            }
            self.space();
            let value = self.value()?;
            match items.iter_mut().find(|(k, _)| *k == key) {
                Some(entry) => entry.1 = value,
                None => items.push((key, value)),
            }
            self.space();
            if self.eat(b"}") {
                return Some(Py::Dict(items));
            }
            if !self.eat(b",") {
                return None;
            }
        }
    }

    fn array(&mut self) -> Option<Py> {
        self.at += 1;
        let mut items = Vec::new();
        self.space();
        if self.eat(b"]") {
            return Some(Py::List(items));
        }
        loop {
            self.space();
            items.push(self.value()?);
            self.space();
            if self.eat(b"]") {
                return Some(Py::List(items));
            }
            if !self.eat(b",") {
                return None;
            }
        }
    }

    fn number(&mut self) -> Option<Py> {
        let start = self.at;
        let digits = |p: &mut Self| {
            let from = p.at;
            while p.text.get(p.at).is_some_and(u8::is_ascii_digit) {
                p.at += 1;
            }
            p.at - from
        };
        self.eat(b"-");
        match self.text.get(self.at) {
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => {
                digits(self);
            }
            _ => return None,
        }
        let mut float = false;
        let integer_end = self.at;
        if self.text.get(self.at) == Some(&b'.')
            && self.text.get(self.at + 1).is_some_and(u8::is_ascii_digit)
        {
            self.at += 1;
            digits(self);
            float = true;
        }
        if matches!(self.text.get(self.at), Some(b'e' | b'E')) {
            let mark = self.at;
            self.at += 1;
            if matches!(self.text.get(self.at), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if digits(self) == 0 {
                self.at = mark; // NUMBER_RE leaves a bare 'e' unmatched (then "Extra data")
            } else {
                float = true;
            }
        }
        let text = std::str::from_utf8(&self.text[start..self.at]).ok()?;
        if float {
            text.parse().ok().map(Py::Float)
        } else {
            let text = &text[..integer_end - start];
            // int() refuses more than 4300 digits (sys.int_info.str_digits_check_threshold).
            if text.trim_start_matches('-').len() > 4300 {
                return None;
            }
            Some(Py::Int(if text == "-0" { "0".into() } else { text.into() }))
        }
    }

    fn string(&mut self) -> Option<String> {
        self.at += 1;
        let mut out = String::new();
        loop {
            let start = self.at;
            while let Some(&b) = self.text.get(self.at) {
                if b == b'"' || b == b'\\' || b < 0x20 {
                    break;
                }
                self.at += 1;
            }
            out.push_str(std::str::from_utf8(&self.text[start..self.at]).ok()?);
            match *self.text.get(self.at)? {
                b'"' => {
                    self.at += 1;
                    return Some(out);
                }
                b'\\' => {
                    self.at += 1;
                    let c = *self.text.get(self.at)?;
                    self.at += 1;
                    out.push(match c {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let first = self.hex4()?;
                            if (0xd800..0xdc00).contains(&first)
                                && self.text[self.at..].starts_with(b"\\u")
                            {
                                let mark = self.at;
                                self.at += 2;
                                let second = self.hex4()?;
                                if (0xdc00..0xe000).contains(&second) {
                                    char::from_u32(
                                        0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00),
                                    )?
                                } else {
                                    self.at = mark;
                                    char::from_u32(first)? // a lone surrogate: not representable
                                }
                            } else {
                                char::from_u32(first)?
                            }
                        }
                        _ => return None,
                    });
                }
                _ => return None, // a control character (strict=True)
            }
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let digits = self.text.get(self.at..self.at + 4)?;
        if !digits.iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
        self.at += 4;
        u32::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()
    }
}

// ---- dumps ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// `json.dumps(v, sort_keys=True)`.
    Spaced,
    /// `separators=(',', ':')`.
    Compact,
    /// `indent=1`.
    Indent,
}

/// `json.dumps(value, sort_keys=True, ...)` (ensure_ascii, allow_nan).
pub fn dumps(value: &Py, style: Style) -> String {
    let mut out = String::new();
    write(&mut out, value, style, 1);
    out
}

/// `json.dumps(..., allow_nan=False)`: Python's ValueError text for a non-finite float.
pub fn dumps_strict(value: &Py, style: Style) -> Result<String, String> {
    fn finite(value: &Py) -> Result<(), String> {
        match value {
            Py::Float(f) if !f.is_finite() => Err(format!(
                "Out of range float values are not JSON compliant: {}",
                if f.is_nan() {
                    "nan"
                } else if *f > 0.0 {
                    "inf"
                } else {
                    "-inf"
                }
            )),
            Py::List(items) => items.iter().try_for_each(finite),
            Py::Dict(items) => items.iter().try_for_each(|(_, v)| finite(v)),
            _ => Ok(()),
        }
    }
    finite(value)?;
    Ok(dumps(value, style))
}

pub fn string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(c),
            _ => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
}

fn write(out: &mut String, value: &Py, style: Style, level: usize) {
    let (item, key) = match style {
        Style::Spaced => (", ", ": "),
        Style::Compact => (",", ":"),
        Style::Indent => (",", ": "),
    };
    let newline = |out: &mut String, level: usize| {
        if style == Style::Indent {
            out.push('\n');
            out.push_str(&" ".repeat(level));
        }
    };
    match value {
        Py::None => out.push_str("null"),
        Py::Bool(true) => out.push_str("true"),
        Py::Bool(false) => out.push_str("false"),
        Py::Int(digits) => out.push_str(digits),
        Py::Float(f) => out.push_str(&float_repr(*f)),
        Py::Str(s) => string(out, s),
        Py::List(items) if items.is_empty() => out.push_str("[]"),
        Py::Dict(items) if items.is_empty() => out.push_str("{}"),
        Py::List(items) => {
            out.push('[');
            for (n, v) in items.iter().enumerate() {
                if n > 0 {
                    out.push_str(item);
                }
                newline(out, level);
                write(out, v, style, level + 1);
            }
            newline(out, level - 1);
            out.push(']');
        }
        Py::Dict(items) => {
            let mut sorted: Vec<&(String, Py)> = items.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            out.push('{');
            for (n, (k, v)) in sorted.into_iter().enumerate() {
                if n > 0 {
                    out.push_str(item);
                }
                newline(out, level);
                string(out, k);
                out.push_str(key);
                write(out, v, style, level + 1);
            }
            newline(out, level - 1);
            out.push('}');
        }
    }
}

/// Python `str(value)` of a parsed JSON value (`'%s' % value`).
pub fn py_str(value: &Py) -> String {
    match value {
        Py::Str(s) => s.clone(),
        other => py_repr(other),
    }
}

/// Python `repr(value)`.
pub fn py_repr(value: &Py) -> String {
    match value {
        Py::None => "None".into(),
        Py::Bool(true) => "True".into(),
        Py::Bool(false) => "False".into(),
        Py::Int(d) => d.clone(),
        Py::Float(f) if f.is_nan() => "nan".into(),
        Py::Float(f) if f.is_infinite() => if *f > 0.0 { "inf" } else { "-inf" }.into(),
        Py::Float(f) => float_repr(*f),
        Py::Str(s) => y2p_core::py::repr(s),
        Py::List(items) => format!(
            "[{}]",
            items.iter().map(py_repr).collect::<Vec<_>>().join(", ")
        ),
        Py::Dict(items) => format!(
            "{{{}}}",
            items
                .iter()
                .map(|(k, v)| format!("{}: {}", y2p_core::py::repr(k), py_repr(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_as_python() {
        let text = r#"{"b": [1, 2.5, -0, 1e400, NaN, -Infinity, 123456789012345678901234567890],
                       "a": "é😀\"/", "b": {"z": 0.1, "y": null, "x": true}, "c": 1E5}"#;
        let value = loads(text).unwrap();
        // python3 -c 'import json; print(json.dumps(json.loads(t), sort_keys=True))'
        assert_eq!(
            dumps(&value, Style::Spaced),
            "{\"a\": \"\\u00e9\\ud83d\\ude00\\\"/\", \"b\": {\"x\": true, \"y\": null, \"z\": 0.1}, \
             \"c\": 100000.0}"
        );
        assert_eq!(
            dumps(&loads("[{\"a\": [1, {}], \"b\": []}]").unwrap(), Style::Indent),
            "[\n {\n  \"a\": [\n   1,\n   {}\n  ],\n  \"b\": []\n }\n]"
        );
        assert_eq!(
            dumps(&loads(r#"[1, -0, 1e400, NaN, 1.0]"#).unwrap(), Style::Compact),
            "[1,0,Infinity,NaN,1.0]"
        );
        for bad in ["", "{", "[1,]", "{\"a\" 1}", "01", "1 2", "\"\u{1}\"", "'a'", "[1e]"] {
            assert_eq!(loads(bad), None, "{bad:?}");
        }
        assert_eq!(py_repr(&loads(r#"{"a": [null, true, "it's"]}"#).unwrap()), "{'a': [None, True, \"it's\"]}");
    }
}
