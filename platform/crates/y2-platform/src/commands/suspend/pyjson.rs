//! `json.loads(bytes)` as CPython 3 reads Reborn's control-socket answer, including its exceptions:
//! the answer's text decides the sleep receipt's `reason` (`str(error)`) and the class name main.py
//! prints. UTF-8 (with `surrogatepass`, a UTF-8 BOM skipped) and CPython's `_json` scanner messages
//! and positions (`Expecting value: line 1 column 1 (char 0)` for an empty answer).
//!
//! Accepted differences (Reborn writes serde_json, so none is reachable from it): `NaN`, `Infinity`,
//! `-Infinity` and floats that overflow read as `null`; a lone surrogate in a string reads as U+FFFD;
//! integers beyond 64 bits read as floats; UTF-16/UTF-32 answers (BOM or zero bytes) are refused as
//! undecodable instead of being decoded.
// SPDX-License-Identifier: GPL-2.0-only
use serde_json::{Map, Number, Value};

/// A Python exception: its class name and `str()`.
#[derive(Debug, PartialEq, Eq)]
pub struct Error {
    pub class: &'static str,
    pub text: String,
}

pub fn loads(bytes: &[u8]) -> Result<Value, Error> {
    let starts = |prefix: &[u8]| bytes.starts_with(prefix);
    if starts(&[0xff, 0xfe, 0, 0])
        || starts(&[0, 0, 0xfe, 0xff])
        || starts(&[0xfe, 0xff])
        || starts(&[0xff, 0xfe])
        || (bytes.len() >= 4 && (bytes[0] == 0 || bytes[1] == 0))
        || (bytes.len() == 2 && (bytes[0] == 0 || bytes[1] == 0))
    {
        return Err(Error {
            class: "UnicodeDecodeError",
            text: "utf-16/utf-32 answers are not read".into(),
        });
    }
    let body = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let text = decode(body)?;
    Scanner { s: &text }.document()
}

/// UTF-8 with `errors='surrogatepass'` into code points (surrogates kept).
fn decode(bytes: &[u8]) -> Result<Vec<u32>, Error> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    let cont = |b: u8| (0x80..=0xbf).contains(&b);
    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x80 {
            out.push(u32::from(b));
            i += 1;
            continue;
        }
        // (length, allowed range of the second byte)
        let (len, second) = match b {
            0xc2..=0xdf => (2, 0x80..=0xbf),
            0xe0 => (3, 0xa0..=0xbf),
            0xe1..=0xec | 0xee..=0xef => (3, 0x80..=0xbf),
            0xed => (3, 0x80..=0x9f),
            0xf0 => (4, 0x90..=0xbf),
            0xf1..=0xf3 => (4, 0x80..=0xbf),
            0xf4 => (4, 0x80..=0x8f),
            _ => return Err(utf8_error(bytes, i, i + 1, "invalid start byte")),
        };
        // surrogatepass: ED A0..BF 80..BF is a surrogate code point.
        if b == 0xed
            && bytes.len() >= i + 3
            && (0xa0..=0xbf).contains(&bytes[i + 1])
            && cont(bytes[i + 2])
        {
            out.push(
                (u32::from(b & 0x0f) << 12)
                    | (u32::from(bytes[i + 1] & 0x3f) << 6)
                    | u32::from(bytes[i + 2] & 0x3f),
            );
            i += 3;
            continue;
        }
        let mut k = i + 1;
        while k < i + len {
            let Some(&next) = bytes.get(k) else {
                return Err(utf8_error(bytes, i, bytes.len(), "unexpected end of data"));
            };
            let valid = if k == i + 1 {
                second.contains(&next)
            } else {
                cont(next)
            };
            if !valid {
                return Err(utf8_error(bytes, i, k, "invalid continuation byte"));
            }
            k += 1;
        }
        let mut code = u32::from(b) & (0x7f >> len);
        for &next in &bytes[i + 1..i + len] {
            code = (code << 6) | u32::from(next & 0x3f);
        }
        out.push(code);
        i += len;
    }
    Ok(out)
}

fn utf8_error(bytes: &[u8], start: usize, end: usize, reason: &str) -> Error {
    let text = if end - start == 1 {
        format!(
            "'utf-8' codec can't decode byte 0x{:02x} in position {start}: {reason}",
            bytes[start]
        )
    } else {
        format!(
            "'utf-8' codec can't decode bytes in position {start}-{}: {reason}",
            end - 1
        )
    };
    Error {
        class: "UnicodeDecodeError",
        text,
    }
}

struct Scanner<'a> {
    s: &'a [u32],
}

