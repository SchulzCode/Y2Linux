//! boot.py's previous-boot evidence and boot_retained.py's archive tests
//! (tests/test_platform_evidence.py `EvidenceTests`, tests/test_boot_retained.py `Retained`), on a
//! fixture root with a verified /data.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::commands::{boot::mark, observation::system::evidence_status};
use crate::testing::Fixture;
use serde_json::json;

const EVIDENCE: &str = "/data/system/platform/previous-boot-evidence.json";

/// The verified /data `boot-evidence` needs, on boot `new-boot`.
fn data_fixture() -> Fixture {
    let fixture = Fixture::new();
    fixture
        .put("/data/.y2data-schema", "1")
        .put(
            "/proc/self/mountinfo",
            "3 1 179:2 / /data rw - ext4 /dev/data rw\n",
        )
        .put("/run/y2-data-device", "/dev/data")
        .put("/proc/sys/kernel/random/boot_id", "new-boot");
    fixture
}

/// `capture_previous(ctx)`: the record `boot-evidence` prints.
fn capture_previous(root: &Root) -> Result<Value, Failure> {
    run(root).map(|reply| serde_json::from_str(&reply.stdout).unwrap())
}

fn status(strup: u32) -> String {
    format!(
        "valid=1 strup_con8=0x{strup:04x} top_rst_misc=0x015b chr_con16=0x0005 \
         strup_con5=0x0000 chrstatus=0x0000\n"
    )
}

#[test]
fn previous_boot_logs_are_private_bounded_once_only_and_not_reset_diagnosis() {
    let fixture = data_fixture();
    let root = &fixture.root;
    fixture.put_json(
        "/data/system/platform/boot.json",
        &json!({"schema": 1, "boot_id": "previous", "last_stage": "application_ready",
                "orderly_shutdown": false}),
    );
    let line = format!("private previous message {}\n", "x".repeat(600));
    fixture.put("/data/logs/system.log", &line.repeat(1000));
    let result = capture_previous(root).unwrap();
    assert_eq!(result["previous_boot_id"], "previous");
    assert!(result["reset_cause"].is_null());
    assert_eq!(
        result["loader"],
        json!({"boot_reason": null, "reset": null})
    );
    assert_eq!(result["kernel_panic_retention_guaranteed"], false);
    assert!(result["logs"][0]["tail_lines"].as_array().unwrap().len() <= 32);
    assert_eq!(fixture.mode(EVIDENCE), 0o600);
    assert!(fs::metadata(fixture.path(EVIDENCE)).unwrap().len() <= 96_000);
    let shown = json!(evidence_status(root)).to_string();
    assert!(!shown.contains("private previous message"));
    fixture.put("/data/logs/system.log", "new boot message");
    assert_eq!(capture_previous(root).unwrap(), result);
    mark(root, "platform_start").unwrap();
    fs::remove_file(fixture.path(EVIDENCE)).unwrap();
    let missed = capture_previous(root).unwrap_err().message();
    assert!(missed.contains("missed_early_boundary"), "{missed}");
    fixture.put("/proc/sys/kernel/random/boot_id", "next-boot");
    let log = fixture.path("/data/logs/system.log");
    fs::remove_file(&log).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", &log).unwrap();
    assert!(matches!(capture_previous(root), Err(Failure::Os { .. })));
}

