//! The scrubber's rules (tests/test_error_reports.py: Report, Scrubbing): patterns, values by key and
//! JSON lines scrubbed as text.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::commands::reports::test_player::plain;
use crate::commands::reports::{loads_text, Value};
use serde_json::json;

fn scrubber(known: &[(&'static str, &[&str])]) -> Scrubber {
    let known: Known = known
        .iter()
        .map(|(n, v)| (*n, v.iter().map(|s| s.to_string()).collect()))
        .collect();
    Scrubber::new(&known, *b"fedcba9876543210").unwrap()
}

/// `plain(json.dumps(value))` of a scrubbed value and of the expected one.
fn same(scrubbed: &Value, expected: &str) {
    assert_eq!(
        plain(&serde_json::to_string(scrubbed).unwrap_or_default()),
        plain(&serde_json::to_string(&loads_text(expected).unwrap()).unwrap_or_default())
    );
}

fn value(text: &str) -> Value {
    loads_text(text).unwrap()
}

#[test]
fn patterns() {
    let mut s = scrubber(&[("ssid", &["Home"])]);
    let mut check = |text: &str, expected: &str| assert_eq!(plain(&s.text(text)), plain(expected));
    check("Home wifi, Homepage", "<ssid#> wifi, Homepage");
    check("fe80::1c2d:3eff:fe4f:5a6b up", "<ip6:link#> up");
    check("std::fmt and a::b", "std::fmt and a::b");
    check(
        "at 20:39:14 v6.18.56 build 2026.08",
        "at 20:39:14 v6.18.56 build 2026.08",
    );
    check(
        "00:00:00:00:00:00 ff:ff:ff:ff:ff:ff",
        "00:00:00:00:00:00 ff:ff:ff:ff:ff:ff",
    );
    check(
        "/media/sd/Some Folder/x y.mp3, then",
        "/media/sd/<path>.mp3, then",
    );
    check(
        "dir /media/internal/A Folder Name",
        "dir /media/internal/<path>",
    );
    check("serial=ABCDEF0123456789", "serial=<serial#>");
    check(
        "/dev/mmcblk1p1: UUID=\"6ACE-5706\" TYPE=\"vfat\"",
        "/dev/mmcblk1p1: UUID=\"<serial#>\" TYPE=\"vfat\"",
    );
    same(
        &s.value(&value(r#"{"title": "x", "n": ["Home"]}"#), None),
        r#"{"title": "<title>", "n": ["<ssid#>"]}"#,
    );
    same(
        &s.value(
            &value(r#"{"path": "/media/sd/a", "uuid": "6ACE-5706"}"#),
            None,
        ),
        r#"{"path": "/media/sd/<path>", "uuid": "<uuid>"}"#,
    );
}

#[test]
fn json_lines_scrubbed_as_text_keep_the_rules_of_values() {
    let mut s = scrubber(&[
        ("ssid", &["Caf\u{e9} W\u{f6}rth"]),
        ("music", &["Canary Song Title"]),
    ]);
    let lines = [
        r#"{"event": "joined", "message": "joined Café Wörth"}"#, // \u escapes
        "{\"event\": \"joined\", \"message\": \"joined Caf\u{e9} W\u{f6}rth\"}",
        r#"{"event": "play", "Title": "Anything At All", "n": 1}"#, // the key rule
        r#"{"peer": {"address": "11:22:33:44:55:66", "name": "Neighbour Buds"}}"#,
        r#"{"note": "from /media/sd/A Folder/x.flac to 192.168.1.20"}"#,
        r#"{"volume": {"uuid": "C4A2-77E1", "label": ""}, "artist": ["Canary \"Band\"", "B"]}"#,
        r#"{"volume": {"UUID": "Say \\\"hi\\\"", "n": 2}}"#,
    ];
    let out = blob(
        &mut s,
        "/data/reborn/logs/reborn-current.jsonl",
        lines.join("\n").as_bytes(),
    );
    let out = String::from_utf8(out).unwrap();
    for secret in [
        "Caf",
        "Anything At All",
        "Neighbour",
        "A Folder",
        "192.168.1.20",
        "44:55:66",
        "C4A2",
        "Canary",
        "hi",
    ] {
        assert!(!out.contains(secret), "{secret} in {out}");
    }
    let rows: Vec<serde_json::Value> = out
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows[0]["message"], rows[1]["message"]); // one token for both forms
    assert_eq!(
        plain(rows[0]["message"].as_str().unwrap()),
        "joined <ssid#>"
    );
    assert_eq!(rows[2]["Title"], "<title>");
    assert_eq!(rows[3]["peer"]["name"], "<bt_name>");
    assert_eq!(
        plain(rows[4]["note"].as_str().unwrap()),
        "from /media/sd/<path>.flac to <ip:private#>"
    );
    assert_eq!(
        rows[5],
        json!({"volume": {"uuid": "<uuid>", "label": ""}, "artist": ["<artist>", "<artist>"]})
    );
    assert_eq!(rows[6], json!({"volume": {"UUID": "<uuid>", "n": 2}}));
}

#[test]
fn unpaired_bluetooth_names_next_to_an_address_are_private() {
    let mut s = scrubber(&[]);
    same(
        &s.value(
            &value(r#"{"devices": [{"name": "Neighbour Headphones", "address": "11:22:33:44:55:66"}]}"#),
            None,
        ),
        r#"{"devices": [{"name": "<bt_name>", "address": "11:22:33:<mac#>"}]}"#,
    );
    same(
        &s.value(&value(r#"{"name": "BAT0", "type": "Battery"}"#), None),
        r#"{"name": "BAT0", "type": "Battery"}"#,
    );
}
