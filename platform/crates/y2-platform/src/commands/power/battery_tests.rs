//! The battery SOC tests of tests/test_battery_soc.py, on the port: the stock profile, `Soc.observe`,
//! `sample` and `boot_kind`, with `LowBattery` where the Python tests combined them.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use super::{profile::*, soc::*, status::*};
use crate::commands::power::policy::{self, load_policy, BatteryState, LowBattery};
use crate::testing::Fixture;
use serde_json::Value;

const ETC: &str = "buildroot/board/y2/production-overlay/etc/y2linux";
const BAT0: &str = "/sys/class/power_supply/BAT0/";

/// A fixture root with the stock battery profile installed (the Python `setUp`).
fn fixture() -> Fixture {
    let fixture = Fixture::new();
    fixture.copy_from_repository(&format!("{ETC}/battery-profile.json"), PROFILE);
    fixture
}

fn stock(fixture: &Fixture) -> Profile {
    load_profile(&fixture.root).expect("stock profile")
}

fn put_bat0(fixture: &Fixture, name: &str, text: &str) {
    fixture.put(&format!("{BAT0}{name}"), text);
}

/// `Soc.observe(now, voltage, present, status)`'s reading; the keywords are struct updates.
fn at(voltage: i64, present: i64, status: &str) -> Reading {
    Reading {
        voltage: Some(voltage),
        present: Some(present),
        status: Some(status.into()),
        ..Reading::default()
    }
}

/// A saved display state (`battery-ui.json`) as `Soc(profile, state)` receives it.
fn saved(text: &str) -> Option<Value> {
    Some(serde_json::from_slice::<Value>(text.as_bytes()).expect("saved state"))
}

/// `soc.state()` as the daemon writes it: `{'schema': 1, 'ui_percent': ..., 'anchor_percent': ...}`.
fn state_json(soc: &Soc) -> Option<Value> {
    soc.state()
        .map(|state| serde_json::to_value(DisplayRecord::new(state)).unwrap())
}

/// One field of a published reading as power.json holds it (null when the reading has none).
fn field(value: &BatteryStatus, key: &str) -> Value {
    serde_json::to_value(value).unwrap()[key].clone()
}

fn percent(value: &BatteryStatus) -> Option<i64> {
    value.percent
}

fn shown(value: &BatteryStatus) -> i64 {
    percent(value).expect("a percent")
}

fn int(value: &BatteryStatus, key: &str) -> Option<i64> {
    field(value, key).as_i64()
}

fn text(value: &BatteryStatus, key: &str) -> Option<String> {
    field(value, key).as_str().map(str::to_owned)
}

fn flag(value: &BatteryStatus, key: &str) -> bool {
    match field(value, key) {
        Value::Bool(b) => b,
        other => panic!("{key}: {other:?}"),
    }
}

fn estimate(percent: i64, confidence: Confidence) -> BatteryStatus {
    BatteryStatus {
        percent: Some(percent),
        confidence,
        ..BatteryStatus::default()
    }
}

fn stock_policy(fixture: &Fixture) -> policy::Policy {
    fixture.copy_from_repository(
        &format!("{ETC}/power-policy.json"),
        "/etc/y2linux/power-policy.json",
    );
    let (policy, reason) = load_policy(&fixture.root);
    assert_eq!(reason, None);
    policy.expect("stock policy")
}

