//! The automatic triggers (tests/test_error_reports.py: Report, Triggers): each fires once per new
//! event, an earlier release's or an already reported crash stays quiet, and one check makes one report.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::commands::reports::test_player as player;
use crate::testing::{ok, Fixture};
use serde_json::json;
use std::time::{Duration, UNIX_EPOCH};

/// The player's tools as the Python tests' `runner` answers them, dmesg with `dmesg` if given.
fn answers(dmesg: Option<&str>) -> Vec<(Vec<String>, command::Outcome)> {
    player::answers()
        .into_iter()
        .map(|(argv, output)| match (argv[0], dmesg) {
            ("dmesg", Some(text)) => ok(&argv, text),
            _ => ok(&argv, &output),
        })
        .collect()
}

/// This root with the player's tools answering (dmesg with `dmesg` if given).
fn answering(root: &Root, dmesg: Option<&str>) -> Root {
    root.clone().with_answers(answers(dmesg))
}

fn fired_with(root: &Root, dmesg: Option<&str>) -> Vec<(String, String)> {
    check(&answering(root, dmesg)).unwrap()
}

fn fired(root: &Root) -> Vec<(String, String)> {
    fired_with(root, None)
}

fn triggers(root: &Root) -> Vec<String> {
    fired(root).into_iter().map(|(t, _)| t).collect()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(t, d)| (t.to_string(), d.to_string()))
        .collect()
}

fn run(root: &Root) -> Py {
    run_checks(&answering(root, None)).unwrap()
}

fn enable(root: &Root) {
    state::set_enabled(&answering(root, None), true).unwrap();
}

/// `os.utime(path, (when, when))`.
fn touch(path: &std::path::Path, when: u64) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(when))
        .unwrap();
}

fn setup() -> Fixture {
    let fixture = Fixture::new();
    player::player(fixture.base());
    fixture
}

#[test]
fn triggers_fire_once_each() {
    let fixture = setup();
    let root = &fixture.root;
    let mut first = fired(root);
    first.sort();
    assert_eq!(
        first,
        pairs(&[("kernel_crash", "previous boot"), ("reborn_crash", "panic")])
    );
    assert!(fired(root).is_empty());
    fixture.put_json(
        "/data/system/platform/previous-boot-evidence.json",
        &json!({"previous_orderly_shutdown": false, "previous_boot_id": "boot-0",
            "reset_cause": null, "previous_last_stage": "ui",
            "loader": {"boot_reason": 4, "reset": "warm"},
            "pmic": {"long_press_reset_reason": "no_long_press_record"}}),
    );
    fixture.put_json(
        "/data/updates/state.json",
        &json!({"state": "RolledBack", "sequence": 6901}),
    );
    fixture.put("/proc/sys/kernel/tainted", "512\n");
    fixture.put("/sys/power/suspend_stats/fail", "2\n");
    fixture.put("/sys/fs/ext4/mmcblk0p8/errors_count", "1\n");
    fixture.put(
        "/sys/devices/platform/18070000.connectivity/status",
        "activated=1 powered=1 chip=6582 error=-110 transport_errors=0\n",
    );
    let found = fired(root);
    let mut names: Vec<&str> = found.iter().map(|(t, _)| t.as_str()).collect();
    names.sort();
    // the panic was reported above
    assert_eq!(
        names,
        [
            "kernel_warning",
            "radio_failure",
            "sleep_failure",
            "storage_error",
            "unclean_restart",
            "update_failure"
        ]
    );
    let detail = |name: &str| found.iter().find(|(t, _)| t == name).unwrap().1.clone();
    assert!(detail("unclean_restart").contains("loader warm(4)"));
    assert!(detail("unclean_restart").ends_with("pmic no_long_press_record"));
    assert_eq!(detail("radio_failure"), "connectivity error=-110");
    assert!(fired(root).is_empty());
    // A sleep a wake event cancelled (-EBUSY while suspending devices) is routine.
    fixture.put("/sys/power/suspend_stats/fail", "5\n");
    fixture.put("/sys/power/suspend_stats/last_failed_errno", "-16\n");
    fixture.put("/sys/power/suspend_stats/last_failed_step", "suspend\n");
    assert!(fired(root).is_empty());
    fixture.put("/sys/power/suspend_stats/failed_resume", "1\n");
    assert_eq!(triggers(root), ["sleep_failure"]);
    // Wi-Fi joining a network, or a wrong password / an absent network, is no radio failure.
    for error in [
        json!(null),
        json!("wrong_credentials"),
        json!("ap_unavailable"),
    ] {
        fixture.put_json(
            "/run/y2/wifi-failure.json",
            &json!({"error": error, "failures": 1}),
        );
        assert!(fired(root).is_empty());
    }
    assert_eq!(run(root), Py::List(Vec::new())); // not enabled: nothing queued
}