enum Fail {
    /// `StopIteration(idx)`: "Expecting value" at idx.
    Stop(usize),
    Message(&'static str, usize),
}

fn is_ws(c: u32) -> bool {
    matches!(c, 0x20 | 0x09 | 0x0a | 0x0d)
}

fn digit(c: Option<&u32>) -> bool {
    c.is_some_and(|c| (0x30..=0x39).contains(c))
}

impl Scanner<'_> {
    fn at(&self, i: usize) -> Option<u32> {
        self.s.get(i).copied()
    }

    fn skip(&self, mut i: usize) -> usize {
        while self.at(i).is_some_and(is_ws) {
            i += 1;
        }
        i
    }

    fn error(&self, message: &str, pos: usize) -> Error {
        let before = &self.s[..pos.min(self.s.len())];
        let line = before.iter().filter(|c| **c == 0x0a).count() + 1;
        let column = match before.iter().rposition(|c| *c == 0x0a) {
            Some(newline) => pos - newline,
            None => pos + 1,
        };
        Error {
            class: "JSONDecodeError",
            text: format!("{message}: line {line} column {column} (char {pos})"),
        }
    }

    fn document(&self) -> Result<Value, Error> {
        let start = self.skip(0);
        let fail = |f: Fail| match f {
            Fail::Stop(i) => self.error("Expecting value", i),
            Fail::Message(m, i) => self.error(m, i),
        };
        let (value, end) = self.value(start).map_err(fail)?;
        let end = self.skip(end);
        if end != self.s.len() {
            return Err(self.error("Extra data", end));
        }
        Ok(value)
    }

    fn literal(&self, i: usize, word: &str) -> bool {
        word.chars()
            .enumerate()
            .all(|(k, c)| self.at(i + k) == Some(c as u32))
    }

    fn value(&self, i: usize) -> Result<(Value, usize), Fail> {
        let Some(c) = self.at(i) else {
            return Err(Fail::Stop(i));
        };
        match char::from_u32(c).unwrap_or('\u{fffd}') {
            '"' => self.string(i + 1).map(|(s, end)| (Value::String(s), end)),
            '{' => self.object(i + 1),
            '[' => self.array(i + 1),
            'n' if self.literal(i, "null") => Ok((Value::Null, i + 4)),
            't' if self.literal(i, "true") => Ok((Value::Bool(true), i + 4)),
            'f' if self.literal(i, "false") => Ok((Value::Bool(false), i + 5)),
            'N' if self.literal(i, "NaN") => Ok((Value::Null, i + 3)),
            'I' if self.literal(i, "Infinity") => Ok((Value::Null, i + 8)),
            '-' if self.literal(i, "-Infinity") => Ok((Value::Null, i + 9)),
            _ => self.number(i),
        }
    }

    fn number(&self, start: usize) -> Result<(Value, usize), Fail> {
        let s = self.s;
        let last = s.len().saturating_sub(1);
        let mut i = start;
        if s.get(i) == Some(&0x2d) {
            i += 1;
            if i > last || s.is_empty() {
                return Err(Fail::Stop(start));
            }
        }
        match s.get(i) {
            Some(0x31..=0x39) => {
                i += 1;
                while digit(s.get(i)) {
                    i += 1;
                }
            }
            Some(0x30) => i += 1,
            _ => return Err(Fail::Stop(start)),
        }
        let mut float = false;
        if i < last && s[i] == 0x2e && digit(s.get(i + 1)) {
            float = true;
            i += 2;
            while digit(s.get(i)) {
                i += 1;
            }
        }
        if i < last && (s[i] == 0x65 || s[i] == 0x45) {
            let e = i;
            i += 1;
            if i < last && (s[i] == 0x2d || s[i] == 0x2b) {
                i += 1;
            }
            while digit(s.get(i)) {
                i += 1;
            }
            if digit(s.get(i - 1)) {
                float = true;
            } else {
                i = e;
            }
        }
        let text: String = s[start..i]
            .iter()
            .map(|c| char::from_u32(*c).unwrap_or('0'))
            .collect();
        let value = if !float {
            text.parse::<i64>()
                .map(Number::from)
                .ok()
                .or_else(|| text.parse::<u64>().ok().map(Number::from))
                .or_else(|| text.parse::<f64>().ok().and_then(Number::from_f64))
        } else {
            text.parse::<f64>().ok().and_then(Number::from_f64)
        };
        Ok((value.map_or(Value::Null, Value::Number), i))
    }