#[test]
fn invalid_override_fails_closed_and_real_fg_has_priority() {
    let fixture = fixture();
    fixture.put(OVERRIDE, r#"{"schema":1,"voltage_soc":[]}"#);
    assert!(load_profile(&fixture.root).is_none());
    let mut soc = Soc::new(None, None);
    assert_eq!(
        percent(&soc.observe(0., &at(3800000, 1, "Discharging"))),
        None
    );
    let value = soc.observe(
        1.,
        &Reading {
            hardware_percent: Some(71),
            ..at(3800000, 1, "Discharging")
        },
    );
    assert_eq!(
        (
            percent(&value),
            text(&value, "source"),
            text(&value, "confidence")
        ),
        (Some(71), Some("fuel_gauge".into()), Some("hardware".into()))
    );
}

/// A fuel-gauge sample sets no display value (nothing is saved for the next start); the next sample
/// without `capacity` starts the voltage estimate as a first sample (Python ended its daemon there).
#[test]
fn a_fuel_gauge_sample_saves_nothing_and_the_estimate_takes_over() {
    let fixture = fixture();
    let mut soc = Soc::new(Some(stock(&fixture)), None);
    let value = soc.observe(
        0.,
        &Reading {
            hardware_percent: Some(71),
            ..at(3800000, 1, "Discharging")
        },
    );
    assert_eq!(percent(&value), Some(71));
    assert_eq!(soc.ui, None);
    assert_eq!(soc.state(), None);
    let next = soc.observe(1., &at(3800000, 1, "Discharging"));
    assert_eq!(text(&next, "source"), Some("voltage_estimate".into()));
    assert!(soc.state().is_some());
}

#[test]
fn no_fake_sensors_charge_limit_or_die_temperature() {
    let fixture = fixture();
    put_bat0(&fixture, "present", "1");
    put_bat0(&fixture, "status", "Charging");
    put_bat0(&fixture, "voltage_now", "3800000");
    fixture.put(
        "/sys/class/power_supply/USB/constant_charge_current",
        "450000",
    );
    fixture.put("/sys/class/thermal/thermal_zone0/temp", "50000");
    let value = sample_at(
        &fixture.root,
        &mut Soc::new(load_profile(&fixture.root), None),
        0.,
    );
    assert_eq!(text(&value, "source").as_deref(), Some("voltage_estimate"));
    assert!(field(&value, "current_ua").is_null());
    assert!(field(&value, "temperature_millicelsius").is_null());
    assert!(field(&value, "charge_counter_uah").is_null());
}

#[test]
fn sag_rebound_is_monotonic_and_charging_smoothing_bounded() {
    let fixture = fixture();
    let mut soc = Soc::new(Some(stock(&fixture)), None);
    let first = shown(&soc.observe(0., &at(3900000, 1, "Discharging")));
    let low = shown(&soc.observe(60., &at(3700000, 1, "Discharging")));
    let rebound = shown(&soc.observe(120., &at(3950000, 1, "Discharging")));
    assert!(rebound <= low);
    assert!(low >= first - 1);
    let charging = shown(&soc.observe(180., &at(4175000, 1, "Charging")));
    assert!(charging <= rebound + 1);
    // 28 (stock 100Percent tracking): Full counts up one point per 10 s.
    assert_eq!(
        shown(&soc.observe(181., &at(4175000, 1, "Full"))),
        charging + 1
    );
    assert_eq!(
        shown(&soc.observe(185., &at(4175000, 1, "Full"))),
        charging + 1
    );
    let mut now = 191.;
    while shown(&soc.observe(now, &at(4090000, 1, "Full"))) < 100 {
        now += 10.;
    }
    // Resting full pack of this charger (measured 4.09 V) is 100%, not 94%.
    assert_eq!(shown(&soc.observe(now + 1., &at(4090000, 1, "Full"))), 100);
    // An implausible Full at a low voltage never forces 100%.
    assert!(
        shown(&Soc::new(Some(stock(&fixture)), None).observe(0., &at(3900000, 1, "Full"))) < 100
    );
    assert!(shown(&soc.observe(now + 2., &at(3390000, 1, "Discharging"))) <= 1);
    assert_eq!(
        percent(&soc.observe(now + 3., &at(3390000, 0, "Unknown"))),
        None
    );
    assert_eq!(
        shown(&soc.observe(now + 4., &at(3800000, 1, "Discharging"))),
        54
    );
}

#[test]
fn rest_and_hybrid_require_actual_calibration() {
    let fixture = fixture();
    let mut p = stock(&fixture);
    p.calibrated_resistance = Some(100);
    let mut soc = Soc::new(Some(p), None);
    let mut value = BatteryStatus::default();
    for now in 0..121 {
        value = soc.observe(
            now as f64,
            &Reading {
                current: Some(-100000),
                ..at(3800000, 1, "Discharging")
            },
        );
    }
    assert!(flag(&value, "resting"));
    assert_eq!(text(&value, "source").as_deref(), Some("hybrid"));
}

#[test]
fn sleep_gap_exposes_voltage_loss_without_changing_curve_or_rate() {
    let fixture = fixture();
    put_bat0(&fixture, "present", "1");
    put_bat0(&fixture, "status", "Discharging");
    put_bat0(&fixture, "voltage_now", "4000000");
    let mut soc = Soc::new(Some(stock(&fixture)), None);
    // CLOCK_BOOTTIME at the three samples: 100 s, 700 s, 7900 s.
    assert_eq!(shown(&sample_at(&fixture.root, &mut soc, 100.)), 87);
    put_bat0(&fixture, "voltage_now", "3800000");
    let after_ten_minutes = sample_at(&fixture.root, &mut soc, 700.);
    // The existing one-point-per-minute limit gets ten real minutes,
    // not zero suspended minutes or an artificial one-minute ceiling.
    assert_eq!(shown(&after_ten_minutes), 77);
    assert!(int(&after_ten_minutes, "filtered_voltage_uv").unwrap() < 3810000);
    let after_two_hours = sample_at(&fixture.root, &mut soc, 7900.);
    assert_eq!(shown(&after_two_hours), 54);
    assert!(!flag(&after_two_hours, "resting"));
    assert!(!flag(&after_two_hours, "provisional"));
    assert!(field(&after_two_hours, "current_ua").is_null());
}

#[test]
fn sleep_time_alone_never_changes_soc_and_rebound_is_not_charge() {
    let fixture = fixture();
    let mut soc = Soc::new(Some(stock(&fixture)), None);
    assert_eq!(shown(&soc.observe(0., &at(3800000, 1, "Discharging"))), 54);
    assert_eq!(
        shown(&soc.observe(28800., &at(3800000, 1, "Discharging"))),
        54
    );
    assert_eq!(
        shown(&soc.observe(57600., &at(4000000, 1, "Discharging"))),
        54
    );
}

#[test]
fn low_soc_estimate_warns_but_shutdown_requires_trust_or_hard_floor() {
    let fixture = fixture();
    let policy = stock_policy(&fixture);
    let mut guard = LowBattery::default();
    let provisional = estimate(1, Confidence::Provisional);
    for _ in 0..10 {
        assert_eq!(
            guard.observe(Some(&policy), Some(3800000), Some(1), &provisional),
            "Critical"
        );
    }
    for _ in 0..4 {
        assert_eq!(
            guard.observe(Some(&policy), Some(3300000), Some(1), &provisional),
            "Critical"
        );
    }
    assert_eq!(
        guard.observe(Some(&policy), Some(3300000), Some(1), &provisional),
        "ShutdownPending"
    );
    let mut guard = LowBattery::default();
    let mut state = BatteryState::Unavailable;
    for _ in 0..5 {
        state = guard.observe(
            Some(&policy),
            Some(3800000),
            Some(1),
            &estimate(1, Confidence::Hardware),
        );
    }
    assert_eq!(state, "ShutdownPending");
}

/// POWER-FINAL-16: fgauge_get_profile(25 C) and r_profile from the Y2 FM kernel.
#[test]
fn stock_profile_is_the_fm_kernel_table() {
    let fixture = fixture();
    let p = stock(&fixture);
    assert!(!p.provisional);
    assert_eq!(p.design_capacity_uah, Some(1300000));
    assert_eq!(p.voltage_soc[0], (3400000, 0));
    assert_eq!(p.voltage_soc[p.voltage_soc.len() - 1], (4175000, 100));
    assert_eq!(p.voltage_soc.len(), 95);
    assert!(p.voltage_soc.contains(&(3779000, 50))); // stock DOD 50 at 3779 mV
    let resistance = p.resistance.clone().expect("r_profile");
    assert_eq!(resistance.len(), 95);
    assert!(resistance.iter().all(|(_, r)| (190..=330).contains(r)));
    let value = Soc::new(Some(p), None).observe(0., &at(3779000, 1, "Discharging"));
    assert_eq!(
        (percent(&value), text(&value, "confidence")),
        (Some(50), Some("calibrated_estimate".into()))
    );
    assert_eq!(int(&value, "remaining_uah_estimate"), Some(650000));
}

#[test]
fn charging_uses_stock_oam_resistance_times_sense_current() {
    let fixture = fixture();
    let p = stock(&fixture);
    let fresh = || Soc::new(Some(p.clone()), None);
    let current = |uv, status, current| Reading {
        charge_current: Some(current),
        ..at(uv, 1, status)
    };
    // 450 mA * ~0.2 ohm ~= 90 mV: 3.89 V under charge reads as ~3.80 V OCV.
    let value = fresh().observe(0., &current(3890000, "Charging", 450000));
    assert_eq!(text(&value, "source").as_deref(), Some("stock_oam"));
    assert!((53..=57).contains(&shown(&value)), "{value:?}");
    // Without a sense current the fixed charging offset still applies.
    let fallback = fresh().observe(0., &at(3890000, 1, "Charging"));
    assert_eq!(
        text(&fallback, "source").as_deref(),
        Some("voltage_estimate")
    );
    // An absurd current never moves the estimate by more than 300 mV.
    let clipped = fresh().observe(0., &current(4100000, "Charging", 1200000));
    assert!(shown(&clipped) >= 49);
    // Discharging never uses the charger-path current.
    assert_eq!(
        text(
            &fresh().observe(0., &current(3800000, "Discharging", 450000)),
            "source"
        )
        .as_deref(),
        Some("voltage_estimate")
    );
}

// POWER-FINAL-28: the owner's case. On the charger 100 %, after a restart
// about 80 %; wanted 100 % when full, then 99, 98, 97 ... (ledger F28-F30).
fn full_soc(fixture: &Fixture, rest_uv: i64) -> (Soc, f64) {
    let mut soc = Soc::new(Some(stock(fixture)), None);
    let mut now = 0.;
    soc.observe(
        now,
        &Reading {
            charge_current: Some(70000),
            ..at(4100000, 1, "Charging")
        },
    );
    while shown(&soc.observe(now, &at(rest_uv, 1, "Full"))) < 100 {
        now += 10.;
    }
    (soc, now)
}

#[test]
fn charging_shows_at_most_99_until_full() {
    let fixture = fixture();
    let mut soc = Soc::new(Some(stock(&fixture)), None);
    for now in (0..3600).step_by(10) {
        let value = soc.observe(now as f64, &at(4170000, 1, "Charging"));
        assert!(shown(&value) <= 99);
    }
    // 28 finish stage at the target, 70 mA: the estimate reaches 100, shown 99.
    let mut value = BatteryStatus::default();
    for now in (3600..7200).step_by(10) {
        value = soc.observe(
            now as f64,
            &Reading {
                charge_current: Some(10000),
                ..at(4175000, 1, "Charging")
            },
        );
    }
    assert_eq!(
        (int(&value, "estimate_percent"), percent(&value)),
        (Some(100), Some(99))
    );
    assert_eq!(shown(&soc.observe(7200., &at(4150000, 1, "Full"))), 100);
}

#[test]
fn full_stays_100_on_the_charger_then_counts_down_one_by_one() {
    let fixture = fixture();
    let (mut soc, mut now) = full_soc(&fixture, 4150000);
    let anchor = soc.anchor.expect("anchor");
    assert!((98.0..=99.5).contains(&anchor), "{anchor}");
    // Maintenance hold drains and recharges on the charger: still 100.
    for (uv, status) in [
        (4120000, "Full"),
        (4105000, "Not charging"),
        (4140000, "Charging"),
    ] {
        now += 600.;
        assert_eq!(shown(&soc.observe(now, &at(uv, 1, status))), 100);
    }
    // Unplugged: 100, then single steps as the pack really discharges.
    let mut values = Vec::new();
    let mut uv = 4150000;
    for _ in 0..400 {
        now += 30.;
        uv -= 1000;
        values.push(shown(&soc.observe(now, &at(uv, 1, "Discharging"))));
    }
    assert_eq!(values[0], 100);
    let steps: Vec<i64> = values.windows(2).map(|w| w[0] - w[1]).collect();
    assert!(steps.iter().all(|s| [0, 1].contains(s)), "{steps:?}");
    for expected in [99, 98, 97] {
        assert!(values.contains(&expected), "{expected}");
    }
}

#[test]
fn restart_keeps_the_display_instead_of_the_boot_sag() {
    let fixture = fixture();
    let (soc, _) = full_soc(&fixture, 4150000);
    let state = state_json(&soc);
    let (ui, anchor) = soc.state().expect("state");
    assert_eq!(ui, 100);
    // 27 boot 5919b4fe: first sample 4.002 V under boot load (87 % raw).
    let cold = Soc::new(Some(stock(&fixture)), None).observe(0., &at(4002000, 1, "Discharging"));
    assert_eq!(shown(&cold), 87);
    // Power-on OCV alone (4.062 V latched before load) already says ~92 %.
    let ocv = Soc::new(Some(stock(&fixture)), None).observe(
        0.,
        &Reading {
            boot_ocv: Some(4062000),
            ..at(4002000, 1, "Discharging")
        },
    );
    assert_eq!(shown(&ocv), 92);
    // With the saved display value (stock RTC spare) the restart shows 100.
    let again = Soc::new(Some(stock(&fixture)), state).observe(
        0.,
        &Reading {
            boot_ocv: Some(4150000),
            ..at(4002000, 1, "Discharging")
        },
    );
    assert_eq!(shown(&again), 100);
    assert_eq!(field(&again, "anchor_percent"), Value::from(anchor));
}

#[test]
fn saved_value_is_refused_beyond_40_points_and_charger_moved_ocv() {
    let fixture = fixture();
    let state = saved(r#"{"schema": 1, "ui_percent": 100, "anchor_percent": 98.8}"#);
    let ocv = |uv, status, ocv| Reading {
        boot_ocv: Some(ocv),
        ..at(uv, 1, status)
    };
    let stale =
        Soc::new(Some(stock(&fixture)), state).observe(0., &ocv(3700000, "Discharging", 3700000));
    // 40-point rule
    assert!(shown(&stale) < 60);
    // Booted on a charger: an OCV 31+ points away from the live sample loses.
    let p = stock(&fixture);
    let moved = Soc::new(Some(p.clone()), None).observe(0., &ocv(3800000, "Charging", 4150000));
    assert!(shown(&moved) < 70);
    // Garbage in the saved file is ignored.
    let junk = Soc::new(
        Some(p),
        saved(r#"{"ui_percent": "x", "anchor_percent": 7}"#),
    )
    .observe(0., &at(3779000, 1, "Discharging"));
    assert_eq!(
        (percent(&junk), field(&junk, "anchor_percent")),
        (Some(50), Value::Null)
    );
}

#[test]
fn start_on_a_charger_after_a_low_battery_poweroff_runs() {
    // F36 (97f11bb1): turned off at 5 %, charged on the charging screen for
    // an hour, then every start showed 5 % and turned off again at 4.03 V.
    let fixture = fixture();
    let policy = stock_policy(&fixture);
    let low = r#"{"schema": 1, "ui_percent": 5, "anchor_percent": null}"#;
    for (now, voltage, ocv) in [(3850, 4027148, 3620000), (17, 3714257, 3700000)] {
        let mut soc = Soc::new(Some(stock(&fixture)), saved(low));
        let mut guard = LowBattery::default();
        let mut value = BatteryStatus::default();
        let mut state = BatteryState::Unavailable;
        for tick in 0..6 {
            value = soc.observe(
                (now + tick) as f64,
                &Reading {
                    charge_current: Some(450000),
                    boot_ocv: Some(ocv),
                    ..at(voltage, 1, "Charging")
                },
            );
            state = guard.observe(Some(&policy), Some(voltage), Some(1), &value);
        }
        assert!(shown(&value) > 30);
        assert_ne!(state, "ShutdownPending");
    }
    let ocv = |now: f64, ocv| {
        Soc::new(Some(stock(&fixture)), None).observe(
            now,
            &Reading {
                boot_ocv: Some(ocv),
                ..at(3779000, 1, "Discharging")
            },
        )
    };
    // The stale power-on OCV alone no longer decides a late first sample.
    let late = ocv(3850., 3850000);
    let fresh = ocv(10., 3850000);
    assert!(shown(&late) < shown(&fresh));
    // F54: an OCV below the loaded voltage predates a charge and is ignored.
    let below = ocv(10., 3500000);
    assert_eq!(shown(&below), shown(&late));
    // On battery a saved 5 % still turns a drained player off.
    let mut soc = Soc::new(Some(stock(&fixture)), saved(low));
    let mut guard = LowBattery::default();
    let mut state = BatteryState::Unavailable;
    for tick in 0..6 {
        let value = soc.observe((10 + tick) as f64, &at(3627026, 1, "Discharging"));
        state = guard.observe(Some(&policy), Some(3627026), Some(1), &value);
    }
    assert_eq!(state, "ShutdownPending");
    // A charger that cannot carry the load is still caught by the voltage floor.
    let mut guard = LowBattery::default();
    let charging = BatteryStatus {
        charging: true,
        status: Some("Charging".into()),
        ..estimate(40, Confidence::CalibratedEstimate)
    };
    for _ in 0..5 {
        state = guard.observe(Some(&policy), Some(3390000), Some(1), &charging);
    }
    assert_eq!(state, "ShutdownPending");
}

#[test]
fn warm_restart_keeps_the_saved_percent_not_the_old_power_on_ocv() {
    // F54, 98499743 (8.38): cold start near empty, charged to 95 %, then an
    // update restart (boot_reason=4) read the old 0x724 OCV and showed 13 %.
    assert_eq!(
        boot_kind(Some(
            "valid=1 boot_mode=0 boot_reason=4 offline=0 recovery_image=0 panel=gc9503v"
        )),
        Some("warm")
    );
    assert_eq!(
        boot_kind(Some(
            "valid=1 boot_mode=8 boot_reason=3 offline=1 recovery_image=0"
        )),
        Some("charged")
    );
    assert_eq!(
        boot_kind(Some("valid=1 boot_mode=0 boot_reason=0 offline=0")),
        Some("cold")
    );
    assert_eq!(
        boot_kind(Some("valid=0 boot_mode=-1 boot_reason=-1 offline=0")),
        None
    );
    assert_eq!(boot_kind(None), None);
    let fixture = fixture();
    let restart = |state: &str, voltage, ocv: Option<i64>, boot| {
        Soc::new(Some(stock(&fixture)), saved(state)).observe(
            14.,
            &Reading {
                boot_ocv: ocv,
                boot,
                ..at(voltage, 1, "Discharging")
            },
        )
    };
    let high = r#"{"schema": 1, "ui_percent": 95, "anchor_percent": null}"#;
    let mut value = BatteryStatus::default();
    for boot in [Some("warm"), None] {
        value = restart(high, 4021655, Some(3545000), boot);
        assert_eq!(shown(&value), 95);
    }
    assert_eq!(text(&value, "start").as_deref(), Some("unknown/saved"));
    // Played down since the cold start: the old OCV is plausible by voltage
    // alone, so only the boot reason tells it apart.
    let forty = r#"{"schema": 1, "ui_percent": 40, "anchor_percent": null}"#;
    let warm = restart(forty, 3700000, Some(3950000), Some("warm"));
    assert_eq!(
        (percent(&warm), text(&warm, "start")),
        (Some(40), Some("warm/saved".into()))
    );
    let cold = restart(forty, 3700000, Some(3950000), Some("cold"));
    assert_eq!(text(&cold, "start").as_deref(), Some("cold/ocv"));
    assert!(shown(&cold) > 70);
    // Stock dod_init keeps the saved value beyond 40 points after a warm
    // restart; only a live estimate 40+ above it proves it wrong.
    let low = restart(
        r#"{"schema": 1, "ui_percent": 13}"#,
        4021655,
        None,
        Some("warm"),
    );
    assert_eq!(
        (percent(&low), text(&low, "start")),
        (Some(89), Some("warm/live".into()))
    );
    let high = restart(
        r#"{"schema": 1, "ui_percent": 95}"#,
        3700000,
        None,
        Some("warm"),
    );
    assert_eq!(shown(&high), 95);
}

#[test]
fn warm_restart_on_a_charger_keeps_the_lower_saved_percent() {
    // F54, c0a4f35d (8.41): update restart while charging at 28 % showed 42 %,
    // then 37, then 29 at unplug.
    let fixture = fixture();
    let value = Soc::new(
        Some(stock(&fixture)),
        saved(r#"{"schema": 1, "ui_percent": 28, "anchor_percent": 96.74}"#),
    )
    .observe(
        14.,
        &Reading {
            boot_ocv: Some(3720000),
            boot: Some("warm"),
            ..at(3633837, 1, "Charging")
        },
    );
    assert_eq!(shown(&value), 28);
    // F36 is kept: after LK's charging screen (offline=1) or a cold start on
    // a charger, a lower saved value still gives way to the live one.
    for (boot, status) in [
        (Some("charged"), "Discharging"),
        (Some("charged"), "Charging"),
        (Some("cold"), "Charging"),
        (None, "Charging"),
    ] {
        let value = Soc::new(
            Some(stock(&fixture)),
            saved(r#"{"schema": 1, "ui_percent": 5, "anchor_percent": null}"#),
        )
        .observe(
            17.,
            &Reading {
                boot,
                ..at(3714257, 1, status)
            },
        );
        assert!(shown(&value) > 20, "{boot:?} {status}");
        assert!(
            text(&value, "start").unwrap().ends_with("/live"),
            "{boot:?} {status}"
        );
    }
}

#[test]
fn sample_reads_the_boot_reason_once() {
    let fixture = fixture();
    put_bat0(&fixture, "present", "1");
    put_bat0(&fixture, "status", "Discharging");
    put_bat0(&fixture, "voltage_now", "4021655");
    put_bat0(&fixture, "voltage_ocv", "3545000");
    fixture.put(
        BOOT_METADATA,
        "valid=1 boot_mode=0 boot_reason=4 offline=0 recovery_image=0",
    );
    let mut soc = Soc::new(
        Some(stock(&fixture)),
        saved(r#"{"schema": 1, "ui_percent": 95}"#),
    );
    assert_eq!(
        text(&sample_at(&fixture.root, &mut soc, 14.), "start").as_deref(),
        Some("warm/saved")
    );
    assert_eq!(shown(&sample_at(&fixture.root, &mut soc, 15.)), 95);
}

#[test]
fn awake_load_follows_the_drain_not_the_loaded_voltage() {
    // 28 boot 8f43c71e: started at 89 % from the power-on OCV, then the old
    // estimator pulled toward the loaded 3.95-3.98 V and showed 85 within
    // 3.5 minutes. Stock oam_run integrates the model current instead.
    let fixture = fixture();
    let mut soc = Soc::new(Some(stock(&fixture)), None);
    let mut value = soc.observe(
        0.,
        &Reading {
            boot_ocv: Some(4022000),
            ..at(3946069, 1, "Discharging")
        },
    );
    assert_eq!(shown(&value), 89);
    for now in 1..211 {
        value = soc.observe(now as f64, &at(3970000, 1, "Discharging"));
    }
    assert!(shown(&value) >= 88);
    // That model current is a real drain: sustained load still discharges.
    for now in 211..3811 {
        value = soc.observe(now as f64, &at(3940000, 1, "Discharging"));
    }
    assert!(shown(&value) < 85);
    // A rested voltage after a suspend gap is still taken directly.
    let after = soc.observe((3811 + 1800) as f64, &at(3800000, 1, "Discharging"));
    assert!(int(&after, "estimate_percent").unwrap() < int(&value, "estimate_percent").unwrap());
}

/// The service half of test_service_persists_the_display_value: `sample` reads BAT0's power-on OCV
/// (`voltage_ocv`); the daemon's restore and write of the display value are tests/power.rs.
#[test]
fn sample_reads_the_power_on_ocv() {
    let fixture = fixture();
    put_bat0(&fixture, "present", "1");
    put_bat0(&fixture, "status", "Discharging");
    put_bat0(&fixture, "voltage_now", "4002000");
    let mut soc = Soc::new(Some(stock(&fixture)), None);
    assert_eq!(shown(&sample_at(&fixture.root, &mut soc, 0.)), 87);
    put_bat0(&fixture, "voltage_ocv", "4062000");
    let mut soc = Soc::new(Some(stock(&fixture)), None);
    let value = sample_at(&fixture.root, &mut soc, 0.);
    assert_eq!(
        (percent(&value), text(&value, "start")),
        (Some(92), Some("unknown/ocv".into()))
    );
}
