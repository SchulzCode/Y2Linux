//! The regular expressions and Python string rules the power and sleep records parse sysfs text with
//! (`re.findall` with Python's leftmost-first, non-overlapping matches; `str.split()`).
// SPDX-License-Identifier: GPL-2.0-only
pub use crate::text::{is_space as python_space, split as python_split};
use regex::Regex;
use std::sync::OnceLock;

fn cached(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("pattern"))
}

/// `re.findall(pattern, text)` for a two-group pattern.
fn pairs(pattern: &Regex, text: &str) -> Vec<(String, String)> {
    pattern
        .captures_iter(text)
        .map(|c| (c[1].to_owned(), c[2].to_owned()))
        .collect()
}

/// `dict(pairs)`: a later key replaces the value, keeping the first position.
fn dict(found: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (key, value) in found {
        match out.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = value,
            None => out.push((key, value)),
        }
    }
    out
}

/// `sleep_diagnostics.key_values`: `dict(re.findall(r'\b([a-z0-9_]+)=(\S+)', text or ''))`.
pub fn key_values(text: Option<&str>) -> Vec<(String, String)> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    dict(pairs(
        cached(&PATTERN, r"\b([a-z0-9_]+)=(\S+)"),
        text.unwrap_or_default(),
    ))
}

/// `charger_state.classify`'s `dict(re.findall(r'(\w+)=(-?[\w.]+)', charging_state or ''))`.
pub fn charger_fields(text: Option<&str>) -> Vec<(String, String)> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    dict(pairs(
        cached(&PATTERN, r"(\w+)=(-?[\w.]+)"),
        text.unwrap_or_default(),
    ))
}

/// `observe.charge_field(charging_state, 'sense_current_est_ua')`: `\bsense_current_est_ua=(-?\d+)`.
pub fn sense_current(text: Option<&str>) -> Option<i64> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = cached(&PATTERN, r"\bsense_current_est_ua=(-?\d+)");
    let found = pattern.captures(text?)?;
    crate::root::python_int(&found[1])
}

/// `re.match(r'[|+`-]*\s*([0-9]+(?:\.[0-9]+){3})$', text)` on a stripped /proc/net/fib_trie line.
pub fn trie_address(text: &str) -> Option<String> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = cached(&PATTERN, r"^[|+`-]*\s*([0-9]+(?:\.[0-9]+){3})$");
    pattern.captures(text).map(|c| c[1].to_owned())
}

/// Python `int(text, 16)`: sign, optional `0x`, hex digits with single `_` between them.
pub fn python_hex(text: &str) -> Option<i128> {
    let text = crate::root::python_strip(text);
    let (sign, rest) = match text.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, text.strip_prefix('+').unwrap_or(text)),
    };
    let digits = rest
        .strip_prefix("0x")
        .or_else(|| rest.strip_prefix("0X"))
        .map(|d| d.strip_prefix('_').unwrap_or(d))
        .unwrap_or(rest);
    let valid = !digits.is_empty()
        && !digits.starts_with('_')
        && !digits.ends_with('_')
        && !digits.contains("__")
        && digits.chars().all(|c| c.is_ascii_hexdigit() || c == '_');
    if !valid {
        return None;
    }
    i128::from_str_radix(&digits.replace('_', ""), 16)
        .ok()
        .map(|v| sign * v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn findall_and_int_follow_python() {
        assert_eq!(
            key_values(Some("stage=EXIT error=0 x==1 Bad=2 stage=again")),
            [
                ("stage".to_owned(), "again".to_owned()),
                ("error".to_owned(), "0".to_owned()),
                ("x".to_owned(), "=1".to_owned()),
            ]
        );
        assert_eq!(
            charger_fields(Some("phase=CONSTANT_CURRENT active=1 v=-4.2 stop_error=0")),
            [
                ("phase".to_owned(), "CONSTANT_CURRENT".to_owned()),
                ("active".to_owned(), "1".to_owned()),
                ("v".to_owned(), "-4.2".to_owned()),
                ("stop_error".to_owned(), "0".to_owned()),
            ]
        );
        assert_eq!(
            sense_current(Some("a=1 sense_current_est_ua=-350 b")),
            Some(-350)
        );
        assert_eq!(sense_current(Some("xsense_current_est_ua=5")), None);
        assert_eq!(trie_address("|-- 192.168.7.2"), Some("192.168.7.2".into()));
        assert_eq!(trie_address("+-- 192.168.7.0/24 2 0 2"), None);
        assert_eq!(python_split(" a\x1cb  c\n"), ["a", "b", "c"]);
        assert_eq!(python_hex("0007A8C0"), Some(0x7a8c0));
        assert_eq!(python_hex("0x_ff"), Some(255));
        assert_eq!(python_hex("zz"), None);
    }
}