#[test]
fn pmic_long_press_record_is_decoded_only_when_it_changed() {
    // F56/F57: LK leaves STRUP_CON8 bit 4 set after clearing a long-press reset; a set bit proves a
    // new one only after a boot that had it clear.
    let fresh = json!(pmic_power_status(Some(&status(0x8010)), Some(false)));
    assert_eq!(
        (
            &fresh["long_press_reset"],
            &fresh["long_press_reset_reason"]
        ),
        (&json!(true), &json!("lk_cleared_long_press_reset"))
    );
    assert_eq!(
        (
            &fresh["long_press_reset_enabled"],
            &fresh["long_press_reset_seconds"]
        ),
        (&json!(true), &json!(11))
    );
    assert_eq!(
        (
            &fresh["uvlo_selector"],
            &fresh["thermal_status"],
            &fresh["power_key_down"]
        ),
        (&json!(1), &json!(0), &json!(true))
    );
    assert_eq!(fresh["raw"]["strup_con8"], "0x8010");
    assert_eq!(
        json!(pmic_power_status(Some(&status(0x8010)), Some(true)))["long_press_reset_reason"],
        "record_set_before"
    );
    assert!(json!(pmic_power_status(Some(&status(0x8010)), None))["long_press_reset"].is_null());
    let clear = json!(pmic_power_status(Some(&status(0x8000)), Some(true)));
    assert_eq!(
        (&clear["long_press_reset"], &clear["lk_long_press_record"]),
        (&json!(false), &json!(false))
    );
    for text in [None, Some("valid=0\n"), Some("valid=1 strup_con8=error\n")] {
        assert_eq!(
            json!(pmic_power_status(text, Some(false)))["state"],
            "Unavailable"
        );
    }
    let fixture = data_fixture();
    let root = &fixture.root;
    fixture
        .put_json(
            "/data/system/platform/boot.json",
            &json!({"schema": 1, "boot_id": "previous", "orderly_shutdown": false}),
        )
        .put_json(
            EVIDENCE,
            &json!({"captured_on_boot_id": "previous",
                    "pmic": json!(pmic_power_status(Some(&status(0x8000)), None))}),
        )
        .put(PMIC_POWER_STATUS, &status(0x8010))
        .put(
            BOOT_METADATA,
            "valid=1 boot_mode=0 boot_reason=0 offline=0 recovery_image=0 panel=st7701s\n",
        );
    assert_eq!(
        capture_previous(root).unwrap()["pmic"]["long_press_reset"],
        true
    );
    let shown = json!(evidence_status(root));
    assert_eq!(shown["pmic"]["long_press_reset"], true);
    assert_eq!(shown["loader"], json!({"boot_reason": 0, "reset": "cold"}));
}

#[test]
fn loader_start_tells_a_watchdog_reset_from_a_cut() {
    // F56: the RGU status reads 0 by the time Linux probes; LK's boot reason survives. 4 = SoC reset
    // past the Power key (watchdog, restart).
    let meta = |reason: &str| {
        format!(
            "valid=1 boot_mode=0 boot_reason={reason} offline=0 recovery_image=0 panel=gc9503v\n"
        )
    };
    assert_eq!(
        json!(loader_boot(Some(&meta("4")))),
        json!({"boot_reason": 4, "reset": "warm"})
    );
    assert_eq!(
        json!(loader_boot(Some(&meta("3")))),
        json!({"boot_reason": 3, "reset": "warm"})
    );
    assert_eq!(
        json!(loader_boot(Some(&meta("1")))),
        json!({"boot_reason": 1, "reset": "cold"})
    );
    for text in [
        None,
        Some("valid=0 boot_reason=4\n".to_owned()),
        Some(meta("x")),
    ] {
        assert_eq!(
            json!(loader_boot(text.as_deref())),
            json!({"boot_reason": null, "reset": null})
        );
    }
}

// ---- boot_retained -------------------------------------------------------------------------------

const PANIC: &[u8] = b"Panic#1\n\xff\x00unaltered bytes\n";
const PSTORE: &str = "1 0 0:1 / /sys/fs/pstore rw - pstore pstore rw\n";

/// Retained.setUp: pstore mounted with one panic record, the PM driver's previous-stage record.
fn retained_fixture() -> Fixture {
    let fixture = Fixture::new();
    fixture
        .put("/proc/self/mountinfo", PSTORE)
        .put(
            "/sys/firmware/y2_pm/previous",
            "valid=1 stage=TASKS_THAWED\n",
        )
        .mkdir("/sys/fs/pstore");
    fs::write(fixture.path("/sys/fs/pstore/dmesg-ramoops-0"), PANIC).unwrap();
    fixture
}

