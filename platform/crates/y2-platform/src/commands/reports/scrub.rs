//! `report.Scrubber`: replace personal data; tokens are stable within one report.
//!
//! A token names its value by a keyed hash (`<ssid#3fa2c1>`): the same value gets the same token in
//! every part of a report, also when parts are scrubbed in parallel, and the per-report random key
//! (never written) keeps a short hash of, say, an IP address from being guessed back.
//!
//! Every pattern is report.py's, with Python's semantics: lookbehinds are checked by hand (`pyre`),
//! lookaheads are consumed context after the replaced group, `\s`/`\w`/`\b` are Python's.
// SPDX-License-Identifier: GPL-2.0-only
use super::pyjson::{self, Py};
use super::pyre::{self, lazy, S};
use super::tarball;
use hmac::{Hmac, Mac};
use regex::{Captures, Regex};
use sha2::Sha256;
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, OnceLock},
};

/// Raw bytes per section (logs keep their newest end).
pub const SECTION_MAX: usize = 4 * 1024 * 1024;

/// JSON keys whose values are music metadata or account data, whatever the source.
pub const PRIVATE_KEYS: [&str; 30] = [
    "title",
    "artist",
    "album",
    "album_artist",
    "albumartist",
    "composer",
    "genre",
    "filename",
    "comment",
    "lyrics",
    "ssid",
    "bssid",
    "psk",
    "passphrase",
    "password",
    "session_key",
    "username",
    "user",
    "email",
    "station",
    "stream_url",
    "playlist",
    "playlist_name",
    "query",
    // Filesystem identity of the user's cards (a FAT volume serial, a label they chose).
    "uuid",
    "fs_uuid",
    "partuuid",
    "label",
    "volume_label",
    "volume_id",
];

/// The classes of known secrets, in `secrets()`'s order.
pub const CLASSES: [&str; 7] = [
    "ssid",
    "wifi_secret",
    "bt_name",
    "account",
    "ssh",
    "serial",
    "music",
];

