//! Python `re` and `str` semantics the scrubber depends on, where Rust's differ: `\s` (Python adds
//! U+001C..U+001F), `\w` and `\b` (Python's word characters are `str.isalnum()` and `_`: letters and
//! numbers, no combining marks), `str.splitlines()` (eleven separators), and `re.sub` with a lookbehind
//! (Rust's regex has no lookaround: the pattern carries its lookahead as consumed context after the
//! group it replaces, the lookbehind is checked here and a refused start is retried one character on,
//! as Python's search does).
// SPDX-License-Identifier: GPL-2.0-only
use regex::{Captures, Regex};
use std::sync::OnceLock;

/// Python's `\s` inside a character class.
pub const S: &str = r"\s\x1C-\x1F";

fn unicode(pattern: &'static str, cell: &'static OnceLock<Regex>) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("class"))
}

fn one(c: char, pattern: &'static str, cell: &'static OnceLock<Regex>) -> bool {
    let mut buffer = [0u8; 4];
    unicode(pattern, cell).is_match(c.encode_utf8(&mut buffer))
}

/// `str.isalnum()` for one character: a letter (L*) or a number (N*).
pub fn isalnum(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphanumeric();
    }
    static CELL: OnceLock<Regex> = OnceLock::new();
    one(c, r"^[\p{L}\p{N}]$", &CELL)
}

/// `str.isdecimal()` (Python's `\d`).
pub fn isdecimal(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_digit();
    }
    static CELL: OnceLock<Regex> = OnceLock::new();
    one(c, r"^\p{Nd}$", &CELL)
}

/// A character of Python's `\w`.
pub fn is_word(c: char) -> bool {
    c == '_' || isalnum(c)
}

/// `str.isspace()`.
pub fn isspace(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// `str.rstrip()`.
pub fn rstrip(text: &str) -> &str {
    text.trim_end_matches(isspace)
}

/// `str.strip()`.
pub fn strip(text: &str) -> &str {
    text.trim_matches(isspace)
}

/// `str.splitlines()`.
pub fn splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        let separator = matches!(
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
        if !separator {
            continue;
        }
        lines.push(&text[start..index]);
        let mut end = index + c.len_utf8();
        if c == '\r' {
            if let Some((_, '\n')) = chars.peek() {
                chars.next();
                end += 1;
            }
        }
        start = end;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// The character before byte offset `at`.
pub fn before(text: &str, at: usize) -> Option<char> {
    text[..at].chars().next_back()
}

/// The character at byte offset `at`.
pub fn after(text: &str, at: usize) -> Option<char> {
    text[at..].chars().next()
}

/// Python's `\b` before a match whose first character is a word character.
pub fn word_start(text: &str, at: usize) -> bool {
    !before(text, at).is_some_and(is_word)
}

/// Every match Python's `re.finditer` would give: group `group` of `re` is the match (the pattern's
/// lookahead follows it as consumed context), `allowed(text, start)` its lookbehind.
pub fn finditer<'t>(
    re: &Regex,
    text: &'t str,
    group: usize,
    allowed: impl Fn(&str, usize) -> bool,
) -> Vec<Captures<'t>> {
    let mut found = Vec::new();
    let mut at = 0;
    while at <= text.len() {
        let Some(caps) = re.captures_at(text, at) else {
            break;
        };
        let m = caps.get(group).expect("the match group");
        if !allowed(text, m.start()) {
            // No match starts here; Python tries the next position.
            at = m.start() + after(text, m.start()).map_or(1, char::len_utf8);
            continue;
        }
        at = if m.end() > m.start() {
            m.end()
        } else {
            m.end() + after(text, m.end()).map_or(1, char::len_utf8)
        };
        found.push(caps);
    }
    found
}

/// `re.sub(pattern, function, text)` with `finditer`'s matches.
pub fn sub(
    re: &Regex,
    text: &str,
    group: usize,
    allowed: impl Fn(&str, usize) -> bool,
    mut replace: impl FnMut(&Captures) -> String,
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    for caps in finditer(re, text, group, allowed) {
        let m = caps.get(group).expect("the match group");
        out.push_str(&text[copied..m.start()]);
        out.push_str(&replace(&caps));
        copied = m.end();
    }
    out.push_str(&text[copied..]);
    out
}

/// A regex with Python's IGNORECASE-free semantics where it matters; panics on a bad pattern (all are
/// constants).
pub fn compile(pattern: &str) -> Regex {
    regex::RegexBuilder::new(pattern)
        .size_limit(1 << 28)
        .dfa_size_limit(1 << 26)
        .nest_limit(4096)
        .build()
        .expect("pattern")
}

pub fn lazy(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| compile(pattern))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_split_and_classify_as_python() {
        assert_eq!(
            splitlines("a\nb\r\nc\rd\x1ce\u{2028}f\n"),
            ["a", "b", "c", "d", "e", "f"]
        );
        assert_eq!(splitlines(""), Vec::<&str>::new());
        assert_eq!(splitlines("\n"), [""]);
        assert!(is_word('é') && is_word('½') && is_word('_') && !is_word('-'));
        assert!(
            !isalnum('\u{301}'),
            "a combining mark is not alnum in Python"
        );
        assert!(isdecimal('٣') && !isdecimal('½'));
        assert_eq!(rstrip("a \x1f\n"), "a");
    }

    #[test]
    fn lookbehind_retries_the_next_start() {
        let re = compile(r"(ab)(?:[^b]|\z)");
        let text = "xab abb ab";
        let starts: Vec<usize> = finditer(&re, text, 1, |t, at| before(t, at) != Some('x'))
            .iter()
            .map(|c| c.get(1).unwrap().start())
            .collect();
        assert_eq!(starts, [8]);
    }
}
