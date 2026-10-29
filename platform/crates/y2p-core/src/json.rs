//! Python `json.dumps` output: what the CLI prints (`", "` and `": "` separators) and what
//! `atomic_json` writes (`","` and `":"`), both with `ensure_ascii` and Python's float `repr`.
//! Key order: a `serde_json::Map` keeps insertion (or file) order (serde_json's `preserve_order`), as a
//! Python dict; `dumps_sorted` sorts every object while printing (Python's `sort_keys=True`), `dumps`
//! prints the order the value has. A struct serialises its fields in declaration order.
// SPDX-License-Identifier: GPL-2.0-only
use serde::Serialize;
use serde_json::ser::{CharEscape, Formatter};
use std::io::{self, Write};

/// The CLI's `print(json.dumps(result))` separators, or the records' compact ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Separators {
    Spaced,
    Compact,
}

pub struct PythonFormatter {
    separators: Separators,
}

impl PythonFormatter {
    pub fn new(separators: Separators) -> Self {
        Self { separators }
    }

    fn comma(&self) -> &'static [u8] {
        match self.separators {
            Separators::Spaced => b", ",
            Separators::Compact => b",",
        }
    }
}

impl Formatter for PythonFormatter {
    fn begin_array_value<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        if first {
            Ok(())
        } else {
            writer.write_all(self.comma())
        }
    }

    fn begin_object_key<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        if first {
            Ok(())
        } else {
            writer.write_all(self.comma())
        }
    }

    fn begin_object_value<W: ?Sized + Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(match self.separators {
            Separators::Spaced => b": ",
            Separators::Compact => b":",
        })
    }

    fn write_f32<W: ?Sized + Write>(&mut self, writer: &mut W, value: f32) -> io::Result<()> {
        writer.write_all(float_repr(f64::from(value)).as_bytes())
    }

    fn write_f64<W: ?Sized + Write>(&mut self, writer: &mut W, value: f64) -> io::Result<()> {
        writer.write_all(float_repr(value).as_bytes())
    }

    /// `ensure_ascii=True`: everything outside printable ASCII (also DEL) as `\uXXXX`, characters above
    /// U+FFFF as a surrogate pair, lowercase hex as Python writes it.
    fn write_string_fragment<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        fragment: &str,
    ) -> io::Result<()> {
        let mut start = 0;
        for (index, c) in fragment.char_indices() {
            if (' '..='~').contains(&c) {
                continue;
            }
            writer.write_all(&fragment.as_bytes()[start..index])?;
            start = index + c.len_utf8();
            if let Some(unit) = crate::os_text::surrogate(c) {
                // An escaped byte of a non-UTF-8 argument: Python's lone surrogate.
                write!(writer, "\\u{unit:04x}")?;
                continue;
            }
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                write!(writer, "\\u{unit:04x}")?;
            }
        }
        writer.write_all(&fragment.as_bytes()[start..])
    }

    fn write_char_escape<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        escape: CharEscape,
    ) -> io::Result<()> {
        let short: &[u8] = match escape {
            CharEscape::Quote => b"\\\"",
            CharEscape::ReverseSolidus => b"\\\\",
            CharEscape::Solidus => b"/",
            CharEscape::Backspace => b"\\b",
            CharEscape::FormFeed => b"\\f",
            CharEscape::LineFeed => b"\\n",
            CharEscape::CarriageReturn => b"\\r",
            CharEscape::Tab => b"\\t",
            CharEscape::AsciiControl(byte) => return write!(writer, "\\u{byte:04x}"),
        };
        writer.write_all(short)
    }
}