fn errors(result: &Value) -> &Vec<Value> {
    result["errors"].as_array().unwrap()
}

#[test]
fn exact_bytes_private_archive_and_kernel_entries_preserved() {
    let fixture = retained_fixture();
    let result = json!(capture(&fixture.root, "one", Some("previous")).unwrap());
    assert_eq!(result["pstore_mounted"], true);
    assert!(errors(&result).is_empty());
    let path = fixture.base().join(result["archive"].as_str().unwrap());
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
    let mut entries = std::collections::BTreeMap::new();
    for entry in tar::Archive::new(File::open(&path).unwrap())
        .entries()
        .unwrap()
    {
        let mut entry = entry.unwrap();
        let name = entry.path().unwrap().to_string_lossy().into_owned();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        entries.insert(name, data);
    }
    assert_eq!(entries["pstore/dmesg-ramoops-0"], PANIC);
    let previous = String::from_utf8_lossy(&entries["pm/previous"]).into_owned();
    assert!(previous.contains("TASKS_THAWED"));
    let manifest: Value = serde_json::from_slice(&entries["manifest.json"]).unwrap();
    assert_eq!(manifest["previous_boot_id"], "previous");
    assert_eq!(
        fs::read(fixture.path("/sys/fs/pstore/dmesg-ramoops-0")).unwrap(),
        PANIC
    );
}

#[test]
fn unmounted_is_distinct_and_symlink_is_never_archived() {
    let fixture = retained_fixture();
    fixture.put("/proc/self/mountinfo", "");
    let result = json!(capture(&fixture.root, "one", None).unwrap());
    assert_eq!(result["pstore_mounted"], false);
    assert!(!result["files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["name"].as_str().unwrap().starts_with("pstore/")));
    fixture.put("/proc/self/mountinfo", PSTORE);
    let panic = fixture.path("/sys/fs/pstore/dmesg-ramoops-0");
    fs::remove_file(&panic).unwrap();
    std::os::unix::fs::symlink(fixture.path("/proc/self/mountinfo"), &panic).unwrap();
    let result = json!(capture(&fixture.root, "two", Some("one")).unwrap());
    assert_eq!(errors(&result)[0]["source"], "pstore/dmesg-ramoops-0");
}

/// Python lowered MAX_BYTES to 100 for a 500-byte record; here the record is one byte past the
/// real bound.
#[test]
fn bounds_are_explicit_and_only_old_private_archives_rotate() {
    let fixture = retained_fixture();
    let panic = fixture.path("/sys/fs/pstore/dmesg-ramoops-0");
    let large = vec![b'x'; MAX_BYTES + 1];
    fs::write(&panic, &large).unwrap();
    let result = json!(capture(&fixture.root, "zero", None).unwrap());
    assert_eq!(
        errors(&result)[0]["error"],
        "retained_source_exceeds_archive_bound"
    );
    let directory = fixture.path("/data/system/platform/retained");
    fs::write(directory.join("owner-evidence.tar"), b"keep").unwrap();
    for index in 0..10 {
        json!(capture(&fixture.root, &format!("test-{index}"), Some("previous")).unwrap());
    }
    let archives = fs::read_dir(&directory)
        .unwrap()
        .filter(|e| {
            let name = e.as_ref().unwrap().file_name();
            let name = name.to_string_lossy();
            name.starts_with("boot-") && name.ends_with(".tar")
        })
        .count();
    assert_eq!(archives, 8);
    assert_eq!(
        fs::read(directory.join("owner-evidence.tar")).unwrap(),
        b"keep"
    );
    assert_eq!(fs::read(&panic).unwrap(), large);
    assert_eq!(
        capture(&fixture.root, "../escape", None).unwrap_err(),
        "invalid_boot_archive_identity"
    );
}
