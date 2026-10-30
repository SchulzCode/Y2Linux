//! POWER-FINAL-14 persistent sleep and USB diagnostics, synthetic evidence only
//! (tests/test_sleep_diagnostics.py; the USB rows of the power sample are `power::journal`'s
//! `Diagnostics`), and the 32-bit DMA fields of POWER-FINAL-16 (tests/test_usb_dma_unmask.py).
// SPDX-License-Identifier: GPL-2.0-only
use super::{kernel_tail, record, usb_net, LIMIT, PATH};
use crate::commands::power::journal::usb::USB_SUMMARY;
use crate::commands::{
    power::journal::{Diagnostics, PowerJournal, FOLLOW_UP_S, PATH as JOURNAL},
    suspend::receipt::{self, Stage},
};
use crate::testing::Fixture;
use serde_json::{json, Value};
use y2p_core::{
    clock,
    json::dumps,
    json::{Object, ObjectExt},
};

const FIB_TRIE: &str = "Main:
  +-- 0.0.0.0/0 3 0 5
     +-- 10.42.0.0/24 2 0 2
        |-- 10.42.0.0
           /24 link UNICAST
        |-- 10.42.0.1
           /32 host LOCAL
     +-- 192.168.123.0/24 2 0 2
        |-- 192.168.123.136
           /32 host LOCAL
";
const ROUTE: &str =
    "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT
usb0\t00002A0A\t00000000\t0001\t0\t0\t0\t00FFFFFF\t0\t0\t0
wlan0\t007BA8C0\t00000000\t0001\t0\t0\t0\t00FFFFFF\t0\t0\t0
";
const KMSG: &str = "6,100,5000000,-;Y2USB PHY entry savecurrent=1
6,101,5000100,-;random: crng init done
6,102,5000200,-;y2-usb 11200000.usb: resume_noirq restored 11 bytes
 SUBSYSTEM=platform
6,103,5000300,-;mediatek-drm display: vblank
6,104,5000400,-;PM: suspend exit
";
const UDC: &str = "/sys/class/udc/musb-hdrc.4.auto/state";

/// `Diagnostics.setUp`: a verified data mount, 13's counters, a configured USB gadget with its
/// network up and umtprd running.
fn player() -> Fixture {
    let fixture = Fixture::new();
    for (name, text) in [
        ("/data/.y2data-schema", "1"),
        ("/run/y2-data-device", "/dev/data"),
        (
            "/proc/self/mountinfo",
            "5 1 179:7 / /data rw - ext4 /dev/data rw",
        ),
        ("/proc/sys/kernel/random/boot_id", "one"),
        (
            "/etc/y2linux/versions.json",
            r#"{"build_id": "Y2LINUX-PLATFORM-V1-POWER-FINAL-14"}"#,
        ),
        ("/sys/module/pwrap/parameters/vproc_sleep", "Y"),
        (
            "/sys/module/pwrap/parameters/vproc_sleep_state",
            "stage=restored entries=3 restores=3 failures=0",
        ),
        ("/sys/firmware/y2_pmic/lp_sel", "applied=1"),
        (
            "/sys/bus/platform/drivers/y2-usb/11200000.usb/phy_savecurrent",
            "enabled=1 saves=3 restores=3 mismatches=0",
        ),
        (
            "/sys/bus/platform/drivers/y2-usb/11200000.usb/status",
            "stage=6 error=0 configured=1 polls=9 detached=0\nirqs=5",
        ),
        (
            "/sys/bus/i2c/devices/0-0051/sleep",
            "enabled=1 sleeps=3 wakes=3 failures=0",
        ),
        (
            "/sys/firmware/y2_infra/state",
            "saves=3 restores=3 last_lost=0x1",
        ),
        (
            "/sys/devices/platform/10006000.power-controller/state",
            "entries=3",
        ),
        (
            "/sys/devices/platform/10006000.power-controller/golden",
            "captures=3 entry_differences=4",
        ),
        (UDC, "configured"),
        ("/sys/class/net/usb0/operstate", "up"),
        ("/sys/class/net/usb0/carrier", "1"),
        ("/sys/class/net/usb0/statistics/rx_packets", "0"),
        ("/sys/class/net/usb0/statistics/tx_packets", "7"),
        ("/proc/net/fib_trie", FIB_TRIE),
        ("/proc/net/route", ROUTE),
        ("/dev/kmsg", KMSG),
        ("/proc/41/comm", "umtprd"),
    ] {
        fixture.put(name, text);
    }
    fixture
}

