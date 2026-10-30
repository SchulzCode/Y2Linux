//! What the storage observation reports about controllers and card identity (from
//! tests/test_platform_storage_tools.py and test_storage_ceiling.py).
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::commands::observation::snapshot;
use crate::testing::Fixture;

/// The storage observation's controllers, every command answered `fixture`.
fn controllers(fixture: &Fixture) -> Value {
    serde_json::json!(snapshot::storage(&fixture.root).unwrap())["controllers"].clone()
}

/// A fixture whose root answers no command.
fn quiet() -> Fixture {
    Fixture::new().with_answers(vec![])
}

#[test]
fn zero_uuid_whole_card_uses_cid_and_zero_start() {
    let fixture = Fixture::new();
    let disk = "/sys/devices/platform/11240000.mmc/block/mmcblk1";
    fixture.put(&format!("{disk}/device/cid"), "whole-card-cid");
    fixture.put(&format!("{disk}/size"), "1000000");
    let identity = media_identity(&fixture.path(disk), "vfat", None).unwrap();
    assert_eq!(identity["geometry"]["start"], "0");
    assert!(identity["geometry"]["partition"].is_null());
    assert_eq!(identity["cid"], "whole-card-cid");
}

#[test]
fn status_exposes_passive_runtime_pm_and_tuning() {
    let fixture = quiet();
    fixture.mkdir("/data");
    fixture.put("/proc/sys/kernel/random/boot_id", "boot-one");
    let base = "/sys/bus/platform/devices/11240000.mmc";
    fixture.put(
        &format!("{base}/y2_performance"),
        "cap_hz=100000000 actual_hz=99999885 transport_errors=0",
    );
    fixture.put(
        &format!("{base}/y2_storage"),
        "level=SDR50 width=4 signal_mv=1800 crc_events=0",
    );
    fixture.put(
        &format!("{base}/y2_runtime_pm"),
        "gated=1 retained=37 last_mismatch=0x0 error=0",
    );
    fixture.put(&format!("{base}/power/runtime_status"), "suspended");
    fixture.put(&format!("{base}/y2_tune_diag"), "tune.taps=32 tune.runs=1");
    // The diagnostic attribute resumes hardware; status must not read it.
    fixture.put(&format!("{base}/y2_storage_diag"), "DO_NOT_READ");
    let all = controllers(&fixture);
    let value = &all[0];
    assert_eq!(value["runtime_pm"]["gated"], 1);
    assert_eq!(value["runtime_pm"]["last_mismatch"], 0);
    assert_eq!(value["runtime_status"], "suspended");
    assert!(value["tuning"].as_str().unwrap().contains("tune.taps=32"));
    assert!(!all.to_string().contains("DO_NOT_READ"));
}

#[test]
fn platform_status_reports_the_negotiated_mode_without_register_decoding() {
    let fixture = quiet();
    let base = "/sys/bus/platform/devices";
    for (name, mode) in [
        (
            "11230000.mmc",
            Some(
                "host=emmc level=HS200 ceiling=HS200 timing=hs200 width=8 clock_hz=200000000 \
                 actual_hz=200000000 cap_hz=200000000 signal_mv=1800 vqmmc_mv=1800 card_level=HS200 \
                 card_caps=0x57 hs400_card=1 tuning=pass tune_pad=0x80000 verify=pass fallbacks=0 \
                 crc_events=0 timeout_events=2 tuning_events=0 verify_events=0\n",
            ),
        ),
        ("11240000.mmc", None),
    ] {
        fixture.put(
            &format!("{base}/{name}/y2_performance"),
            "cap_hz=200000000 actual_hz=200000000 transport_errors=0 fallbacks=0 clock_error=0\n",
        );
        if let Some(mode) = mode {
            fixture.put(&format!("{base}/{name}/y2_storage"), mode);
        }
    }
    let all = controllers(&fixture);
    let named = |name: &str| {
        all.as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap()
            .clone()
    };
    let emmc = named("11230000.mmc")["mode"].clone();
    assert_eq!(emmc["level"], "HS200");
    assert_eq!(emmc["width"], 8);
    assert_eq!(emmc["signal_mv"], 1800);
    assert_eq!(emmc["card_caps"], 0x57);
    assert_eq!(emmc["tune_pad"], 0x80000);
    assert_eq!(emmc["verify"], "pass");
    assert_eq!(emmc["tuning"], "pass"); // result, not the event count
    assert_eq!(emmc["timeout_events"], 2);
    let sd = named("11240000.mmc");
    assert!(sd["mode"].is_null()); // older kernel
    assert_eq!(sd["cap_hz"], 200_000_000);
}