/// Python `repr(float)` for a finite value: the shortest round-trip digits, fixed notation for
/// decimal exponents -4..=15 (always with a fraction), otherwise `d.ddde±XX`.
pub fn float_repr(value: f64) -> String {
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0"
        } else {
            "0.0"
        }
        .into();
    }
    if !value.is_finite() {
        // json.dumps(allow_nan=True) spells; the CLI's allow_nan=False refuses them before printing.
        return if value.is_nan() {
            "NaN"
        } else if value > 0.0 {
            "Infinity"
        } else {
            "-Infinity"
        }
        .into();
    }
    // Ryu, like CPython's dtoa (mode 0), gives the shortest digits that round-trip and, among those,
    // the closest to the exact value; Rust's `{:e}` does not always pick the same one.
    let mut buffer = ryu::Buffer::new();
    let (digits, exponent) = decimal(buffer.format_finite(value.abs()));
    let sign = if value < 0.0 { "-" } else { "" };
    let text = if (-4..16).contains(&exponent) {
        if exponent >= 0 {
            let point = exponent as usize + 1;
            let mut whole = digits.clone();
            while whole.len() < point {
                whole.push('0');
            }
            let (integer, fraction) = whole.split_at(point);
            format!(
                "{integer}.{}",
                if fraction.is_empty() { "0" } else { fraction }
            )
        } else {
            format!("0.{}{digits}", "0".repeat((-exponent - 1) as usize))
        }
    } else {
        let (first, rest) = digits.split_at(1);
        let fraction = if rest.is_empty() {
            String::new()
        } else {
            format!(".{rest}")
        };
        format!(
            "{first}{fraction}e{}{:02}",
            if exponent < 0 { '-' } else { '+' },
            exponent.abs()
        )
    };
    format!("{sign}{text}")
}

/// The significant digits and the decimal exponent of the first one, from ryu's `123.45`, `0.0001` or
/// `1.2e-7` forms.
fn decimal(text: &str) -> (String, i32) {
    let (mantissa, exponent) = text.split_once('e').unwrap_or((text, "0"));
    let exponent: i32 = exponent.parse().expect("ryu exponent");
    let (integer, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let all = format!("{integer}{fraction}");
    let leading = all.len() - all.trim_start_matches('0').len();
    let digits = all[leading..].trim_end_matches('0');
    (
        digits.to_owned(),
        integer.len() as i32 - 1 - leading as i32 + exponent,
    )
}

/// A dict in insertion order, for the commands that print `json.dumps(result)` without `sort_keys`
/// (a `serde_json::Map` keeps its keys sorted). Setting an existing key keeps its position, as a dict.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ordered(pub Vec<(String, serde_json::Value)>);

impl Ordered {
    pub fn new() -> Self {
        Self::default()
    }

    /// `dict[key] = value`. The value's own objects are sorted.
    pub fn set(&mut self, key: &str, value: impl Into<serde_json::Value>) -> &mut Self {
        let value = sorted(value.into());
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = value,
            None => self.0.push((key.to_owned(), value)),
        }
        self
    }

    pub fn with(mut self, key: &str, value: impl Into<serde_json::Value>) -> Self {
        self.set(key, value);
        self
    }

    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// The keys of a parsed record in their file order (sorted: records are written with sort_keys).
    pub fn from_map(map: &serde_json::Map<String, serde_json::Value>) -> Self {
        let mut keys: Vec<&String> = map.keys().collect();
        keys.sort();
        Self(
            keys.into_iter()
                .map(|k| (k.clone(), sorted(map[k].clone())))
                .collect(),
        )
    }
}

impl Serialize for Ordered {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

/// `json.dumps(value)` with the given separators.
pub fn dumps<T: Serialize + ?Sized>(
    value: &T,
    separators: Separators,
) -> serde_json::Result<String> {
    let mut out = Vec::new();
    let mut serializer =
        serde_json::Serializer::with_formatter(&mut out, PythonFormatter::new(separators));
    value.serialize(&mut serializer)?;
    Ok(String::from_utf8(out).expect("ensure_ascii output is ASCII"))
}

/// `json.dumps(value, sort_keys=True)`: through a `Value` whose objects are then sorted at every level
/// (by code point, the byte order of UTF-8).
pub fn dumps_sorted<T: Serialize + ?Sized>(
    value: &T,
    separators: Separators,
) -> serde_json::Result<String> {
    dumps(&sorted(serde_json::to_value(value)?), separators)
}

/// `value` with the keys of every object in it sorted, nested ones too.
pub fn sorted(mut value: serde_json::Value) -> serde_json::Value {
    value.sort_all_objects();
    value
}

/// A map's entries by key (`sorted(d.items())`).
pub fn sorted_entries(
    map: &serde_json::Map<String, serde_json::Value>,
) -> Vec<(&String, &serde_json::Value)> {
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn floats_print_as_python_repr() {
        // repr() in CPython 3.15 for each value.
        for (value, python) in [
            (1.0, "1.0"),
            (0.5, "0.5"),
            (-2.25, "-2.25"),
            (12.497034384, "12.497034384"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1.5e-7, "1.5e-07"),
            (123456789012345.6, "123456789012345.6"),
            (1e15, "1000000000000000.0"),
            (1e16, "1e+16"),
            (1.2345e20, "1.2345e+20"),
            (1e100, "1e+100"),
            (0.1 + 0.2, "0.30000000000000004"),
            (-0.0, "-0.0"),
            (2730.79, "2730.79"),
            (5e-324, "5e-324"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
        ] {
            assert_eq!(float_repr(value), python, "{value:e}");
        }
    }

    #[test]
    fn random_floats_print_as_python_repr() {
        // Bit patterns from a fixed xorshift sequence.
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let bits: Vec<u64> = (0..20_000)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state
            })
            .filter(|b| f64::from_bits(*b).is_finite())
            .collect();
        // repr() of each as CPython printed it (recorded from host Python 3.15).
        let ours: String = bits
            .iter()
            .map(|b| float_repr(f64::from_bits(*b)) + "\n")
            .collect();
        crate::recorded::check(
            env!("CARGO_MANIFEST_DIR"),
            "float-repr.txt",
            ours.as_bytes(),
        );
    }

