//! Error reports through the command line (tests/test_error_reports.py: Report): the report's live
//! commands (dmesg, rebornctl) answered with secrets as the player's tools would, which only a run of
//! the binary can give them (a report runs them on their own thread).
// SPDX-License-Identifier: GPL-2.0-only
mod common;
#[path = "../src/commands/reports/test_player.rs"]
mod test_player;

use common::Fixture;
use serde_json::{json, Value};
use test_player as player;

const QUEUE: &str = "/data/system/platform/reports";

/// The synthetic player, its tools answering as the Python tests' `runner`.
fn setup() -> Fixture {
    let mut fixture = Fixture::new();
    player::player(&fixture.root);
    for (argv, output) in player::answers() {
        fixture.answer(&argv, true, &output);
    }
    fixture
}

fn text(members: &std::collections::BTreeMap<String, Vec<u8>>, name: &str) -> String {
    String::from_utf8_lossy(members.get(name).unwrap_or_else(|| panic!("{name}"))).into_owned()
}

fn json_of(members: &std::collections::BTreeMap<String, Vec<u8>>, name: &str) -> Value {
    serde_json::from_str(&text(members, name)).unwrap()
}

#[test]
fn no_planted_secret_reaches_the_report() {
    let fixture = setup();
    player::mount(&fixture.root, &["/media/sd"], "rw,relatime");
    let run = fixture.run(&["report", "save", "--to", "sd", "--topic", "sound"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    let result = run.json();
    assert_eq!(result["saved"], true);
    let data = std::fs::read(
        fixture
            .path("/media/sd")
            .join(result["file"].as_str().unwrap()),
    )
    .unwrap();
    let everything = player::members(&data);
    player::assert_no_canary(&everything);
    let index = json_of(&everything, "report.json");
    let scrubbed = &index["scrubbed"];
    // system.log, Reborn's log and dmesg (a detail naming the network adds a fourth: the unit test
    // of build gives one; `report save` has none).
    assert!(scrubbed["ssid"].as_i64().unwrap_or(0) >= 3, "{scrubbed}");
    for name in [
        "mac",
        "ip",
        "music",
        "serial",
        "media_path",
        "private_key",
        "bt_name",
        "email",
    ] {
        assert!(scrubbed.get(name).is_some(), "{name} in {scrubbed}");
    }
    // The commands ran and were scrubbed, not left out.
    assert!(text(&everything, "commands/dmesg.txt").contains("<ssid#"));
}

#[test]
fn the_evidence_survives_scrubbing() {
    let fixture = setup();
    // Opted in: the previous boot's kernel crash (pstore) and Reborn's panic fire in one report.
    fixture.put_json(
        "/data/system/platform/reports.json",
        &json!({"schema": 1, "enabled": true, "install_id": "install-1", "daily": {},
            "boot_triggers": {}, "seen": {}}),
    );
    let run = fixture.run(&["report", "check"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    assert_eq!(run.json()["reports"][0]["queued"], true, "{}", run.stdout);
    let queued: Vec<_> = std::fs::read_dir(fixture.path(QUEUE))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".tar.gz"))
        .collect();
    let [path] = &queued[..] else {
        panic!("{queued:?}")
    };
    let everything = player::members(&std::fs::read(path).unwrap());
    assert!(text(&everything, "files/sys/fs/pstore/console-ramoops-0")
        .contains("Kernel panic - not syncing"));
    let log = text(&everything, "files/data/reborn/logs/reborn-current.jsonl");
    assert!(log.contains("\"prepared\""));
    assert!(log.contains("14:14:7D:<mac#")); // maker kept, device masked
    assert!(player::plain(&log).contains("<ip:private#>"));
    assert!(log.contains("10.42.0.1")); // the USB network stays
    assert!(log.contains("/media/internal/<path>.flac: EIO")); // text after the file stays
    assert!(log.contains("https://ws.audioscrobbler.com/<path>"));
    let system = text(&everything, "files/data/logs/system.log");
    assert!(system.contains("3C:22:FB:<mac#"));
    assert!(system.contains("dev_14_14_7D_<mac#"));
    assert!(system.contains("reborn_audio::native ok at 20:39:14, kernel 6.18.56"));
    assert!(system.contains("Y2Linux stays Y2Linux")); // a key comment, not a secret
    let session = json_of(&everything, "reborn/session.json");
    assert_eq!(session["settings"]["volume"], 47);
    assert_eq!(session["settings"]["time_zone"], "set");
    assert_eq!(session["queue_length"], 3001);
    assert_eq!(
        session["current_track"],
        json!({"codec": "flac", "sample_rate": 96000, "duration_ms": 1000})
    );
    let library = json_of(&everything, "reborn/library.json");
    assert_eq!(library["tables"], json!({"tracks": 1}));
    assert_eq!(library["by_codec"], json!([["flac", 1]]));
    let hardware = json_of(&everything, "hardware.json");
    assert_eq!(hardware["storage"]["mmc0"]["name"], "FNX2MB");
    assert!(hardware["storage"]["mmc0"].get("cid").is_none());
    assert!(hardware["y2_boot"]["lcm"]
        .as_str()
        .unwrap()
        .contains("lcm=0-st7701"));
    assert!(text(&everything, "commands/reborn_status.txt").contains("bluetooth"));
    assert!(system.contains("'<ssid#"));
    let index = json_of(&everything, "report.json");
    assert_eq!(index["trigger"], "kernel_crash");
    assert_eq!(index["install_id"], "install-1");
    // Never read: library contents, account files, keys, the raw session.
    let never = regex::Regex::new(
        r"library\.db|lastfm|wpa_supplicant|authorized_keys|state/session\.json|bluez",
    )
    .unwrap();
    for name in everything.keys() {
        assert!(!never.is_match(name), "{name}");
    }
}
