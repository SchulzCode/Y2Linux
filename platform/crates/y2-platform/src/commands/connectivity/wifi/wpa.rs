//! wpa_cli and its tab-separated answers, and the `KEY=value` searches over supplicant event text.
// SPDX-License-Identifier: GPL-2.0-only
use y2p_core::{command, Root};

/// A word character (a Unicode letter, digit or `_`): the searches below match whole words only.
fn word(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// `re.search(r'\b(?:KEY|…)=(V{1,max})\b', text).group(1)`, for a value class `V` of word characters:
/// the first `KEY=` after a non-word character whose run of `V` is at most `max` long and is followed
/// by a non-word character.
pub(super) fn search_value<'a>(
    text: &'a str,
    keys: &[&str],
    class: impl Fn(char) -> bool,
    max: usize,
) -> Option<&'a str> {
    let mut previous = None;
    for (index, c) in text.char_indices() {
        if !word(previous) {
            for key in keys {
                let Some(rest) = text[index..]
                    .strip_prefix(key)
                    .and_then(|r| r.strip_prefix('='))
                else {
                    continue;
                };
                let run = rest.find(|c: char| !class(c)).unwrap_or(rest.len());
                let length = rest[..run].chars().count();
                if (1..=max).contains(&length) && !word(rest[run..].chars().next()) {
                    return Some(&rest[..run]);
                }
            }
        }
        previous = Some(c);
    }
    None
}

/// `re.findall(r'\[[A-Z0-9+_-]{1,24}\]', text)`, joined.
pub(super) fn flags(text: &str) -> String {
    let class = |c: char| c.is_ascii_uppercase() || c.is_ascii_digit() || "+_-".contains(c);
    let (mut found, mut rest) = (String::new(), text);
    while let Some(start) = rest.find('[') {
        let after = &rest[start + 1..];
        let run = after.find(|c: char| !class(c)).unwrap_or(after.len());
        if (1..=24).contains(&run) && after[run..].starts_with(']') {
            found.push_str(&rest[start..start + run + 2]);
            rest = &after[run + 1..];
        } else {
            rest = after;
        }
    }
    found
}

/// `str.isdigit()` for the ASCII text wpa_cli prints.
pub(super) fn digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// `re.fullmatch(r'-?\d+', text)`.
pub(super) fn signed(text: &str) -> bool {
    digits(text.strip_prefix('-').unwrap_or(text))
}

pub(super) fn int(text: &str) -> i64 {
    text.parse().unwrap_or(i64::MAX)
}

pub(super) fn rows(output: Option<&str>) -> Vec<Vec<String>> {
    output
        .unwrap_or_default()
        .lines()
        .skip(1)
        .map(|line| line.split('\t').map(str::to_owned).collect())
        .collect()
}

pub(super) fn wpa(root: &Root, argv: &[&str]) -> command::Outcome {
    let mut full = vec!["wpa_cli", "-i", "wlan0"];
    full.extend_from_slice(argv);
    command::run(root, &full, 2.0)
}
