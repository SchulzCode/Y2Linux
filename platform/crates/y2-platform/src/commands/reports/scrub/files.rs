//! One file's bytes, scrubbed by its kind: JSON documents value by value, JSON lines as text with
//! their private keys' strings replaced, archives member by member, anything else as text.
// SPDX-License-Identifier: GPL-2.0-only
use super::{pyre, tarball, Captures, OnceLock, Regex, Scrubber, PRIVATE_KEYS, S, SECTION_MAX};
use crate::commands::reports::loads_text;

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
        pyre::constant(&format!(
            r#"(?i)"(?:{})"[{S}]*:[{S}]*[\[{{]|"address"[{S}]*:"#,
            keys()
        ))
    })
}

fn private_string() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| {
        pyre::constant(&format!(
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

/// One file's bytes, scrubbed by its kind (JSON, JSON lines, tar, text).
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
            match loads_text(line) {
                Some(value) => {
                    let clean = scrubber.value(&value, None);
                    lines.push(serde_json::to_string(&clean).unwrap_or_default());
                }
                None => lines.push(scrubber.text(line)),
            }
        }
        return (lines.join("\n") + "\n").into_bytes();
    }
    if path.ends_with(".json") {
        if let Some(value) = loads_text(&text) {
            let clean = scrubber.value(&value, None);
            return serde_json::to_string_pretty(&clean)
                .unwrap_or_default()
                .into_bytes();
        }
    }
    scrubber.text(&text).into_bytes()
}

/// The members of a retained tar (boot archives, Reborn diagnostics), each scrubbed.
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
