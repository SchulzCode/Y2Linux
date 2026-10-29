//! The boot journal keeps failures and refuses unverified storage.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::Fixture;
use std::fs;

#[test]
fn boot_history_preserves_previous_failure_and_bounds_retention() {
    let next = |record: Option<Value>, boot: &str, stage, now| {
        let record = transition(record, boot, stage, json!({}), now, reset_cause(None));
        serde_json::to_value(record).unwrap()
    };
    let mut record = None;
    for n in 0..15 {
        record = Some(next(
            record,
            &format!("boot-{n}"),
            BootStage::PlatformStart,
            n,
        ));
    }
    let record = record.unwrap();
    assert_eq!(record["boot_count"], 15);
    assert_eq!(record["previous_boot_id"], "boot-13");
    assert_eq!(record["history"].as_array().unwrap().len(), 8);
    assert_eq!(record["recovery_recommended"], true);
    assert!(record["reset_cause"].is_null());
    let record = next(Some(record), "boot-14", BootStage::ShutdownComplete, 16);
    let record = next(Some(record), "boot-15", BootStage::PlatformStart, 17);
    assert_eq!(record["consecutive_unclean"], 0);
    let history = record["history"].as_array().unwrap();
    assert_eq!(history.last().unwrap()["orderly_shutdown"], true);
}

#[test]
fn boot_journal_requires_verified_data_and_rejects_symlink() {
    let fixture = Fixture::new();
    fixture
        .put("/proc/sys/kernel/random/boot_id", "boot-a")
        .put("/data/.y2data-schema", "1");
    let error = mark(&fixture.root, "platform_start").err().unwrap();
    assert!(matches!(error, Failure::Value(_)));
    assert!(error.message().contains("mount"), "{}", error.message());
    fixture.put("/run/y2-data-device", "/dev/mmcblk0p7").put(
        "/proc/self/mountinfo",
        "33 1 179:7 / /data rw - ext4 /dev/mmcblk0p7 rw",
    );
    mark(&fixture.root, "platform_start").unwrap();
    assert_eq!(
        fixture.json("/data/system/platform/boot.json")["boot_count"],
        1
    );
    mark(&fixture.root, "services_started").unwrap();
    assert_eq!(
        fixture.json("/data/system/platform/boot.json")["boot_count"],
        1
    );
    assert_eq!(
        fixture.mode("/data/system/platform/boot.json") & 0o777,
        0o600
    );
    let directory = fixture.path("/data/system/platform");
    fs::remove_dir_all(&directory).unwrap();
    std::os::unix::fs::symlink(fixture.base(), &directory).unwrap();
    let error = mark(&fixture.root, "platform_start").err().unwrap();
    assert!(matches!(error, Failure::Value(_)));
    assert!(error.message().contains("directory"), "{}", error.message());
}