    /// `scanstring`: `end` is just after the opening quote.
    fn string(&self, mut end: usize) -> Result<(String, usize), Fail> {
        let s = self.s;
        let begin = end - 1;
        let mut out: Vec<u32> = Vec::new();
        loop {
            let mut next = end;
            let mut c = 0;
            while next < s.len() {
                c = s[next];
                if c == 0x22 || c == 0x5c {
                    break;
                }
                if c <= 0x1f {
                    return Err(Fail::Message("Invalid control character at", next));
                }
                next += 1;
            }
            if next >= s.len() || !(c == 0x22 || c == 0x5c) {
                return Err(Fail::Message("Unterminated string starting at", begin));
            }
            out.extend_from_slice(&s[end..next]);
            next += 1;
            if c == 0x22 {
                end = next;
                break;
            }
            if next == s.len() {
                return Err(Fail::Message("Unterminated string starting at", begin));
            }
            let escape = s[next];
            if escape != 0x75 {
                end = next + 1;
                let decoded = match char::from_u32(escape) {
                    Some('"') => 0x22,
                    Some('\\') => 0x5c,
                    Some('/') => 0x2f,
                    Some('b') => 0x08,
                    Some('f') => 0x0c,
                    Some('n') => 0x0a,
                    Some('r') => 0x0d,
                    Some('t') => 0x09,
                    _ => return Err(Fail::Message("Invalid \\escape", end - 2)),
                };
                out.push(decoded);
            } else {
                next += 1;
                end = next + 4;
                if end > s.len() {
                    return Err(Fail::Message("Invalid \\uXXXX escape", next - 1));
                }
                let hex = |from: usize, until: usize| -> Result<u32, Fail> {
                    let mut value = 0;
                    for &d in &s[from..until] {
                        let v = char::from_u32(d)
                            .and_then(|c| c.to_digit(16))
                            .ok_or(Fail::Message("Invalid \\uXXXX escape", until - 5))?;
                        value = (value << 4) | v;
                    }
                    Ok(value)
                };
                let mut code = hex(next, end)?;
                if (0xd800..=0xdbff).contains(&code)
                    && end + 6 < s.len()
                    && s[end] == 0x5c
                    && s[end + 1] == 0x75
                {
                    let low = hex(end + 2, end + 6)?;
                    if (0xdc00..=0xdfff).contains(&low) {
                        code = 0x10000 + ((code - 0xd800) << 10) + (low - 0xdc00);
                        end += 6;
                    }
                }
                out.push(code);
            }
        }
        let text = out
            .iter()
            .map(|c| char::from_u32(*c).unwrap_or('\u{fffd}'))
            .collect();
        Ok((text, end))
    }

    fn object(&self, start: usize) -> Result<(Value, usize), Fail> {
        let mut map = Map::new();
        let mut i = self.skip(start);
        if self.at(i) != Some(0x7d) {
            loop {
                if self.at(i) != Some(0x22) {
                    return Err(Fail::Message(
                        "Expecting property name enclosed in double quotes",
                        i,
                    ));
                }
                let (key, next) = self.string(i + 1)?;
                i = self.skip(next);
                if self.at(i) != Some(0x3a) {
                    return Err(Fail::Message("Expecting ':' delimiter", i));
                }
                i = self.skip(i + 1);
                let (value, next) = self.value(i)?;
                map.insert(key, value);
                i = self.skip(next);
                if self.at(i) == Some(0x7d) {
                    break;
                }
                if self.at(i) != Some(0x2c) {
                    return Err(Fail::Message("Expecting ',' delimiter", i));
                }
                let comma = i;
                i = self.skip(i + 1);
                if self.at(i) == Some(0x7d) {
                    return Err(Fail::Message(
                        "Illegal trailing comma before end of object",
                        comma,
                    ));
                }
            }
        }
        Ok((Value::Object(map), i + 1))
    }

