//! Arguments and environment that are not UTF-8, as Python sees them: `os.fsdecode` (UTF-8 with
//! `surrogateescape`) turns each byte that is no part of a valid UTF-8 sequence into a lone surrogate
//! U+DC80..U+DCFF. A Rust `str` cannot hold a lone surrogate, so such a byte becomes the private-use
//! character U+10DC80..U+10DCFF instead (`ESCAPE_BASE` + the byte), and every place Python would show
//! or encode the surrogate does it for that character: `repr()` and `json.dumps` write `\udcXX`,
//! stderr (`backslashreplace`) writes `\udcXX`, `os.fsencode` gives the byte back.
//!
//! Accepted difference: a valid UTF-8 argument holding U+10DC80..U+10DCFF itself reads as the escaped
//! byte (Python would keep the character). No caller of the platform passes those characters.
// SPDX-License-Identifier: GPL-2.0-only
use std::{
    ffi::{OsStr, OsString},
    os::unix::ffi::{OsStrExt, OsStringExt},
};

/// The character that stands for the escaped byte `b` is `ESCAPE_BASE + b` (b in 0x80..=0xff).
pub const ESCAPE_BASE: u32 = 0x10_DC00;

/// `os.fsdecode(bytes)`.
pub fn fsdecode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut rest = bytes;
    loop {
        match std::str::from_utf8(rest) {
            Ok(text) => {
                out.push_str(text);
                return out;
            }
            Err(error) => {
                let (valid, after) = rest.split_at(error.valid_up_to());
                out.push_str(std::str::from_utf8(valid).expect("valid prefix"));
                let bad = error.error_len().unwrap_or(after.len());
                for &byte in &after[..bad] {
                    out.push(escape(byte));
                }
                rest = &after[bad..];
            }
        }
    }
}

/// `os.fsdecode` of an argument or environment value.
pub fn fsdecode_os(text: &OsStr) -> String {
    fsdecode(text.as_bytes())
}

/// The character for the escaped byte `byte` (0x80..=0xff).
pub fn escape(byte: u8) -> char {
    char::from_u32(ESCAPE_BASE + u32::from(byte)).expect("private-use character")
}

/// The lone surrogate Python has where this text has `c` (U+DC80..U+DCFF), if `c` stands for one.
pub fn surrogate(c: char) -> Option<u32> {
    let code = c as u32;
    (ESCAPE_BASE + 0x80..=ESCAPE_BASE + 0xff)
        .contains(&code)
        .then(|| code - ESCAPE_BASE + 0xdc00)
}

/// Whether the text holds an escaped byte (Python: a lone surrogate).
pub fn has_surrogates(text: &str) -> bool {
    text.chars().any(|c| surrogate(c).is_some())
}

/// `os.fsencode(text)`: the escaped bytes as they were given.
pub fn fsencode(text: &str) -> OsString {
    let mut bytes = Vec::with_capacity(text.len());
    for c in text.chars() {
        match surrogate(c) {
            Some(unit) => bytes.push((unit - 0xdc00) as u8),
            None => bytes.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes()),
        }
    }
    OsString::from_vec(bytes)
}

/// The text as Python's stderr writes it (UTF-8, `errors='backslashreplace'`): a surrogate as `\udcXX`.
pub fn backslashreplace(text: &str) -> String {
    if !has_surrogates(text) {
        return text.to_owned();
    }
    text.chars()
        .map(|c| match surrogate(c) {
            Some(unit) => format!("\\u{unit:04x}"),
            None => c.to_string(),
        })
        .collect()
}

/// `str.encode()` (strict UTF-8): Python's `UnicodeEncodeError` text for the first surrogate.
pub fn encode_error(text: &str) -> Option<String> {
    let (position, unit) = text
        .chars()
        .enumerate()
        .find_map(|(i, c)| surrogate(c).map(|u| (i, u)))?;
    let run = text
        .chars()
        .skip(position)
        .take_while(|&c| surrogate(c).is_some())
        .count();
    Some(if run > 1 {
        format!(
            "'utf-8' codec can't encode characters in position {position}-{}: surrogates not allowed",
            position + run - 1
        )
    } else {
        format!("'utf-8' codec can't encode character '\\u{unit:04x}' in position {position}: surrogates not allowed")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_round_trip_as_python_escapes_them() {
        for bytes in [
            &b"plain"[..],
            b"\xff",
            b"a\x80b\xc3\xa9c",
            b"\xe2\x82",
            b"\xed\xa0\x80",
            b"\xc0\xaf",
            b"\xf4\x90\x80\x80",
        ] {
            let text = fsdecode(bytes);
            assert_eq!(fsencode(&text).as_bytes(), bytes);
        }
        assert_eq!(fsdecode(b"a\xffb"), format!("a{}b", escape(0xff)));
        assert_eq!(fsdecode(b"\xc3\xa9"), "\u{e9}");
        assert_eq!(fsdecode(b"\xe2\x82").chars().count(), 2);
        assert_eq!(backslashreplace(&fsdecode(b"x\xffy")), "x\\udcffy");
        assert!(has_surrogates(&fsdecode(b"\x80")));
        assert!(!has_surrogates("\u{10dc7f}"));
    }

    #[test]
    fn encode_errors_read_as_pythons() {
        // python3 -c "'a\udcffb'.encode()" / "'\udcff\udcfe'.encode()"
        assert_eq!(
            encode_error(&fsdecode(b"a\xffb")).unwrap(),
            "'utf-8' codec can't encode character '\\udcff' in position 1: surrogates not allowed"
        );
        assert_eq!(
            encode_error(&fsdecode(b"\xff\xfe")).unwrap(),
            "'utf-8' codec can't encode characters in position 0-1: surrogates not allowed"
        );
        assert_eq!(encode_error("plain"), None);
    }
}
