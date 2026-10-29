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

/// One /proc/interrupts row: its key (`12`, `IPI0`), the leading per-CPU counts and the words after
/// them (chip, hardware IRQ, trigger, name).
pub struct InterruptRow<'a> {
    pub key: &'a str,
    pub counts: Vec<&'a str>,
    pub rest: Vec<&'a str>,
}

/// One /proc/interrupts line as a row (a line without `:`, such as the CPU header, is none).
pub fn interrupt_row(line: &str) -> Option<InterruptRow<'_>> {
    let (key, tail) = line.split_once(':')?;
    let words = split(tail);
    let digits = words
        .iter()
        .take_while(|w| !w.is_empty() && w.bytes().all(|b| b.is_ascii_digit()))
        .count();
    Some(InterruptRow {
        key: strip(key),
        counts: words[..digits].to_vec(),
        rest: words[digits..].to_vec(),
    })
}

/// The rows of a /proc/interrupts text.
pub fn interrupt_rows(text: &str) -> Vec<InterruptRow<'_>> {
    splitlines(text)
        .into_iter()
        .filter_map(interrupt_row)
        .collect()
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
