//! Python `str` and `re` behaviour the suspend group reads sysfs and procfs text with: `splitlines`,
//! `split()`, `isdecimal`, `int(text, 16)` and `re.findall(r'(\w+)=(\S+)', text)`. The `regex` crate
//! is re-exported so the command crate needs no dependency of its own.
// SPDX-License-Identifier: GPL-2.0-only
pub use regex::Regex;
use serde_json::{Map, Value};
use std::sync::OnceLock;

/// Python's `str.isspace` (Unicode whitespace plus the separators U+001C..U+001F).
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// `str.split()`.
pub fn split(text: &str) -> Vec<&str> {
    text.split(is_space).filter(|s| !s.is_empty()).collect()
}

/// `str.splitlines()`: `\n`, `\r`, `\r\n`, `\v`, `\f`, U+001C..U+001E, U+0085, U+2028, U+2029.
pub fn splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        let breaks = matches!(
            c,
            '\n' | '\r'
                | '\u{b}'
                | '\u{c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if !breaks {
            continue;
        }
        lines.push(&text[start..index]);
        let mut end = index + c.len_utf8();
        if c == '\r' {
            if let Some((next, '\n')) = chars.peek().copied() {
                chars.next();
                end = next + 1;
            }
        }
        start = end;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// `str.isdecimal()` for the ASCII text sysfs gives (`''` is not decimal).
pub fn is_decimal(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_digit())
}

/// `int(text, 16)`: surrounding whitespace, a sign, an optional `0x`, `_` between digits.
pub fn int16(text: &str) -> Option<i64> {
    let text = text.trim_matches(is_space);
    let (sign, rest) = match text.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, text.strip_prefix('+').unwrap_or(text)),
    };
    let rest = match rest.get(..2) {
        Some("0x" | "0X") => rest[2..].strip_prefix('_').unwrap_or(&rest[2..]),
        _ => rest,
    };
    if rest.is_empty() || rest.starts_with('_') || rest.ends_with('_') || rest.contains("__") {
        return None;
    }
    let digits: String = rest.chars().filter(|c| *c != '_').collect();
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    i64::from_str_radix(&digits, 16).ok().map(|v| sign * v)
}

fn pairs() -> &'static Regex {
    static PAIRS: OnceLock<Regex> = OnceLock::new();
    PAIRS.get_or_init(|| Regex::new(r"(\w+)=(\S+)").expect("pattern"))
}

/// `dict(re.findall(r'(\w+)=(\S+)', text or ''))`: a later key wins.
pub fn fields(text: Option<&str>) -> Map<String, Value> {
    let mut map = Map::new();
    for found in pairs().captures_iter(text.unwrap_or_default()) {
        map.insert(found[1].to_owned(), Value::String(found[2].to_owned()));
    }
    map
}

/// `fields(..).get(key)` as text.
pub fn field<'a>(map: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    map.get(key).and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_follows_python() {
        // python3: the same calls on the same text.
        assert_eq!(splitlines("a\r\nb\n"), ["a", "b"]);
        assert_eq!(
            splitlines("a\u{b}b\u{c}c\u{1c}d\u{1d}e\u{1e}f\u{85}g\u{2028}h\u{2029}i\rj"),
            ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]
        );
        assert!(splitlines("").is_empty());
        assert_eq!(splitlines("\n"), [""]);
        assert_eq!(splitlines("x\n\n"), ["x", ""]);
        assert_eq!(split(" x\u{a0}y\u{2003}z\u{1c}"), ["x", "y", "z"]);
        assert_eq!(int16(" 0x1F "), Some(31));
        assert_eq!(int16("1_f"), Some(31));
        assert_eq!(int16("-0x10"), Some(-16));
        assert_eq!(int16("0x"), None);
        assert_eq!(int16("g"), None);
        let found = fields(Some("a=1 b=2=3 \u{fc}=x _=y -z=k"));
        assert_eq!(
            found,
            serde_json::from_str::<Map<String, Value>>(
                r#"{"a": "1", "b": "2=3", "ü": "x", "_": "y", "z": "k"}"#
            )
            .unwrap()
        );
        assert!(is_decimal("012") && !is_decimal("") && !is_decimal("1a"));
    }
}
