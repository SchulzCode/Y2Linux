//! The secrets a report knows to replace (tests/test_error_reports.py: Report, Scrubbing).
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::commands::reports::{scrub::Scrubber, test_player as player};
use crate::testing::Fixture;
use serde_json::json;

fn class(known: &Known, name: &str) -> BTreeSet<String> {
    known
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, values)| values.iter().cloned().collect())
        .unwrap()
}

fn set(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|v| v.to_string()).collect()
}

#[test]
fn the_boot_id_is_not_taken_for_a_volume_serial() {
    let fixture = Fixture::new();
    player::player(fixture.base());
    let boot = "63c1a2b4-5d6e-4f70-8192-a3b4c5d6e7f8";
    fixture.put("/proc/sys/kernel/random/boot_id", &format!("{boot}\n"));
    fixture.put_json(
        "/run/y2/media-lifecycle.json",
        &json!({"boot_id": boot, "uuid": "C4A2-77E1"}),
    );
    let found = class(&secrets(&fixture.root).unwrap(), "serial");
    assert!(!found.contains(boot));
    assert!(found.contains("C4A2-77E1"));
}

#[test]
fn library_names_that_are_versions_or_state_words_stay() {
    // F41 (471831b6, 65fd5cfb): songs called "Ready" and "1.0" replaced the shutdown answer and
    // turned Y2LINUX-1.0.0-BETA-8.22 into <music#…>.0.
    let fixture = Fixture::new();
    fixture.put_json(
        "/data/reborn/state/session.json",
        &json!({"schema": 2, "model": {"queue": [
            {"title": "Ready", "artist": "1.0", "album": "2001"},
            {"title": "Secret Song", "artist": "Ready Band"}]}}),
    );
    fixture.put(
        "/data/network/wpa_supplicant.conf",
        "network={\n ssid=\"1234\"\n psk=\"Ready\"\n}\n",
    );
    let found = secrets(&fixture.root).unwrap();
    assert_eq!(class(&found, "music"), set(&["Secret Song", "Ready Band"]));
    assert_eq!(class(&found, "ssid"), set(&["1234"]));
    assert_eq!(class(&found, "wifi_secret"), set(&["Ready"]));
    let text = Scrubber::new(&found, *b"0123456789abcdef")
        .unwrap()
        .text("Y2LINUX-1.0.0-BETA-8.22 outcome Ready, playing Secret Song");
    assert!(text.contains("Y2LINUX-1.0.0-BETA-8.22"));
    assert!(!text.contains("Secret Song"));
}