pub type Known = Vec<(&'static str, Vec<String>)>;

/// What every worker shares: the known values and their one pattern.
struct Shared {
    /// value (or its JSON-escaped form) -> class.
    known: HashMap<String, &'static str>,
    /// escaped form -> the value it stands for (one token for both forms).
    original: HashMap<String, String>,
    pattern: Option<Regex>,
}

#[derive(Clone)]
pub struct Scrubber {
    key: [u8; 16],
    shared: Arc<Shared>,
    pub counts: BTreeMap<String, u64>,
}

/// One regex for many literal strings, shaped as a prefix tree: the engine then follows one branch
/// per character instead of trying every string at every position (2000 queue titles: 4 s →
/// milliseconds per 200 KB). Optional tails are greedy, so the longest string wins.
pub fn trie_pattern(values: &[&str]) -> Option<String> {
    #[derive(Default)]
    struct Node {
        end: bool,
        children: BTreeMap<char, Node>,
    }
    if values.is_empty() {
        return None;
    }
    let mut trie = Node::default();
    for value in values {
        let mut node = &mut trie;
        for c in value.chars() {
            node = node.children.entry(c).or_default();
        }
        node.end = true;
    }
    fn pattern(node: &Node) -> String {
        let branches: Vec<String> = node
            .children
            .iter()
            .map(|(c, child)| regex::escape(&c.to_string()) + &pattern(child))
            .collect();
        if branches.is_empty() {
            return String::new();
        }
        let body = if branches.len() == 1 {
            branches[0].clone()
        } else {
            format!("(?:{})", branches.join("|"))
        };
        if node.end {
            format!("(?:{body})?")
        } else {
            body
        }
    }
    Some(pattern(&trie))
}

impl Scrubber {
    pub fn new(known: &Known, key: [u8; 16]) -> Scrubber {
        let mut pairs: Vec<(String, &'static str)> = Vec::new();
        let mut original = HashMap::new();
        for (name, values) in known {
            for value in values {
                pairs.push((value.clone(), name));
                // Also as written inside JSON by Python (\u escapes, \"): JSON lines are scrubbed as
                // text (_scrub_blob).
                let mut escaped = String::new();
                pyjson::string(&mut escaped, value);
                let escaped = escaped[1..escaped.len() - 1].to_owned();
                if escaped != *value {
                    pairs.push((escaped.clone(), name));
                    original.insert(escaped, value.clone());
                }
            }
        }
        // Longest first so a passphrase containing the SSID is replaced whole.
        pairs.sort_by(|a, b| {
            b.0.chars()
                .count()
                .cmp(&a.0.chars().count())
                .then_with(|| a.0.cmp(&b.0))
        });
        let values: Vec<&str> = pairs.iter().map(|(v, _)| v.as_str()).collect();
        let pattern = trie_pattern(&values).map(|p| pyre::compile(&p));
        let mut map = HashMap::new();
        for (value, name) in pairs {
            map.insert(value, name); // dict(pairs): the last pair of a value wins
        }
        Scrubber {
            key,
            shared: Arc::new(Shared {
                known: map,
                original,
                pattern,
            }),
            counts: BTreeMap::new(),
        }
    }

    /// A fresh worker with the same key and values (`_worker_start`).
    pub fn worker(&self) -> Scrubber {
        Scrubber {
            key: self.key,
            shared: Arc::clone(&self.shared),
            counts: BTreeMap::new(),
        }
    }

    pub fn count(&mut self, name: &str) {
        *self.counts.entry(name.to_owned()).or_default() += 1;
    }

    pub fn token(&mut self, name: &str, value: &str, keep: &str) -> String {
        self.count(name);
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.key).expect("any key length");
        mac.update(format!("{name}\0{value}").as_bytes());
        let digest = mac.finalize().into_bytes();
        format!(
            "<{name}{keep}#{:02x}{:02x}{:02x}>",
            digest[0], digest[1], digest[2]
        )
    }

    fn known(&mut self, caps: &Captures, text: &str) -> String {
        let m = caps.get(0).expect("match");
        let value = m.as_str();
        // Word-edged values only replace whole words ("Home", not "Homepage").
        let first = value.chars().next().expect("non-empty");
        let last = value.chars().next_back().expect("non-empty");
        if pyre::isalnum(first) && pyre::before(text, m.start()).is_some_and(pyre::isalnum) {
            return value.to_owned();
        }
        if pyre::isalnum(last) && pyre::after(text, m.end()).is_some_and(pyre::isalnum) {
            return value.to_owned();
        }
        let shared = Arc::clone(&self.shared);
        let name = shared.known[value];
        let original = shared
            .original
            .get(value)
            .map_or(value, String::as_str)
            .to_owned();
        self.token(name, &original, "")
    }

    fn mac(&mut self, text: &str, at: usize) -> String {
        let bytes = &text.as_bytes()[at..at + 17];
        let octets: Vec<&str> = (0..6)
            .map(|i| std::str::from_utf8(&bytes[i * 3..i * 3 + 2]).expect("ascii"))
            .collect();
        let whole = &text[at..at + 17];
        if octets.iter().all(|o| *o == "00") || octets.iter().all(|o| o.eq_ignore_ascii_case("ff"))
        {
            return whole.to_owned();
        }
        let sep = &whole[2..3];
        let maker = octets[..3].join(sep).to_ascii_uppercase();
        let token = self.token("mac", &octets.join(sep).to_ascii_uppercase(), "");
        format!("{maker}{sep}{token}")
    }

    fn ipv4(&mut self, text: &str) -> String {
        match ipv4_kind(text) {
            Some(kind) => self.token("ip", text, &format!(":{kind}")),
            None => text.to_owned(),
        }
    }

    fn ipv6(&mut self, text: &str) -> String {
        let Some(address) = parse_ipv6(text) else {
            return text.to_owned();
        };
        let Some(kind) = ipv6_kind(address) else {
            return text.to_owned(); // loopback, unspecified
        };
        if text.matches(':').count() < 3 && !text.chars().any(|c| c.is_ascii_digit()) {
            return text.to_owned(); // "a::b", "dead::beef": a path or a name, not an address
        }
        self.token("ip6", text, &format!(":{kind}"))
    }

    fn media(&mut self, matched: &str) -> String {
        let text = pyre::rstrip(matched);
        let extension = text.rsplit_once('.').and_then(|(_, ext)| {
            (!ext.is_empty() && ext.len() <= 5 && ext.bytes().all(|b| b.is_ascii_alphanumeric()))
                .then_some(ext)
        });
        self.count("media_path");
        let root = if text.starts_with("/media/sd") {
            "/media/sd"
        } else if text.starts_with("/media/internal") {
            "/media/internal"
        } else {
            "/media"
        };
        format!(
            "{root}/<path>{}",
            extension.map_or(String::new(), |e| format!(".{e}"))
        )
    }

    /// `Scrubber.text`.
    pub fn text(&mut self, value: &str) -> String {
        if value.is_empty() {
            return String::new();
        }
        let mut value = value.to_owned();
        // Paths and URLs first: their shape is lost once a name inside is replaced. A pattern runs
        // only when the text holds a character it needs.
        if value.contains("/media/") {
            value = media_sub(&value, |m| self.media(m));
        }
        if value.contains("://") {
            value = pyre::sub(url(), &value, 0, pyre::word_start, |caps| {
                let path = caps.get(3).map_or("", |m| m.as_str());
                if path.is_empty() || path == "/" {
                    caps[0].to_owned()
                } else {
                    format!("{}://{}/<path>", &caps[1], &caps[2])
                }
            });
        }
        if let Some(pattern) = self.shared.pattern.clone() {
            let text = value.clone();
            value = pyre::sub(
                &pattern,
                &text,
                0,
                |_, _| true,
                |caps| self.known(caps, &text),
            );
        }
        if value.contains('@') {
            value = pyre::sub(
                email(),
                &value,
                1,
                |t, at| {
                    !pyre::before(t, at)
                        .is_some_and(|c| c.is_ascii_alphanumeric() || "._%+-".contains(c))
                },
                |caps| {
                    let address = caps[1].to_owned();
                    self.token("email", &address, "")
                },
            );
        }
        let lower = value.to_lowercase();
        if ["serial", "cid", "machine", "uuid"]
            .iter()
            .any(|w| lower.contains(w))
        {
            value = pyre::sub(serial_field(), &value, 0, pyre::word_start, |caps| {
                let token = self.token("serial", &caps[3], "");
                format!("{}{}{token}", &caps[1], &caps[2])
            });
        }
        value = mac_sub(&value, |text, at| self.mac(text, at));
        if value.contains(':') {
            value = pyre::sub(ipv6(), &value, 1, ipv6_start, |caps| self.ipv6(&caps[1]));
        }
        if value.contains('.') {
            value = ipv4_sub(&value, |m| self.ipv4(m));
        }
        value
    }

    /// `Scrubber.value`.
    pub fn value(&mut self, value: &Py, key: Option<&str>) -> Py {
        match value {
            Py::Dict(items) => {
                let peer = matches!(value.get("address"), Some(Py::Str(a)) if mac_search(a));
                let mut out: Vec<(String, Py)> = Vec::new();
                for (k, v) in items {
                    let name = self.text(k);
                    let clean = if peer
                        && (k == "name" || k == "alias")
                        && matches!(v, Py::Str(s) if !s.is_empty())
                    {
                        Py::str("<bt_name>")
                    } else {
                        self.value(v, Some(k))
                    };
                    match out.iter_mut().find(|(n, _)| *n == name) {
                        Some(entry) => entry.1 = clean,
                        None => out.push((name, clean)),
                    }
                }
                Py::Dict(out)
            }
            Py::List(items) => Py::List(items.iter().map(|v| self.value(v, key)).collect()),
            Py::Str(s) => {
                if let Some(key) = key {
                    let lower = key.to_lowercase();
                    if PRIVATE_KEYS.contains(&lower.as_str()) && !s.is_empty() {
                        self.count("private_key");
                        return Py::Str(format!("<{lower}>"));
                    }
                }
                Py::Str(self.text(s))
            }
            other => other.clone(),
        }
    }
}

// ---- patterns --------------------------------------------------------------------------------------

const MEDIA_EXTENSIONS: &str = "flac|mp3|m4a|mp4|aac|ogg|oga|opus|wav|aiff?|ape|wv|dsf|dff|alac|\
                                jpe?g|png|webp|bmp|gif|txt|cue|m3u8?|pls|lrc";

/// A music path ends at its file extension; without one (a folder, a name with spaces) the rest of
/// the line goes too: privacy before context.
fn media() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        pyre::compile(&format!(
            r#"(?i)((?:/run)?/media/[^\n\t"']*?\.(?:{MEDIA_EXTENSIONS}))(?:[{S}"',:;)\]]|\z)|((?:/run)?/media/[^\n\t"']*)"#
        ))
    })
}