    #[test]
    fn output_matches_json_dumps() {
        let value = json!({"schema": "org.y2linux.status/v1", "b": [1, 2.0, null, true], "a": "é\u{7f}\u{1f600}\"\n\t/",
                           "n": {"z": 0.00001, "y": -3}});
        // python3 -c 'import json; print(json.dumps(v, sort_keys=True))'
        assert_eq!(
            dumps_sorted(&value, Separators::Spaced).unwrap(),
            "{\"a\": \"\\u00e9\\u007f\\ud83d\\ude00\\\"\\n\\t/\", \"b\": [1, 2.0, null, true], \
                    \"n\": {\"y\": -3, \"z\": 1e-05}, \"schema\": \"org.y2linux.status/v1\"}"
        );
        // json.dump(v, sort_keys=True, separators=(',', ':'))
        assert_eq!(
            dumps_sorted(&value, Separators::Compact).unwrap(),
            "{\"a\":\"\\u00e9\\u007f\\ud83d\\ude00\\\"\\n\\t/\",\"b\":[1,2.0,null,true],\
                    \"n\":{\"y\":-3,\"z\":1e-05},\"schema\":\"org.y2linux.status/v1\"}"
        );
        // json.dumps(v): the insertion order of the literal.
        assert_eq!(
            dumps(&json!({"b": 1, "a": {"d": 2, "c": 3}}), Separators::Compact).unwrap(),
            "{\"b\":1,\"a\":{\"d\":2,\"c\":3}}"
        );
        assert_eq!(dumps(&json!({}), Separators::Spaced).unwrap(), "{}");
        assert_eq!(dumps(&json!([]), Separators::Spaced).unwrap(), "[]");
        assert_eq!(
            dumps(&json!("\u{1}\u{8}\u{c}\r"), Separators::Spaced).unwrap(),
            "\"\\u0001\\b\\f\\r\""
        );
    }

    #[test]
    fn ordered_objects_keep_insertion_order() {
        let mut value = Ordered::new().with("state", "Ready").with("boot_id", "b");
        value.set("state", "Failed").set("monotonic_s", 1.5);
        assert_eq!(
            dumps(&value, Separators::Spaced).unwrap(),
            "{\"state\": \"Failed\", \"boot_id\": \"b\", \"monotonic_s\": 1.5}"
        );
    }

    #[test]
    fn structs_keep_declaration_order_unless_sorted() {
        #[derive(Serialize)]
        struct Insertion {
            state: &'static str,
            boot_id: &'static str,
        }
        let value = Insertion {
            state: "Ready",
            boot_id: "b",
        };
        assert_eq!(
            dumps(&value, Separators::Spaced).unwrap(),
            "{\"state\": \"Ready\", \"boot_id\": \"b\"}"
        );
        assert_eq!(
            dumps_sorted(&value, Separators::Spaced).unwrap(),
            "{\"boot_id\": \"b\", \"state\": \"Ready\"}"
        );
    }
}
