//! Where the scrubber finds music paths, URLs, e-mail addresses, serial numbers, MAC and IP
//! addresses: Python's patterns (report.py), lookbehinds checked by hand.
// SPDX-License-Identifier: GPL-2.0-only
use super::{lazy, pyre, OnceLock, Regex, S};

const MEDIA_EXTENSIONS: &str = "flac|mp3|m4a|mp4|aac|ogg|oga|opus|wav|aiff?|ape|wv|dsf|dff|alac|\
                                jpe?g|png|webp|bmp|gif|txt|cue|m3u8?|pls|lrc";

/// A music path ends at its file extension; without one (a folder, a name with spaces) the rest of
/// the line goes too: privacy before context.
fn media() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        pyre::constant(&format!(
            r#"(?i)((?:/run)?/media/[^\n\t"']*?\.(?:{MEDIA_EXTENSIONS}))(?:[{S}"',:;)\]]|\z)|((?:/run)?/media/[^\n\t"']*)"#
        ))
    })
}

pub(super) fn media_sub(text: &str, mut replace: impl FnMut(&str) -> String) -> String {
    let re = media();
    let mut out = String::with_capacity(text.len());
    let (mut copied, mut at) = (0, 0);
    while at <= text.len() {
        let Some(caps) = re.captures_at(text, at) else {
            break;
        };
        let m = caps.get(1).or_else(|| caps.get(2)).expect("one side");
        out.push_str(&text[copied..m.start()]);
        out.push_str(&replace(m.as_str()));
        copied = m.end();
        at = m.end().max(m.start() + 1);
    }
    out.push_str(&text[copied..]);
    out
}

pub(super) fn url() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    lazy(
        &CELL,
        &format!(r#"(https?|rtsp|icy)://([^/{S}"'<>?#]+)([^{S}"'<>]*)"#),
    )
}

// Every pattern starts only at a boundary and is bounded: a long run of hex or letters (a hash, a key,
// a corrupt log) must not make a scan quadratic.
pub(super) fn email() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    lazy(
        &CELL,
        r"([A-Za-z0-9._%+-]{1,64}@[A-Za-z0-9-]{1,63}(?:\.[A-Za-z0-9-]{1,63}){0,8}\.[A-Za-z]{2,24})(?:[^A-Za-z0-9-]|\z)",
    )
}

pub(super) fn serial_field() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    lazy(
        &CELL,
        &format!(
            r#"(?i)(serial(?:_number)?|cid|machine[-_]id|(?:part)?uuid)([{S}]*[:=][{S}]*"?)([0-9A-Za-z-]{{8,}})"#
        ),
    )
}

pub(super) fn ipv6() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    lazy(
        &CELL,
        r"((?:[0-9A-Fa-f]{0,4}:){2,7}[0-9A-Fa-f]{0,4})(?:[^0-9A-Za-z:]|\z)",
    )
}

pub(super) fn hex(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

/// IPV6's `(?<![0-9A-Za-z:])(?=[0-9A-Fa-f:]{0,39}::|(?:[0-9A-Fa-f]{1,4}:){5})`.
pub(super) fn ipv6_start(text: &str, at: usize) -> bool {
    if pyre::before(text, at).is_some_and(|c| c.is_ascii_alphanumeric() || c == ':') {
        return false;
    }
    let rest = &text.as_bytes()[at..];
    let run = rest.iter().take_while(|&&b| hex(b) || b == b':').count();
    if (0..=39).any(|k| k + 2 <= run && &rest[k..k + 2] == b"::") {
        return true;
    }
    let mut i = 0;
    for _ in 0..5 {
        let digits = rest[i..].iter().take_while(|&&b| hex(b)).count();
        if digits == 0 || digits > 4 || rest.get(i + digits) != Some(&b':') {
            return false;
        }
        i += digits + 1;
    }
    true
}

/// MAC's `(?<![0-9A-Fa-f])hh(sep)hh\2hh\2hh\2hh\2hh(?![0-9A-Fa-f])` at byte `at`.
fn mac_at(text: &str, at: usize) -> bool {
    let b = text.as_bytes();
    if at + 17 > b.len() || (at > 0 && hex(b[at - 1])) {
        return false;
    }
    let sep = b[at + 2];
    if !matches!(sep, b':' | b'_' | b'-') {
        return false;
    }
    (0..6).all(|i| hex(b[at + i * 3]) && hex(b[at + i * 3 + 1]))
        && (1..6).all(|i| b[at + i * 3 - 1] == sep)
        && !b.get(at + 17).copied().is_some_and(hex)
}

pub fn mac_search(text: &str) -> bool {
    (0..text.len()).any(|at| mac_at(text, at))
}

pub(super) fn mac_sub(text: &str, mut replace: impl FnMut(&str, usize) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let (mut copied, mut at) = (0, 0);
    while at + 17 <= text.len() {
        if mac_at(text, at) {
            out.push_str(&text[copied..at]);
            out.push_str(&replace(text, at));
            at += 17;
            copied = at;
        } else {
            at += 1;
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// IPV4: `(?<![\d.])(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})(?![\d.]|\.\d)` (Unicode digits).
fn ipv4_at(text: &str, at: usize) -> Option<usize> {
    if pyre::before(text, at).is_some_and(|c| c == '.' || pyre::isdecimal(c)) {
        return None;
    }
    let mut i = at;
    for part in 0..4 {
        let start = i;
        let mut count = 0;
        while let Some(c) = pyre::after(text, i) {
            if !pyre::isdecimal(c) {
                break;
            }
            count += 1;
            i += c.len_utf8();
        }
        if count == 0 || count > 3 || i == start {
            return None;
        }
        let next = pyre::after(text, i);
        if part < 3 {
            if next != Some('.') {
                return None;
            }
            i += 1;
        } else if next.is_some_and(|c| c == '.' || pyre::isdecimal(c)) {
            return None;
        }
    }
    Some(i)
}

pub(super) fn ipv4_sub(text: &str, mut replace: impl FnMut(&str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let (mut copied, mut at) = (0, 0);
    while at < text.len() {
        let c = pyre::after(text, at).expect("in text");
        if pyre::isdecimal(c) {
            if let Some(end) = ipv4_at(text, at) {
                out.push_str(&text[copied..at]);
                out.push_str(&replace(&text[at..end]));
                copied = end;
                at = end;
                continue;
            }
        }
        at += c.len_utf8();
    }
    out.push_str(&text[copied..]);
    out
}