fn media_sub(text: &str, mut replace: impl FnMut(&str) -> String) -> String {
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

fn url() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    lazy(
        &CELL,
        &format!(r#"(https?|rtsp|icy)://([^/{S}"'<>?#]+)([^{S}"'<>]*)"#),
    )
}

// Every pattern starts only at a boundary and is bounded: a long run of hex or letters (a hash, a key,
// a corrupt log) must not make a scan quadratic.
fn email() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    lazy(
        &CELL,
        r"([A-Za-z0-9._%+-]{1,64}@[A-Za-z0-9-]{1,63}(?:\.[A-Za-z0-9-]{1,63}){0,8}\.[A-Za-z]{2,24})(?:[^A-Za-z0-9-]|\z)",
    )
}

fn serial_field() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    lazy(
        &CELL,
        &format!(
            r#"(?i)(serial(?:_number)?|cid|machine[-_]id|(?:part)?uuid)([{S}]*[:=][{S}]*"?)([0-9A-Za-z-]{{8,}})"#
        ),
    )
}

fn ipv6() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    lazy(
        &CELL,
        r"((?:[0-9A-Fa-f]{0,4}:){2,7}[0-9A-Fa-f]{0,4})(?:[^0-9A-Za-z:]|\z)",
    )
}

fn hex(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

/// IPV6's `(?<![0-9A-Za-z:])(?=[0-9A-Fa-f:]{0,39}::|(?:[0-9A-Fa-f]{1,4}:){5})`.
fn ipv6_start(text: &str, at: usize) -> bool {
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

fn mac_sub(text: &str, mut replace: impl FnMut(&str, usize) -> String) -> String {
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

fn ipv4_sub(text: &str, mut replace: impl FnMut(&str) -> String) -> String {
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

/// ipaddress's IPv4 properties: (loopback, unspecified, private, link-local).
fn ipv4_props(value: u32) -> (bool, bool, bool, bool) {
    let within = |network: u32, prefix: u32| prefix == 0 || (value ^ network) >> (32 - prefix) == 0;
    let net = |a: u8, b: u8, c: u8, d: u8| u32::from_be_bytes([a, b, c, d]);
    // ipaddress._private_networks (Python 3.13+), minus its exceptions.
    let private = [
        (net(0, 0, 0, 0), 8),
        (net(10, 0, 0, 0), 8),
        (net(127, 0, 0, 0), 8),
        (net(169, 254, 0, 0), 16),
        (net(172, 16, 0, 0), 12),
        (net(192, 0, 0, 0), 24),
        (net(192, 0, 0, 170), 31),
        (net(192, 0, 2, 0), 24),
        (net(192, 168, 0, 0), 16),
        (net(198, 18, 0, 0), 15),
        (net(198, 51, 100, 0), 24),
        (net(203, 0, 113, 0), 24),
        (net(240, 0, 0, 0), 4),
        (net(255, 255, 255, 255), 32),
    ]
    .iter()
    .any(|&(n, p)| within(n, p))
        && !(value == net(192, 0, 0, 9) || value == net(192, 0, 0, 10));
    (
        within(net(127, 0, 0, 0), 8),
        value == 0,
        private,
        within(net(169, 254, 0, 0), 16),
    )
}

/// `ipaddress.IPv4Address(text)` and the kind `_ipv4` names: `None` when kept as it is.
pub fn ipv4_kind(text: &str) -> Option<&'static str> {
    let mut octets = [0u8; 4];
    let parts: Vec<&str> = text.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    for (octet, part) in octets.iter_mut().zip(&parts) {
        if part.is_empty()
            || part.len() > 3
            || !part.bytes().all(|b| b.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return None;
        }
        *octet = part.parse().ok()?;
    }
    let value = u32::from_be_bytes(octets);
    let (loopback, unspecified, private, link) = ipv4_props(value);
    if loopback || unspecified || value == u32::MAX {
        return None;
    }
    if value >> 8 == u32::from_be_bytes([0, 10, 42, 0]) {
        return None; // the fixed USB network of every Y2Linux player
    }
    Some(if private {
        "private"
    } else if link {
        "link"
    } else {
        "public"
    })
}

/// `ipaddress.IPv6Address(text)` for the forms the pattern finds (hex groups and colons).
pub fn parse_ipv6(text: &str) -> Option<u128> {
    let parts: Vec<&str> = text.split(':').collect();
    if parts.len() < 3 || parts.len() > 9 {
        return None;
    }
    let mut skip = None;
    for (i, part) in parts.iter().enumerate().take(parts.len() - 1).skip(1) {
        if part.is_empty() {
            if skip.is_some() {
                return None;
            }
            skip = Some(i);
        }
    }
    let (hi, lo) = match skip {
        Some(index) => {
            let mut hi = index;
            let mut lo = parts.len() - index - 1;
            if parts[0].is_empty() {
                hi -= 1;
                if hi > 0 {
                    return None;
                }
            }
            if parts[parts.len() - 1].is_empty() {
                lo -= 1;
                if lo > 0 {
                    return None;
                }
            }
            if 8 < hi + lo + 1 {
                return None;
            }
            (hi, lo)
        }
        None => {
            if parts.len() != 8 || parts[0].is_empty() || parts[7].is_empty() {
                return None;
            }
            (8, 0)
        }
    };
    let hextet = |part: &str| -> Option<u128> {
        if part.is_empty() || part.len() > 4 || !part.bytes().all(hex) {
            return None;
        }
        u128::from_str_radix(part, 16).ok()
    };
    let mut value: u128 = 0;
    for part in &parts[..hi] {
        value = (value << 16) | hextet(part)?;
    }
    value = value.checked_shl(16 * (8 - hi) as u32).unwrap_or(0);
    let mut low: u128 = 0;
    for part in &parts[parts.len() - lo..] {
        low = (low << 16) | hextet(part)?;
    }
    Some(value | low)
}

/// `_ipv6`'s answer: `None` for a kept address (loopback, unspecified), else its kind: link-local,
/// else private (ipaddress, Python 3.13+), else public. An IPv4-mapped address has its IPv4 address's
/// properties.
pub fn ipv6_kind(address: u128) -> Option<&'static str> {
    let within =
        |network: u128, prefix: u32| prefix == 0 || (address ^ network) >> (128 - prefix) == 0;
    let net = |text: &str| parse_ipv6(text).expect("constant");
    if within(net("::ffff:0:0"), 96) {
        let (loopback, unspecified, private, link) = ipv4_props(address as u32);
        if loopback || unspecified {
            return None;
        }
        return Some(if link {
            "link"
        } else if private {
            "private"
        } else {
            "public"
        });
    }
    if address == 1 || address == 0 {
        return None;
    }
    if within(net("fe80::"), 10) {
        return Some("link");
    }
    let private = [
        ("::1", 128),
        ("::", 128),
        ("64:ff9b:1::", 48),
        ("100::", 64),
        ("2001::", 23),
        ("2001:db8::", 32),
        ("2002::", 16),
        ("3fff::", 20),
        ("fc00::", 7),
        ("fe80::", 10),
    ]
    .iter()
    .any(|&(n, p)| within(net(n), p));
    let exception = [
        ("2001:1::1", 128),
        ("2001:1::2", 128),
        ("2001:3::", 32),
        ("2001:4:112::", 48),
        ("2001:20::", 28),
        ("2001:30::", 28),
    ]
    .iter()
    .any(|&(n, p)| within(net(n), p));
    Some(if private && !exception {
        "private"
    } else {
        "public"
    })
}

// ---- JSON lines and blobs ----------------------------------------------------------------------------

fn keys() -> String {
    let mut keys = PRIVATE_KEYS.to_vec();
    keys.sort();
    keys.join("|")
}

/// A line with a private key holding a list or object, or a Bluetooth peer's address, goes value by
/// value.
fn structured() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        pyre::compile(&format!(
            r#"(?i)"(?:{})"[{S}]*:[{S}]*[\[{{]|"address"[{S}]*:"#,
            keys()
        ))
    })
}

