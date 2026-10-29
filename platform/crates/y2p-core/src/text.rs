//! Python's `str` whitespace and line rules, which the platform's records and sysfs readers keep: the
//! whitespace set is Unicode's plus the separators U+001C..U+001F, `splitlines` breaks at every Unicode
//! line boundary. One implementation for every group (they each had a copy).
// SPDX-License-Identifier: GPL-2.0-only

/// `str.isspace()` for one character.
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// `str.strip()`.
pub fn strip(text: &str) -> &str {
    text.trim_matches(is_space)
}

/// `str.split()` without arguments: runs of whitespace separate, no empty fields.
pub fn split(text: &str) -> Vec<&str> {
    text.split(is_space).filter(|s| !s.is_empty()).collect()
}

/// `str.splitlines()`: `\\n`, `\\r`, `\\r\\n` (once), `\\v`, `\\f`, U+001C..U+001E, U+0085, U+2028, U+2029;
/// no trailing empty line.
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
            if let Some(&(next, '\n')) = chars.peek() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_rules() {
        assert_eq!(splitlines("a\r\nb\rc\u{2028}d\n"), ["a", "b", "c", "d"]);
        assert_eq!(splitlines("a\n\nb"), ["a", "", "b"]);
        assert!(splitlines("").is_empty());
        assert_eq!(split(" a\u{1f}b\t c "), ["a", "b", "c"]);
        assert_eq!(strip("\u{1c} x \u{a0}"), "x");
    }
}
