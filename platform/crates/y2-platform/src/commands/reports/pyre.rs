//! The text rules of the scrubber and the diagnostic events, which are contract (reports already sent
//! are compared with new ones, crates/y2-platform/tests/recorded/scrubber.json pins them): Python's
//! `re`/`str` semantics where Rust's differ on text a log can hold.
//!
//! - `\s` and `str.isspace()` take U+001C..U+001F too (Rust's White_Space does not): binary junk in a
//!   log must split and strip the same.
//! - `\w`/`\b` and `str.isalnum()` are letters (L*) and numbers (N*) only: `char::is_alphanumeric`
//!   also takes combining marks (Other_Alphabetic: Devanagari vowel signs), so a Hindi title would end
//!   a word elsewhere. `\d` is Nd only (`char::is_numeric` is all of N*).
//! - `str.splitlines()` splits at eleven separators (`str::lines` at two): a JSON line with U+2028 in a
//!   title must split as before.
//! - `re.sub`/`re.finditer` with a lookbehind: Rust's regex has no lookaround, so the pattern carries
//!   its lookahead as consumed context after the group it replaces, the lookbehind is checked here and
//!   a refused start is retried one character on, as Python's search does.
// SPDX-License-Identifier: GPL-2.0-only
use regex::{Captures, Regex};
use std::sync::OnceLock;
pub use y2p_core::text::{is_space as isspace, splitlines};

/// Python's `\s` inside a character class.
pub const S: &str = r"\s\x1C-\x1F";

/// Whether `c` is in the Unicode class `pattern` (a constant like `^\p{Nd}$`).
fn in_class(c: char, cell: &'static OnceLock<Regex>, pattern: &str) -> bool {
    let mut buffer = [0u8; 4];
    lazy(cell, pattern).is_match(c.encode_utf8(&mut buffer))
}

/// `str.isalnum()` for one character: a letter (L*) or a number (N*).
pub fn isalnum(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphanumeric();
    }
    static CELL: OnceLock<Regex> = OnceLock::new();
    in_class(c, &CELL, r"^[\p{L}\p{N}]$")
}

/// `str.isdecimal()` (Python's `\d`): a decimal digit of any script (Nd).
pub fn isdecimal(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_digit();
    }
    static CELL: OnceLock<Regex> = OnceLock::new();
    in_class(c, &CELL, r"^\p{Nd}$")
}

/// A character of Python's `\w`.
pub fn is_word(c: char) -> bool {
    c == '_' || isalnum(c)
}

/// `str.rstrip()`.
pub fn rstrip(text: &str) -> &str {
    text.trim_end_matches(isspace)
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
        let Some(m) = matched(&caps, group) else {
            break;
        };
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
        let Some(m) = matched(&caps, group) else {
            continue;
        };
        out.push_str(&text[copied..m.start()]);
        out.push_str(&replace(&caps));
        copied = m.end();
    }
    out.push_str(&text[copied..]);
    out
}

/// The replaced group of a match: every pattern here has it outside any alternative, so it took part
/// whenever the pattern matched (the whole match otherwise).
fn matched<'t>(caps: &Captures<'t>, group: usize) -> Option<regex::Match<'t>> {
    caps.get(group).or_else(|| caps.get(0))
}

/// A regex with room for the scrubber's prefix tree of known values (thousands of titles).
pub fn try_compile(pattern: &str) -> Result<Regex, regex::Error> {
    regex::RegexBuilder::new(pattern)
        .size_limit(1 << 28)
        .dfa_size_limit(1 << 26)
        .nest_limit(4096)
        .build()
}

/// A constant pattern of this group. One that does not compile is a programming error the tests meet
/// first (each pattern is used by a test).
pub fn constant(pattern: &str) -> Regex {
    try_compile(pattern).expect("a constant pattern")
}

/// A constant pattern, compiled once.
pub fn lazy(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| constant(pattern))
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
        let re = constant(r"(ab)(?:[^b]|\z)");
        let text = "xab abb ab";
        let starts: Vec<usize> = finditer(&re, text, 1, |t, at| before(t, at) != Some('x'))
            .iter()
            .map(|c| c.get(1).unwrap().start())
            .collect();
        assert_eq!(starts, [8]);
    }
}
