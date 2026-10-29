//! The service daemon's connectivity and clock work: the coexistence record the Wi-Fi power policy
//! takes as activity (radio_policy.Coexistence) and the hourly time floor (timekeeping.remember), with
//! `time-remember` (every=0). What tests/test_radio_policy.py and tests/test_platform_network_time.py
//! checked of the Python platform. The clocks are the real ones: the anchor is written as it would
//! have been that long ago.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::Fixture;
use y2p_core::clock;

#[test]
fn heavy_transfer_is_observed_without_a_codec_override() {
    let fixture = Fixture::new();
    fixture.mkdir("/run/y2");
    let mut observe = Coexistence::default();
    let online = Object::new().with("state", "Online");
    let first = observe.observe(&fixture.root, &online, 1.0).unwrap();
    assert!(first["wifi_bytes_per_second"].is_null());
    let busy = online.with("traffic_counters", json!({"rx_bytes": 3000000}));
    let value = observe.observe(&fixture.root, &busy, 2.0).unwrap();
    assert_eq!(value["wifi_heavy_transfer"], true);
    assert!(value["codec_bitrate_override"].is_null());
}

/// The clock as the owner set it `ago` seconds of wall time earlier, `awake` of them awake, `asleep`
/// of them in CLOCK_BOOTTIME (time.json as set_manual writes it).
fn anchor(fixture: &Fixture, ago: f64, awake: f64, asleep: f64) {
    fixture.put_json(
        "/run/y2/time.json",
        &json!({"schema": 1, "boot_id": "boot-one", "epoch": time::wall() - ago,
                "monotonic_s": clock::monotonic() - awake, "boottime_s": time::boottime() - asleep,
                "source": "manual"}),
    );
}

/// The durable floor as written `ago` seconds earlier.
fn remembered(fixture: &Fixture, ago: f64) {
    fixture.put_json(
        time::LAST,
        &json!({"epoch": time::wall() - ago, "source": "manual"}),
    );
}

fn last(fixture: &Fixture) -> Option<f64> {
    fixture
        .exists(time::LAST)
        .then(|| fixture.json(time::LAST)["epoch"].as_f64().unwrap())
}

/// Whether the durable floor now reads the running clock.
fn current(fixture: &Fixture) -> bool {
    last(fixture).is_some_and(|epoch| (time::wall() - epoch).abs() < 5.0)
}

#[test]
fn running_clock_is_remembered_hourly_only_when_set() {
    let fixture = Fixture::new();
    fixture
        .put("/proc/sys/kernel/random/boot_id", "boot-one")
        .put(
            "/etc/y2linux/build-epoch",
            &(time::wall() as i64 - 86_400).to_string(),
        );
    // The build date alone is never remembered as a time.
    fixture.put_json(
        "/run/y2/time.json",
        &json!({"schema": 1, "boot_id": "boot-one", "epoch": time::wall(),
                "monotonic_s": clock::monotonic(), "boottime_s": time::boottime(),
                "source": "build_floor"}),
    );
    remember_time(&fixture.root).unwrap();
    assert_eq!(last(&fixture), None);
    // Set by hand 1800 s ago (and remembered then): not yet again.
    anchor(&fixture, 1800.0, 1800.0, 1800.0);
    remembered(&fixture, 1800.0);
    remember_time(&fixture.root).unwrap();
    assert!(!current(&fixture));
    // 3700 s ago: written, then not again within the hour.
    anchor(&fixture, 3700.0, 3700.0, 3700.0);
    remembered(&fixture, 3700.0);
    remember_time(&fixture.root).unwrap();
    assert!(current(&fixture));
    assert_eq!(fixture.json(time::LAST)["source"], "manual");
    let written = fixture.read(time::LAST);
    remember_time(&fixture.root).unwrap();
    assert_eq!(fixture.read(time::LAST), written);
    // Asleep for two hours: the wall clock and CLOCK_BOOTTIME both ran on.
    anchor(&fixture, 11_000.0, 3900.0, 11_000.0);
    remembered(&fixture, 3800.0);
    remember_time(&fixture.root).unwrap();
    assert!(current(&fixture));
    // Someone stepped the clock since the anchor: not written.
    anchor(&fixture, 30_000.0, 4000.0, 15_000.0);
    remembered(&fixture, 3800.0);
    remember_time(&fixture.root).unwrap();
    assert!(!current(&fixture));
    // Orderly shutdown asks for it now (`time-remember`: every=0).
    anchor(&fixture, 600.0, 600.0, 600.0);
    remembered(&fixture, 10.0);
    let reply = time::remember(&fixture.root).unwrap();
    assert!(
        reply.stdout.contains("\"source\": \"manual\""),
        "{}",
        reply.stdout
    );
    assert!(current(&fixture));
    // Another boot's anchor is not this clock.
    fixture.put("/proc/sys/kernel/random/boot_id", "boot-two");
    remembered(&fixture, 10.0);
    let reply = time::remember(&fixture.root).unwrap();
    assert_eq!(reply.stdout, "{\"state\": \"Unchanged\"}");
    assert!(!current(&fixture));
}
