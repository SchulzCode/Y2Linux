//! Health and readiness never turn corrupt records or missing devices green (were
//! tests/test_platform_contract.py).
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::{scripted, Fixture};

/// Every command of this thread answered `fixture`, never from the process environment (as in
/// snapshot_tests.rs).
fn no_answers<T>(f: impl FnOnce() -> T) -> T {
    scripted(vec![], f).0
}

fn contract() -> Fixture {
    let fixture = Fixture::new();
    fixture
        .put("/proc/sys/kernel/random/boot_id", "boot-a")
        .put("/proc/sys/kernel/osrelease", "fixture-kernel")
        .put_json(
            "/etc/y2linux/versions.json",
            &json!({"build_git_commit": "commit", "rootfs_version": "release"}),
        );
    fixture
}

fn audio(fixture: &Fixture) -> Value {
    no_answers(|| snapshot(&fixture.root, &[], false, 0.0).unwrap())["readiness"]["audio"]["state"]
        .clone()
}

fn filesystem_errors(fixture: &Fixture) -> Value {
    let health = no_answers(|| check(&fixture.root, false).unwrap());
    health["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "filesystem_errors")
        .unwrap()["state"]
        .clone()
}

#[test]
fn record_corruption_and_no_devices_are_not_green() {
    let fixture = contract();
    fixture
        .put("/etc/y2linux/versions.json", "[]")
        .put("/data/updates/state.json", "{}")
        .put("/proc/asound/cards", "--- no soundcards ---")
        .put("/etc/y2linux/audio-qualified.json", "{}");
    let result = no_answers(|| snapshot(&fixture.root, &[], false, 0.0).unwrap());
    assert_eq!(result["readiness"]["audio"]["state"], "Unavailable");
    assert!(result["record"]["y2linux_commit"].is_null());
    let health = no_answers(|| check(&fixture.root, false).unwrap());
    assert_ne!(health["state"], "OK");
    assert_eq!(filesystem_errors(&fixture), "UNAVAILABLE");
}

#[test]
fn unreadable_filesystem_counter_and_invalid_audio_profile_are_not_ready() {
    let fixture = contract();
    fixture
        .put(
            "/sys/fs/ext4/fixture/errors_count",
            "not a readable counter",
        )
        .put("/proc/asound/cards", " 0 [Y2Audio ]: fixture")
        .put(
            "/etc/y2linux/audio-qualified.json",
            r#"{"note":"a nonempty corrupt profile"}"#,
        );
    assert_eq!(audio(&fixture), "Unavailable");
    assert_eq!(filesystem_errors(&fixture), "UNAVAILABLE");
    fixture.put("/sys/fs/ext4/fixture/errors_count", "1");
    assert_eq!(filesystem_errors(&fixture), "FAILED");
    let profile = json!({"schema": "org.y2linux.audio-qualification/v1", "card": "Y2Audio",
                         "channels": 2, "qualified_formats": ["S16_LE"],
                         "qualified_rates": [44100]});
    fixture.put_json("/etc/y2linux/audio-qualified.json", &profile);
    assert_eq!(audio(&fixture), "Ready");
    fixture.put("/proc/asound/cards", " 0 [OtherCard ]: unrelated card");
    assert_eq!(audio(&fixture), "Unavailable");
}