    fn array(&self, start: usize) -> Result<(Value, usize), Fail> {
        let mut items = Vec::new();
        let mut i = self.skip(start);
        if self.at(i) != Some(0x5d) {
            while i < self.s.len() {
                let (value, next) = self.value(i)?;
                items.push(value);
                i = self.skip(next);
                if self.at(i) == Some(0x5d) {
                    break;
                }
                if self.at(i) != Some(0x2c) {
                    return Err(Fail::Message("Expecting ',' delimiter", i));
                }
                let comma = i;
                i = self.skip(i + 1);
                if self.at(i) == Some(0x5d) {
                    return Err(Fail::Message(
                        "Illegal trailing comma before end of array",
                        comma,
                    ));
                }
            }
            if self.at(i) != Some(0x5d) {
                return Err(Fail::Stop(i));
            }
        }
        Ok((Value::Array(items), i + 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn errors_read_as_cpython_words_them() {
        // python3 -c "json.loads(b'...')" for each input (3.14).
        for (input, class, text) in [
            (
                &b""[..],
                "JSONDecodeError",
                "Expecting value: line 1 column 1 (char 0)",
            ),
            (
                b" ",
                "JSONDecodeError",
                "Expecting value: line 1 column 2 (char 1)",
            ),
            (
                b"{",
                "JSONDecodeError",
                "Expecting property name enclosed in double quotes: line 1 column 2 (char 1)",
            ),
            (
                b"{\"a\"",
                "JSONDecodeError",
                "Expecting ':' delimiter: line 1 column 5 (char 4)",
            ),
            (
                b"{\"a\":",
                "JSONDecodeError",
                "Expecting value: line 1 column 6 (char 5)",
            ),
            (
                b"{\"a\":1",
                "JSONDecodeError",
                "Expecting ',' delimiter: line 1 column 7 (char 6)",
            ),
            (
                b"{\"a\":1,",
                "JSONDecodeError",
                "Expecting property name enclosed in double quotes: line 1 column 8 (char 7)",
            ),
            (
                b"{\"a\":1,}",
                "JSONDecodeError",
                "Illegal trailing comma before end of object: line 1 column 7 (char 6)",
            ),
            (
                b"[1,]",
                "JSONDecodeError",
                "Illegal trailing comma before end of array: line 1 column 3 (char 2)",
            ),
            (
                b"[1",
                "JSONDecodeError",
                "Expecting ',' delimiter: line 1 column 3 (char 2)",
            ),
            (
                b"[",
                "JSONDecodeError",
                "Expecting value: line 1 column 2 (char 1)",
            ),
            (
                b"[1,",
                "JSONDecodeError",
                "Expecting value: line 1 column 4 (char 3)",
            ),
            (
                b"\"abc",
                "JSONDecodeError",
                "Unterminated string starting at: line 1 column 1 (char 0)",
            ),
            (
                b"\"a\x01\"",
                "JSONDecodeError",
                "Invalid control character at: line 1 column 3 (char 2)",
            ),
            (
                b"\"\\x\"",
                "JSONDecodeError",
                "Invalid \\escape: line 1 column 2 (char 1)",
            ),
            (
                b"\"\\u12\"",
                "JSONDecodeError",
                "Invalid \\uXXXX escape: line 1 column 3 (char 2)",
            ),
            (
                b"\"\\u1234",
                "JSONDecodeError",
                "Unterminated string starting at: line 1 column 1 (char 0)",
            ),
            (
                b"\"\\ud800\\uzzzz\"",
                "JSONDecodeError",
                "Invalid \\uXXXX escape: line 1 column 9 (char 8)",
            ),
            (
                b"{}x",
                "JSONDecodeError",
                "Extra data: line 1 column 3 (char 2)",
            ),
            (
                b"nul",
                "JSONDecodeError",
                "Expecting value: line 1 column 1 (char 0)",
            ),
            (
                b"-",
                "JSONDecodeError",
                "Expecting value: line 1 column 1 (char 0)",
            ),
            (
                b"01",
                "JSONDecodeError",
                "Extra data: line 1 column 2 (char 1)",
            ),
            (
                b"1e+",
                "JSONDecodeError",
                "Extra data: line 1 column 2 (char 1)",
            ),
            (
                b"{\"a\" 1}",
                "JSONDecodeError",
                "Expecting ':' delimiter: line 1 column 6 (char 5)",
            ),
            (
                b"\xed\xa0\x80\"",
                "JSONDecodeError",
                "Expecting value: line 1 column 1 (char 0)",
            ),
            (
                b"\xff",
                "UnicodeDecodeError",
                "'utf-8' codec can't decode byte 0xff in position 0: invalid start byte",
            ),
            (
                b"a\xe2\x82",
                "UnicodeDecodeError",
                "'utf-8' codec can't decode bytes in position 1-2: unexpected end of data",
            ),
            (
                b"\xe2(\xa1",
                "UnicodeDecodeError",
                "'utf-8' codec can't decode byte 0xe2 in position 0: invalid continuation byte",
            ),
            (
                b"\xe2\x82(",
                "UnicodeDecodeError",
                "'utf-8' codec can't decode bytes in position 0-1: invalid continuation byte",
            ),
            (
                b"\xc0\x80",
                "UnicodeDecodeError",
                "'utf-8' codec can't decode byte 0xc0 in position 0: invalid start byte",
            ),
        ] {
            assert_eq!(
                loads(input),
                Err(Error {
                    class,
                    text: text.into()
                }),
                "{input:?}"
            );
        }
        assert_eq!(loads(b"\xef\xbb\xbf{}").unwrap(), json!({}));
        assert_eq!(
            loads(b" {\"id\": 6582, \"ok\": true, \"result\": {\"a\": [1, -0.5e-3, \"\\u00e9\\ud83d\\ude00\"]}}\r").unwrap(),
            json!({"id": 6582, "ok": true, "result": {"a": [1, -0.0005, "\u{e9}\u{1f600}"]}})
        );
        assert_eq!(loads(b"1E5").unwrap(), json!(100000.0));
        assert_eq!(loads(b"NaN").unwrap(), Value::Null);
    }
}