fn private_string() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        pyre::compile(&format!(
            r#"(?i)"({})"([{S}]*:[{S}]*)"((?:[^"\\\n]|\\[^\n])*)""#,
            keys()
        ))
    })
}

/// JSON lines are scrubbed as one text (every string in them passes the same patterns and known
/// values): value by value, the logs of one report took ≈45 s on the Y2. A private key's string value
/// is replaced in the text too.
fn private_strings(scrubber: &mut Scrubber, line: &str) -> String {
    private_string()
        .replace_all(line, |caps: &Captures| {
            if caps[3].is_empty() {
                return caps[0].to_owned();
            }
            scrubber.count("private_key");
            format!(
                "\"{}\"{}\"<{}>\"",
                &caps[1],
                &caps[2],
                caps[1].to_lowercase()
            )
        })
        .into_owned()
}

/// `_scrub_blob`: one file's bytes, scrubbed by its kind (JSON, JSON lines, tar, text).
pub fn blob(scrubber: &mut Scrubber, path: &str, data: &[u8]) -> Vec<u8> {
    if path.ends_with(".tar") || path.ends_with(".tar.gz") {
        return tar(scrubber, data, path.ends_with(".gz"));
    }
    let text = String::from_utf8_lossy(data);
    if path.ends_with(".jsonl") {
        let mut lines = Vec::new();
        for line in pyre::splitlines(&text) {
            if !structured().is_match(line) {
                let plain = private_strings(scrubber, line);
                lines.push(scrubber.text(&plain));
                continue;
            }
            match pyjson::loads(line) {
                Some(value) => {
                    let clean = scrubber.value(&value, None);
                    lines.push(pyjson::dumps(&clean, pyjson::Style::Spaced));
                }
                None => lines.push(scrubber.text(line)),
            }
        }
        return (lines.join("\n") + "\n").into_bytes();
    }
    if path.ends_with(".json") {
        if let Some(value) = pyjson::loads(&text) {
            let clean = scrubber.value(&value, None);
            return pyjson::dumps(&clean, pyjson::Style::Indent).into_bytes();
        }
    }
    scrubber.text(&text).into_bytes()
}