#[test]
fn a_kept_panic_record_is_not_sent_again() {
    // F50: 8e9181e5 re-sent fcdf3a25's 10-25 panic: the dmesg record stays in pstore while the
    // console and pmsg records change every start.
    let fixture = setup();
    let root = &fixture.root;
    fixture.remove("/sys/fs/pstore/console-ramoops-0");
    let dump = |name: &str, uptime: f64, when: u64| {
        let path = format!("/sys/fs/pstore/{name}");
        fixture.put(
            &path,
            &format!(
                "<0>[  {}] reboot: Power down\n<0>[  {uptime}] Kernel panic - not syncing: Attempted to kill init!\n",
                uptime - 2.0
            ),
        );
        touch(&fixture.path(&path), when);
    };
    dump("dmesg-ramoops-0", 769.016180, 1_761_390_000);
    fixture.put(
        "/sys/fs/pstore/console-ramoops-0",
        "[  769.016180] Kernel panic - not syncing\n",
    );
    assert!(fired(root).contains(&("kernel_crash".into(), "previous boot".into())));
    for start in 0..3 {
        fixture.put(
            "/sys/fs/pstore/console-ramoops-0",
            &format!("[    5.{start}] boot {start}\n"),
        );
        fixture.put(
            "/sys/fs/pstore/pmsg-ramoops-0",
            &format!("{start}.00 shutdown execute poweroff user\n"),
        );
        assert!(triggers(root).is_empty());
    }
    // A cold start flipped a digit of the kernel timestamp: the record time still names it.
    fixture.put(
        "/sys/fs/pstore/dmesg-ramoops-0",
        "<0>[  769.0161$0] Kernel panic - not syncing\n",
    );
    touch(
        &fixture.path("/sys/fs/pstore/dmesg-ramoops-0"),
        1_761_390_000,
    );
    assert!(triggers(root).is_empty());
    // A new panic in another record is news, even beside the kept one.
    dump("dmesg-ramoops-1", 310.695269, 1_761_480_000);
    assert_eq!(triggers(root), ["kernel_crash"]);
    assert!(triggers(root).is_empty());
    // Updated from a release that keyed on the hash of all records: a crash it saw is not resent.
    let mut value = state::state(root);
    let seen = value.get_mut("seen").unwrap();
    seen.remove("pstore_crash");
    seen.set("pstore", Py::str("old-hash"));
    state::save(root, &value).unwrap();
    assert!(triggers(root).is_empty());
    dump("dmesg-ramoops-2", 12.5, 1_761_490_000);
    assert_eq!(triggers(root), ["kernel_crash"]);
}

