// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::Fixture;
use std::sync::Arc;
use y2p_contract::power::Estimate;
use y2p_core::clock::Manual;

fn fixture() -> (Fixture, Arc<Manual>) {
    let clock = Arc::new(Manual::at(1_000_000_000, 1_000_000_000));
    (Fixture::new().with_clock(clock.clone()), clock)
}

fn battery(remaining_uah: i64, charging: bool) -> BatteryStatus {
    BatteryStatus {
        charging,
        estimate: Some(Estimate {
            design_capacity_uah: None,
            remaining_uah_estimate: Some(remaining_uah),
            estimate_percent: 0,
            anchor_percent: None,
            full_latched: false,
            filtered_voltage_uv: 0,
            start: None,
            calibration_source: String::new(),
        }),
        ..BatteryStatus::default()
    }
}

fn dark() -> Observation {
    Observation {
        lease: Some("Idle".into()),
        bluetooth: true,
        ..Observation::default()
    }
}

#[test]
fn the_state_follows_cable_screen_and_playback_lease() {
    let mut seen = dark();
    assert_eq!(seen.state(), State::DarkIdle);
    seen.lease = Some("PlaybackNormal".into());
    assert_eq!(seen.state(), State::DarkPlaying);
    seen.screen_on = true;
    assert_eq!(seen.state(), State::ScreenOn);
    seen.cable = true;
    assert_eq!(seen.state(), State::Charger);
}

#[test]
fn connectivity_bits_name_bluetooth_and_wifi() {
    let line = "activated=1 powered=1 functions=0x9 calibrated=1";
    assert_eq!(connectivity_functions(line), Some(9));
    assert_eq!(connectivity_functions("powered=0 functions=0 "), Some(0));
    assert_eq!(connectivity_functions("powered=0"), None);
}

#[test]
fn observation_reads_the_kernel_files() {
    let f = Fixture::new();
    f.put(
        "/sys/devices/platform/18070000.connectivity/status",
        "powered=1 functions=0x9\n",
    );
    f.put("/sys/class/power_supply/y2-usb-presence/online", "0\n");
    f.put("/sys/class/backlight/y2-backlight/bl_power", "4\n");
    f.put(
        "/sys/module/workload/parameters/leases",
        "PlaybackNormal remaining_ms=800 min_khz=0\n",
    );
    let seen = Observation::read(&f.root);
    assert_eq!(seen.state(), State::DarkPlaying);
    assert!(seen.bluetooth && seen.wifi && !seen.cable);
    f.put("/sys/class/backlight/y2-backlight/bl_power", "0\n");
    assert_eq!(Observation::read(&f.root).state(), State::ScreenOn);
}

#[test]
fn awake_and_suspended_time_and_the_gauge_fall_go_to_their_states() {
    let (f, clock) = fixture();
    f.put_json(
        PROFILE,
        &serde_json::json!({"schema": 1, "source": "meter test",
        "currents_ma": {"asleep": 2.0, "dark_idle": 40.0}}),
    );
    let mut energy = Energy::new(&f.root, Some("boot".into()));
    energy.observe(&f.root, &battery(1_000_000, false), &dark());
    // One awake second, then 3599 s suspended and one more awake second: 2 mAh fell.
    clock.advance(1_000_000_000);
    energy.observe(&f.root, &battery(999_900, false), &dark());
    clock.suspend(3_599_000_000_000);
    clock.advance(1_000_000_000);
    energy.observe(&f.root, &battery(997_900, false), &dark());
    let r = &energy.record;
    assert_eq!(r.seconds["dark_idle"], 2.0);
    assert_eq!(r.seconds["asleep"], 3599.0);
    assert_eq!(r.radio_seconds["bluetooth"], 2.0);
    assert_eq!(r.lease_seconds["Idle"], 2.0);
    let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
    assert!(close(r.gauge_mah["dark_idle"], 0.1 + 2.0 / 3600.0));
    assert!(close(r.gauge_mah["asleep"], 2.0 * 3599.0 / 3600.0));
    assert!(close(r.profile_mah["asleep"], 2.0 * 3599.0 / 3600.0));
    assert!(close(r.profile_mah["dark_idle"], 40.0 * 2.0 / 3600.0));
    assert_eq!(r.profile_source.as_deref(), Some("meter test"));
}

#[test]
fn charging_counts_time_but_no_gauge_fall() {
    let (f, clock) = fixture();
    let mut energy = Energy::new(&f.root, None);
    let mut seen = dark();
    seen.cable = true;
    energy.observe(&f.root, &battery(1_000_000, true), &seen);
    clock.advance(1_000_000_000);
    energy.observe(&f.root, &battery(1_010_000, true), &seen);
    assert_eq!(energy.record.seconds["charger"], 1.0);
    assert!(energy.record.gauge_mah.is_empty());
}

#[test]
fn a_stopped_daemon_gap_is_not_counted() {
    let (f, clock) = fixture();
    let mut energy = Energy::new(&f.root, None);
    energy.observe(&f.root, &battery(1_000_000, false), &dark());
    clock.advance(60_000_000_000);
    energy.observe(&f.root, &battery(990_000, false), &dark());
    assert!(energy.record.seconds.is_empty());
}

#[test]
fn written_every_10_minutes_and_the_previous_boot_kept() {
    let (f, clock) = fixture();
    f.put_json(RECORD, &serde_json::json!({"schema": 1, "boot_id": "old"}));
    let mut energy = Energy::new(&f.root, Some("new".into()));
    energy.observe(&f.root, &battery(1_000_000, false), &dark());
    energy.save_due(&f.root).unwrap();
    assert_eq!(f.json(RECORD)["boot_id"], "old");
    clock.advance(1_000_000_000);
    energy.observe(&f.root, &battery(1_000_000, false), &dark());
    clock.advance(599_000_000_000);
    energy.save_due(&f.root).unwrap();
    assert_eq!(f.json(RECORD)["boot_id"], "new");
    assert_eq!(f.json(PREVIOUS)["boot_id"], "old");
    assert_eq!(f.json(RECORD)["seconds"]["dark_idle"], 1.0);
}

#[test]
fn a_player_asleep_for_days_still_writes_its_record() {
    let (f, clock) = fixture();
    f.put_json(RECORD, &serde_json::json!({"schema": 1, "boot_id": "old"}));
    let mut energy = Energy::new(&f.root, Some("asleep".into()));
    energy.observe(&f.root, &battery(1_000_000, false), &dark());
    energy.save_due(&f.root).unwrap();
    // Test build 277: 42.8 h asleep with ~200 s awake, and no record was ever written.
    clock.advance(2_000_000_000);
    clock.suspend(1_800_000_000_000);
    clock.advance(2_000_000_000);
    energy.observe(&f.root, &battery(999_000, false), &dark());
    energy.save_due(&f.root).unwrap();
    assert_eq!(f.json(RECORD)["boot_id"], "asleep");
    assert_eq!(f.json(RECORD)["seconds"]["asleep"], 1800.0);
}