fn rows(fixture: &Fixture) -> Vec<Value> {
    if !fixture.exists(PATH) {
        return Vec::new();
    }
    fixture
        .read(PATH)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn reasons(fixture: &Fixture) -> Vec<Value> {
    rows(fixture)
        .iter()
        .map(|r| r["usb_reason"].clone())
        .collect()
}

#[test]
fn resume_row_keeps_every_13_counter_golden_and_usb_state() {
    let fixture = player();
    assert_eq!(record(&fixture.root, "kernel_resumed"), Ok(true));
    let row = rows(&fixture).pop().unwrap();
    let features = &row["features"];
    assert_eq!(
        features["vproc_sleep_state"],
        "stage=restored entries=3 restores=3 failures=0"
    );
    assert_eq!(
        features["phy_savecurrent"]["11200000.usb"],
        "enabled=1 saves=3 restores=3 mismatches=0"
    );
    assert_eq!(
        features["wheel_sleep"]["0-0051"],
        "enabled=1 sleeps=3 wakes=3 failures=0"
    );
    assert_eq!(features["infra"], "saves=3 restores=3 last_lost=0x1");
    assert_eq!(features["golden"], "captures=3 entry_differences=4");
    let usb = &row["usb_net"];
    assert_eq!(usb["udc"], json!({"musb-hdrc.4.auto": "configured"}));
    assert_eq!(usb["controller"]["11200000.usb"]["configured"], "1");
    assert_eq!(usb["usb0"]["ipv4"], json!(["10.42.0.1"]));
    assert_eq!(usb["usb0"]["statistics"]["rx_packets"], 0);
    assert_eq!(usb["processes"], json!({"umtprd": true, "dropbear": false}));
    assert_eq!(row["build_id"], "Y2LINUX-PLATFORM-V1-POWER-FINAL-14");
    assert_eq!(fixture.mode(PATH) & 0o777, 0o600);
}

#[test]
fn golden_is_read_only_after_linux_returned() {
    let fixture = player();
    record(&fixture.root, "kernel_suspend").unwrap();
    assert!(rows(&fixture).pop().unwrap()["features"]
        .get("golden")
        .is_none());
}

#[test]
fn kernel_tail_keeps_usb_and_sleep_lines_only() {
    let fixture = player();
    let tail = kernel_tail(&fixture.root, 80, 240).unwrap();
    let messages: Vec<&str> = tail
        .iter()
        .map(|line| line.split_once(' ').unwrap().1)
        .collect();
    assert_eq!(
        messages,
        [
            "Y2USB PHY entry savecurrent=1",
            "y2-usb 11200000.usb: resume_noirq restored 11 bytes",
            "PM: suspend exit"
        ]
    );
}

#[test]
fn unverified_data_mount_creates_nothing() {
    let fixture = player();
    fixture.put("/run/y2-data-device", "/dev/other");
    assert_eq!(record(&fixture.root, "kernel_resumed"), Ok(false));
    assert!(!fixture.exists(PATH));
}

#[test]
fn write_failure_is_counted_never_raised() {
    let fixture = player();
    // The journal's own lock is busy: its append raises (Python's test made `_append` raise).
    fixture.mkdir("/data/system/platform");
    let _busy = y2p_core::lock::try_exclusive(
        &fixture.path("/data/system/platform/.sleep-diagnostics.lock"),
    )
    .unwrap()
    .unwrap();
    let mut diagnostics = Diagnostics::default();
    assert!(!diagnostics.record(
        &fixture.root,
        "kernel_suspend",
        clock::monotonic(),
        false,
        None
    ));
    assert_eq!(diagnostics.failures, 1);
}

#[test]
fn oversized_kernel_tail_is_trimmed_not_dropped() {
    // Python's test lowered ROW_LIMIT to 6000; here 80 kept lines of 240 four-byte characters
    // (UTF-8 in the row's JSON) pass the real 64 KiB limit.
    let fixture = player();
    let kmsg: String = (0..100)
        .map(|n| format!("6,{n},5000000,-;Y2USB {}\n", "\u{1f600}".repeat(300)))
        .collect();
    fixture.put("/dev/kmsg", &kmsg);
    assert_eq!(kernel_tail(&fixture.root, 80, 240).unwrap().len(), 80);
    assert_eq!(record(&fixture.root, "kernel_suspend"), Ok(true));
    let row = rows(&fixture).pop().unwrap();
    let kept = row["kernel_tail"].as_array().unwrap().len();
    assert!(0 < kept && kept < 80, "{kept}");
}

#[test]
fn usb_change_writes_a_row_and_one_follow_up() {
    let fixture = player();
    let mut diagnostics = Diagnostics::default();
    assert!(!diagnostics.observe_usb(&fixture.root, 0.0));
    fixture.put(UDC, "not attached");
    assert!(diagnostics.observe_usb(&fixture.root, 1.0));
    assert!(!diagnostics.observe_usb(&fixture.root, 2.0));
    assert!(diagnostics.observe_usb(&fixture.root, 31.0));
    assert!(!diagnostics.observe_usb(&fixture.root, 62.0));
    assert_eq!(reasons(&fixture), ["udc_change", "udc_follow_up"]);
}

#[test]
fn cable_flapping_is_rate_limited_and_still_followed_up() {
    let fixture = player();
    let mut diagnostics = Diagnostics::default();
    diagnostics.observe_usb(&fixture.root, 0.0);
    for (now, state) in ["not attached", "configured"]
        .repeat(4)
        .into_iter()
        .enumerate()
    {
        fixture.put(UDC, state);
        diagnostics.observe_usb(&fixture.root, (now + 1) as f64);
    }
    assert_eq!(rows(&fixture).len(), 1);
    diagnostics.observe_usb(&fixture.root, 8.0 + FOLLOW_UP_S);
    assert_eq!(reasons(&fixture), ["udc_change", "udc_follow_up"]);
}

#[test]
fn follow_up_close_to_another_row_is_postponed_not_lost() {
    let fixture = player();
    let mut diagnostics = Diagnostics::default();
    diagnostics.observe_usb(&fixture.root, 0.0);
    fixture.put(UDC, "not attached");
    diagnostics.observe_usb(&fixture.root, 1.0);
    diagnostics.record(&fixture.root, "kernel_suspend", 30.0, false, None);
    assert!(!diagnostics.observe_usb(&fixture.root, 31.0));
    assert!(diagnostics.observe_usb(&fixture.root, 40.0));
    assert_eq!(
        reasons(&fixture),
        [json!("udc_change"), Value::Null, json!("udc_follow_up")]
    );
}

#[test]
fn rotation_keeps_the_previous_file() {
    // Python's test lowered LIMIT to 3000 and wrote four rows; here the file is already at the
    // real limit.
    let fixture = player();
    fixture.put(PATH, &format!("{}\n", "x".repeat(LIMIT as usize - 1)));
    record(&fixture.root, "kernel_suspend").unwrap();
    assert!(fixture.exists("/data/system/platform/sleep-diagnostics.previous.jsonl"));
    assert_eq!(rows(&fixture).pop().unwrap()["stage"], "kernel_suspend");
}

#[test]
fn power_journal_rows_carry_the_usb_snapshot() {
    let fixture = player();
    let battery = Object::new().with("status", "Discharging");
    assert!(PowerJournal::new().observe(&fixture.root, "Normal", &battery, None, None));
    let line = fixture.read(JOURNAL).lines().last().unwrap().to_owned();
    let row: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(row["usb_net"]["usb0"]["statistics"]["tx_packets"], 7);
    assert!(dumps(&row).unwrap().len() < 16384);
}

#[test]
fn suspend_stages_write_rows_but_refusals_do_not() {
    let fixture = player();
    let stage = |name: &str, result: i64| {
        receipt::record(&fixture.root, Stage::new(name, Some(result))).unwrap();
    };
    stage("refused_charger_active", 2);
    assert!(rows(&fixture).is_empty());
    stage("quiescing_radios", 0);
    stage("kernel_suspend", 0);
    let stages: Vec<Value> = rows(&fixture).iter().map(|r| r["stage"].clone()).collect();
    assert_eq!(stages, ["kernel_suspend"]);
}

// ---- tests/test_usb_dma_unmask.py ---------------------------------------------------------------

#[test]
fn diagnostics_read_all_32_bits() {
    let fixture = player();
    fixture.put(
        "/sys/bus/platform/drivers/y2-usb/11200000.usb/status",
        "stage=6 dma_tx_programs=12 dma_irqs=11 dma_unmask_rearms=3 dma_unmask_lost=1 \
         dma_intr_before=0000fe00 dma_intr_after=ff00ff00",
    );
    let usb = serde_json::to_value(usb_net(&fixture.root).unwrap()).unwrap();
    let controller = &usb["controller"]["11200000.usb"];
    for (key, value) in [
        ("dma_unmask_lost", "1"),
        ("dma_intr_before", "0000fe00"),
        ("dma_tx_programs", "12"),
        ("dma_irqs", "11"),
    ] {
        assert!(USB_SUMMARY.contains(&key), "{key}");
        assert_eq!(controller[key], value, "{key}");
    }
}