/// `_scrub_tar`: members of a retained tar (boot archives, Reborn diagnostics), each scrubbed.
pub fn tar(scrubber: &mut Scrubber, data: &[u8], compressed: bool) -> Vec<u8> {
    let Ok(members) = tarball::members(data, compressed, 64, |m| {
        m.regular && m.size <= SECTION_MAX as u64
    }) else {
        return Vec::new();
    };
    let mut writer = tarball::Writer::new();
    for (member, body) in members {
        let Some(body) = body else {
            continue;
        };
        let clean = blob(scrubber, &member.name, &body);
        let name = scrubber.text(&member.name);
        writer.add(
            &tarball::Info::file(&name, clean.len(), member.mtime as i64, 0o600),
            &clean,
        );
    }
    writer.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scrubber(known: &[(&'static str, &[&str])]) -> Scrubber {
        let known: Known = known
            .iter()
            .map(|(n, v)| (*n, v.iter().map(|s| s.to_string()).collect()))
            .collect();
        Scrubber::new(&known, *b"0123456789abcdef")
    }

    /// The same texts through host Python's Scrubber (tools/platform) with the same key.
    #[test]
    fn texts_scrub_as_python() {
        let samples = [
            "wlan0: 192.168.1.40/24 gw 192.168.1.1 dns 9.9.9.9 lo 127.0.0.1 usb 10.42.0.2 bc 255.255.255.255",
            "bad 1.2.3.4567 01.2.3.4 1.2.3.4.5 a1.2.3.4 ok 8.8.8.8, 169.254.1.2 100.64.0.1 192.0.0.9",
            "mac aa:bb:cc:dd:ee:ff 00:00:00:00:00:00 FF-FF-FF-FF-FF-FF 11_22_33_44_55_66 aa:bb:cc:dd:ee:ff:00 xaa:bb:cc:dd:ee:f0",
            "v6 fe80::1 2001:db8::5 2606:4700::1111 ::1 :: a::b dead::beef 1:2:3:4:5:6:7:8 fd00::1:2 ::ffff:8.8.8.8",
            "mail me@example.com x.y+z@sub.example.co.uk u@a.bc.de5 a@b.c verylong@x.museum",
            "url https://reports.example.com/v1/x?y=z http://host/ rtsp://cam:554/live xhttps://no/path icy://radio/stream",
            "path /media/sd/Music/Artist/Song.FLAC, then /media/internal/music/a b/c.mp3 x /run/media/x y\t/media/sd/folder",
            "serial=0123456789ab cid: 0x1234567890 machine-id=\"abcdef0123456789\" PARTUUID=12345678-01 xserial=12345678",
            "Home network Homepage MyHome (Home) Living Room's Speaker \u{e9}l\u{e8}ve Ready",
            "{\"title\": \"Song \\\"A\\\"\", \"ssid\": \"Home\", \"t\": \"\\u00e9l\\u00e8ve\"}",
            "mapped ::ffff:7f00:1 ::ffff:a9fe:1 ::ffff:808:808 ::ffff:c0a8:101 64:ff9b:1::1 2001:db8::1:2:3:4 fe80::1%wlan0",
            "hex aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:bbbb:cccc::1 deadbeef00112233445566778899aabbccddeeff 1:2:3:4:5:6:7:8:9",
            "digits \u{663}.\u{664}.\u{665}.\u{666} 1.2.3.4 tail. 9.9.9.9.",
            "esc \"Song \\\"A\\\"\" \\u00e9l\\u00e8ve ok Home\u{301} xHome Home_x _Home",
            "mail a.b@c.museum, <x@y.io> .lead@x.org trailing@x.org. 12345678901234567890123456789012345678901234567890123456789012345@x.org",
            "media /media/sd/a.mp3.flac x /media/internal/music \"quoted /media/sd/b c.ogg\" /media/sd/x.FLAC) /mediax/no /media/sd/song.flac\n/media/sd/next",
            "urls https://h/ https://h/a b http://user@host:8080/p/q?x#y wss://no/x HTTPS://upper/p",
            "serials Serial: ABCDEF12 serial_number = \"12345678-abc\" uuid=1234567 cid=deadbeefcafe PARTUUID:abcdefgh",
            "mac2 aa-bb-cc-dd-ee-ff aa:bb-cc:dd:ee:ff 0a:1b:2c:3d:4e:5f; 00:11:22:33:44:55:66",
        ];
        let known: &[(&'static str, &[&str])] = &[
            ("ssid", &["Home", "Living Room"]),
            ("bt_name", &["Living Room's Speaker"]),
            ("music", &["\u{e9}l\u{e8}ve", "Song \"A\""]),
        ];
        let mut ours = scrubber(known);
        let rust: Vec<String> = samples.iter().map(|s| ours.text(s)).collect();
        // The texts and counts the Python platform's report.Scrubber gave (recorded).
        let recorded = serde_json::to_string(&(&rust, &ours.counts)).unwrap() + "\n";
        y2p_core::recorded::check(
            env!("CARGO_MANIFEST_DIR"),
            "scrubber.json",
            recorded.as_bytes(),
        );
    }

    #[test]
    fn addresses_classify_as_ipaddress() {
        assert_eq!(ipv4_kind("192.168.1.1"), Some("private"));
        assert_eq!(ipv4_kind("8.8.8.8"), Some("public"));
        assert_eq!(ipv4_kind("127.0.0.1"), None);
        assert_eq!(ipv4_kind("10.42.0.7"), None);
        assert_eq!(ipv4_kind("010.0.0.1"), None);
        assert_eq!(
            parse_ipv6("1:2:3:4:5:6:7:8"),
            Some(0x0001_0002_0003_0004_0005_0006_0007_0008)
        );
        assert_eq!(parse_ipv6("1::"), Some(1 << 112));
        assert_eq!(parse_ipv6(":1:2"), None);
        assert_eq!(parse_ipv6("1:::2"), None);
        assert_eq!(ipv6_kind(parse_ipv6("fe80::1").unwrap()), Some("link"));
        assert_eq!(
            ipv6_kind(parse_ipv6("2606:4700::1111").unwrap()),
            Some("public")
        );
        assert_eq!(ipv6_kind(parse_ipv6("fd00::1").unwrap()), Some("private"));
    }

    #[test]
    fn values_and_tries() {
        assert_eq!(trie_pattern(&["abc", "ab", "b"]).unwrap(), "(?:ab(?:c)?|b)");
        let mut s = scrubber(&[("bt_name", &["Speaker One"])]);
        let value = pyjson::loads(
            r#"{"address": "AA:BB:CC:DD:EE:FF", "name": "Speaker One", "alias": "", "Title": "x", "list": [{"Artist": "y"}]}"#,
        )
        .unwrap();
        let clean = pyjson::dumps(&s.value(&value, None), pyjson::Style::Spaced);
        let token = s.token("mac", "AA:BB:CC:DD:EE:FF", "");
        let expected = format!(
            "{{\"Title\": \"<title>\", \"address\": \"AA:BB:CC:{token}\", \"alias\": \"\", \
             \"list\": [{{\"Artist\": \"<artist>\"}}], \"name\": \"<bt_name>\"}}"
        );
        assert_eq!(clean, expected);
        assert_eq!(s.counts["private_key"], 2);
    }
}

#[cfg(test)]
#[path = "scrub_tests.rs"]
mod scrub_tests;
