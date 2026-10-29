//! The regular expressions and Python string rules the power and sleep records parse sysfs text with
//! (`re.findall` with Python's leftmost-first, non-overlapping matches; `str.split()`).
// SPDX-License-Identifier: GPL-2.0-only
pub use crate::text::{is_space as python_space, split as python_split};
use regex::Regex;

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
    dict(pairs(
        crate::regex!(r"\b([a-z0-9_]+)=(\S+)"),
        text.unwrap_or_default(),
    ))
}

/// `charger_state.classify`'s `dict(re.findall(r'(\w+)=(-?[\w.]+)', charging_state or ''))`.
pub fn charger_fields(text: Option<&str>) -> Vec<(String, String)> {
    dict(pairs(
        crate::regex!(r"(\w+)=(-?[\w.]+)"),
        text.unwrap_or_default(),
    ))
}

/// `observe.charge_field(charging_state, 'sense_current_est_ua')`: `\bsense_current_est_ua=(-?\d+)`.
pub fn sense_current(text: Option<&str>) -> Option<i64> {
    let pattern = crate::regex!(r"\bsense_current_est_ua=(-?\d+)");
    let found = pattern.captures(text?)?;
    crate::root::python_int(&found[1])
}

/// `re.match(r'[|+`-]*\s*([0-9]+(?:\.[0-9]+){3})$', text)` on a stripped /proc/net/fib_trie line.
pub fn trie_address(text: &str) -> Option<String> {
    let pattern = crate::regex!(r"^[|+`-]*\s*([0-9]+(?:\.[0-9]+){3})$");
    pattern.captures(text).map(|c| c[1].to_owned())
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
    }
}