fn reports(result: &Py) -> Vec<Py> {
    match result {
        Py::List(items) => items.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn opting_in_reports_only_what_happens_afterwards() {
    let fixture = setup();
    let root = &fixture.root;
    enable(root); // old pstore and panic records exist
    assert_eq!(run(root), Py::List(Vec::new()));
    fixture.put("/data/reborn/logs/panic-last.json", r#"{"message": "new"}"#);
    touch(
        &fixture.path("/data/reborn/logs/panic-last.json"),
        2_000_000_000,
    );
    let result: Vec<(Py, Py)> = reports(&run(root))
        .iter()
        .map(|r| {
            (
                r.get("trigger").cloned().unwrap(),
                r.get("queued").cloned().unwrap(),
            )
        })
        .collect();
    assert_eq!(result, [(Py::str("reborn_crash"), Py::Bool(true))]);
}

#[test]
fn one_check_sends_one_report_for_all_its_triggers() {
    // a8125bf1 (8.10): unclean restart, Reborn crash and rolled-back update within seconds went out
    // as three near copies against the daily limit.
    let fixture = setup();
    let root = &fixture.root;
    enable(root);
    fixture.put_json(
        "/data/system/platform/previous-boot-evidence.json",
        &json!({"previous_orderly_shutdown": false, "previous_boot_id": "boot-0",
            "reset_cause": "None", "previous_last_stage": "application_ready"}),
    );
    fixture.put_json(
        "/data/reborn/logs/panic-last.json",
        &json!({"version": "8.10", "location": "playlists.rs:223", "message": "not a char boundary"}),
    );
    fixture.put_json(
        "/data/updates/state.json",
        &json!({"state": "RolledBack", "sequence": 7500}),
    );
    let result = reports(&run(root));
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].get("queued"), Some(&Py::Bool(true)));
    let expected = Py::List(
        ["unclean_restart", "reborn_crash", "update_failure"]
            .iter()
            .map(|t| Py::str(*t))
            .collect(),
    );
    assert_eq!(result[0].get("triggers"), Some(&expected));
    let queued = state::queued(root);
    assert_eq!(queued.len(), 1);
    let members = player::members(&std::fs::read(&queued[0]).unwrap());
    let index = pyjson::loads_bytes(&members["report.json"]).unwrap();
    assert_eq!(
        (index.get("trigger"), index.get("triggers")),
        (Some(&Py::str("unclean_restart")), Some(&expected))
    );
    assert!(index
        .get("detail")
        .and_then(Py::as_str)
        .unwrap()
        .contains("reborn_crash: panic"));
    assert_eq!(run(root), Py::List(Vec::new()));
}

#[test]
fn a_crash_loop_reports_its_panic_once() {
    let fixture = setup();
    let root = &fixture.root;
    enable(root);
    let panic = "/data/reborn/logs/panic-last.json";
    let record = |extra: serde_json::Value| {
        let mut value = json!({"version": "8.11", "location": "playlists.rs:223",
            "message": "not a char boundary"});
        for (k, v) in extra.as_object().unwrap() {
            value[k] = v.clone();
        }
        value
    };
    let mut found = Vec::new();
    for second in 0..5u64 {
        // rewritten at every restart
        fixture.put_json(panic, &record(json!({"session": second.to_string()})));
        touch(&fixture.path(panic), 2_000_000_000 + second);
        found.extend(triggers(root));
    }
    assert_eq!(found, ["reborn_crash"]);
    fixture.put_json(panic, &record(json!({"location": "other.rs:1"})));
    assert_eq!(fired(root), pairs(&[("reborn_crash", "panic")]));
    // A worker panic Reborn caught (one file skipped, still running) is reported, named apart.
    fixture.put_json(
        panic,
        &record(json!({"location": "worker.rs:9", "isolated": true})),
    );
    assert_eq!(fired(root), pairs(&[("reborn_crash", "isolated panic")]));
    fixture.put_json(
        panic,
        &record(json!({"location": "worker.rs:9", "isolated": true, "session": "next"})),
    );
    assert!(fired(root).is_empty());
}

#[test]
fn an_earlier_release_panic_is_not_reported_again_after_an_update() {
    // 19a41489, d48362a7: a player updated past 8.12 resent its old 8.10 #49 panic once on the new
    // release. Still recorded, still in a report's files.
    let fixture = setup();
    let root = &fixture.root;
    enable(root);
    // The image carries the version in versions.json (no release.json on a player).
    fixture.put_json(
        "/etc/y2linux/versions.json",
        &json!({"reborn_version": "1.0.0-beta.8.17"}),
    );
    let old = json!({"version": "1.0.0-beta.8.10-product-ui-v3", "location": "playlists.rs:223",
        "message": "not a char boundary"});
    fixture.put_json("/data/reborn/logs/panic-last.json", &old);
    assert!(fired(root).is_empty());
    assert!(fired(root).is_empty());
    // The running release's own panic (Reborn adds its build suffix) is reported.
    let mut own = old.clone();
    own["version"] = json!("1.0.0-beta.8.17-product-ui-v3");
    fixture.put_json("/data/reborn/logs/panic-last.json", &own);
    assert_eq!(fired(root), pairs(&[("reborn_crash", "panic")]));
}

#[test]
fn a_boot_time_dsi_lag_counts_only_when_no_frame_reached_the_screen() {
    let fixture = setup();
    let root = &fixture.root;
    let lag = "[    1.011318] mtk-dsi 1400c000.dsi: modeset: DSI register reads still lag after 3 power cycles\n";
    fixture.put_json(
        "/run/y2/application-ready.json",
        &json!({"boot_id": "boot-1", "first_frame": true}),
    );
    let found: Vec<String> = fired_with(root, Some(lag))
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    assert!(!found.contains(&"display_failure".to_owned())); // the panel lit: worked around
    assert!(fired_with(root, Some(lag)).is_empty());
    fixture.put_json(
        "/run/y2/application-ready.json",
        &json!({"boot_id": "boot-0", "first_frame": true}),
    );
    let found: Vec<String> = fired_with(root, Some(lag))
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    assert_eq!(found, ["display_failure"]); // no frame this boot
    fixture.put("/proc/sys/kernel/random/boot_id", "boot-2\n");
    fixture.put_json(
        "/run/y2/application-ready.json",
        &json!({"boot_id": "boot-2", "first_frame": true}),
    );
    let dmesg = format!("{lag}[  900.0] [drm] flip_done timed out\n");
    assert_eq!(
        fired_with(root, Some(&dmesg)),
        pairs(&[("display_failure", "flip_done timed out")])
    );
}

#[test]
fn storage_errors_are_the_internal_emmcs_not_a_pulled_card() {
    for line in [
        "I/O error, dev mmcblk0, sector 4096 op 0x0:(READ)",
        "EXT4-fs error (device mmcblk0p8): x",
        "EXT4-fs (mmcblk0p8): Remounting filesystem read-only",
    ] {
        assert!(storage().is_match(line), "{line}");
    }
    for line in [
        "I/O error, dev mmcblk1, sector 0 op 0x0:(READ)",
        "mmc1: card 59b4 removed",
        "FAT-fs (mmcblk1p1): Filesystem has been set read-only",
    ] {
        assert!(!storage().is_match(line), "{line}");
    }
}
