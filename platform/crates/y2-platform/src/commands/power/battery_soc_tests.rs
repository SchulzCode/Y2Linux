//! The SOC estimate against the Python platform's recorded sequences (battery-soc.jsonl), and the
//! Python rounding and boot kinds.
// SPDX-License-Identifier: GPL-2.0-only
use super::soc::round_int;
use super::*;
use super::{profile::*, soc::*, status::*};
use serde_json::Value;
use y2p_core::json::dumps;

fn profile() -> Profile {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../buildroot/board/y2/production-overlay/etc/y2linux/battery-profile.json"
    ))
    .unwrap();
    let dir = y2p_core_tempdir();
    std::fs::create_dir_all(dir.join("etc/y2linux")).unwrap();
    std::fs::write(dir.join("etc/y2linux/battery-profile.json"), text).unwrap();
    let profile = load_profile(&Root::new(&dir)).expect("stock profile");
    let _ = std::fs::remove_dir_all(&dir);
    profile
}

fn y2p_core_tempdir() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    std::env::temp_dir().join(format!(
        "y2-battery-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ))
}

#[test]
fn a_sample_without_the_fuel_gauge_goes_on_from_its_percent() {
    let mut soc = Soc::new(Some(profile()), None);
    let reading = |hardware_percent| Reading {
        voltage: Some(3900000),
        present: Some(1),
        status: Some("Discharging".into()),
        hardware_percent,
        ..Reading::default()
    };
    assert_eq!(soc.observe(10.0, &reading(Some(55))).percent, Some(55));
    let next = soc.observe(11.0, &reading(None));
    // The voltage estimate takes over as on a first sample (no TypeError, no crash).
    assert_eq!(next.source, BatterySource::VoltageEstimate);
    assert!(next.percent.is_some());
    assert_eq!(
        next.estimate.and_then(|e| e.start).as_deref(),
        Some("unknown/live")
    );
}

#[test]
fn rounding_and_boot_kinds_follow_python() {
    assert_eq!(round2(97.125), 97.12);
    assert_eq!(round_int(2.5), 2);
    assert_eq!(round_int(3.5), 4);
    assert_eq!(boot_kind(Some("valid=1 boot_reason=4")), Some("warm"));
    assert_eq!(
        boot_kind(Some("valid=1 offline=1 boot_reason=4")),
        Some("charged")
    );
    assert_eq!(boot_kind(Some("valid=1 boot_reason=0")), Some("cold"));
    assert_eq!(boot_kind(Some("valid=1 boot_reason=x")), None);
    assert_eq!(boot_kind(Some("valid=0 boot_reason=4")), None);
    assert_eq!(boot_kind(None), None);
}

/// Sequences of readings: every result as the Python platform's battery.Soc gave it (recorded).
#[test]
fn soc_matches_python_battery_py() {
    // (name, saved state, [(now, voltage, present, status, capacity, current, ocv, boot, charge_current)])
    type Step = (
        f64,
        i64,
        i64,
        &'static str,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<&'static str>,
        Option<i64>,
    );
    let mut scenarios: Vec<(&str, &str, Vec<Step>)> = vec![
        (
            "discharge",
            "null",
            (0..40)
                .map(|i| {
                    (
                        5000. + i as f64 * 7.3,
                        3_800_000 - i * 900,
                        1,
                        "Discharging",
                        None,
                        None,
                        None,
                        Some("cold"),
                        None,
                    )
                })
                .collect(),
        ),
        (
            "warm restart keeps 95 (F54)",
            r#"{"schema": 1, "ui_percent": 95, "anchor_percent": 97.5}"#,
            vec![
                (
                    14.,
                    4_020_000,
                    1,
                    "Discharging",
                    None,
                    None,
                    Some(3_700_000),
                    Some("warm"),
                    None,
                ),
                (
                    15.1,
                    4_019_000,
                    1,
                    "Discharging",
                    None,
                    None,
                    None,
                    None,
                    None,
                ),
            ],
        ),
        (
            "cold start uses the ocv",
            r#"{"schema": 1, "ui_percent": 13}"#,
            vec![
                (
                    30.,
                    3_900_000,
                    1,
                    "Discharging",
                    None,
                    None,
                    Some(3_950_000),
                    Some("cold"),
                    None,
                ),
                (
                    31.,
                    3_899_000,
                    1,
                    "Discharging",
                    None,
                    None,
                    None,
                    None,
                    None,
                ),
            ],
        ),
        (
            "charged boot drops a lower saved value (F36)",
            r#"{"schema": 1, "ui_percent": 5}"#,
            vec![
                (
                    4000.,
                    4_027_000,
                    1,
                    "Charging",
                    None,
                    None,
                    Some(3_440_000),
                    Some("charged"),
                    Some(500_000),
                ),
                (
                    4001.5,
                    4_028_000,
                    1,
                    "Charging",
                    None,
                    None,
                    None,
                    None,
                    Some(480_000),
                ),
            ],
        ),
        ("charging up to full", "null", {
            let mut v: Vec<Step> = (0..30)
                .map(|i| {
                    (
                        100. + i as f64 * 3.1,
                        4_100_000 + i * 2_000,
                        1,
                        "Charging",
                        None,
                        None,
                        None,
                        Some("unknown"),
                        Some(300_000 - i * 9_000),
                    )
                })
                .collect();
            v.extend((0..30).map(|i| {
                (
                    200. + i as f64 * 4.7,
                    4_175_000,
                    1,
                    "Full",
                    None,
                    None,
                    None,
                    None,
                    None,
                )
            }));
            v.extend((0..5).map(|i| {
                (
                    400. + i as f64,
                    4_150_000,
                    1,
                    "Discharging",
                    None,
                    None,
                    None,
                    None,
                    None,
                )
            }));
            v
        }),
        (
            "hardware and pack removal",
            "null",
            vec![
                (
                    10.,
                    3_700_000,
                    1,
                    "Discharging",
                    Some(55),
                    None,
                    None,
                    None,
                    None,
                ),
                (
                    11.,
                    3_700_000,
                    0,
                    "Discharging",
                    None,
                    None,
                    None,
                    None,
                    None,
                ),
                (
                    12.,
                    3_690_000,
                    1,
                    "Not charging",
                    None,
                    None,
                    None,
                    None,
                    None,
                ),
                (
                    5.,
                    3_690_000,
                    1,
                    "Discharging",
                    None,
                    None,
                    None,
                    None,
                    None,
                ),
                (
                    6.,
                    3_350_000,
                    1,
                    "Discharging",
                    None,
                    None,
                    None,
                    None,
                    None,
                ),
            ],
        ),
    ];
    // `calibrated_resistance_milliohm` + `current_now`: the hybrid source.
    scenarios.push((
        "hybrid",
        "null",
        (0..10)
            .map(|i| {
                (
                    50. + i as f64,
                    3_750_000,
                    1,
                    "Discharging",
                    None,
                    Some(-150_000 - i * 1000),
                    None,
                    None,
                    None,
                )
            })
            .collect(),
    ));
    // Stock oam_init on a charger: the power-on OCV within 31 points wins, beyond it the live
    // sample; a stale OCV (after 120 s) and a sagging one on battery are ignored; status None.
    for (name, status, ocv, now) in [
        ("charger ocv near", "Charging", 3_980_000, 40.),
        ("charger ocv far", "Charging", 3_500_000, 40.),
        ("ocv too old", "Discharging", 3_950_000, 121.),
        ("ocv below load", "Discharging", 3_700_000, 20.),
        ("ocv no status", "", 3_950_000, 20.),
    ] {
        scenarios.push((
            name,
            r#"{"schema": 1, "ui_percent": 70, "anchor_percent": 90}"#,
            vec![
                (
                    now,
                    3_900_000,
                    1,
                    status,
                    None,
                    None,
                    Some(ocv),
                    Some("cold"),
                    Some(200_000),
                ),
                (
                    now + 1.0,
                    3_901_000,
                    1,
                    status,
                    None,
                    None,
                    None,
                    None,
                    Some(200_000),
                ),
            ],
        ));
    }
    let mut ours = String::new();
    for (name, saved, steps) in &scenarios {
        let mut p = profile();
        if *name == "hybrid" {
            p.calibrated_resistance = Some(180);
        }
        let mut soc = Soc::new(
            Some(p),
            serde_json::from_slice::<Value>(saved.as_bytes()).ok(),
        );
        let mut out = Vec::new();
        for &(now, v, present, status, cap, cur, ocv, boot, cc) in steps {
            let reading = Reading {
                voltage: Some(v),
                present: Some(present),
                status: Some(status.into()),
                hardware_percent: cap,
                current: cur,
                charge_current: cc,
                boot_ocv: ocv,
                boot,
                ..Reading::default()
            };
            let result = serde_json::to_value(soc.observe(now, &reading)).unwrap();
            let state = serde_json::to_value(soc.state().map(DisplayRecord::new)).unwrap();
            out.push(Value::Array(vec![result, state]));
        }
        ours += &(dumps(&out).unwrap() + "\n");
    }
    y2p_core::recorded::check(
        env!("CARGO_MANIFEST_DIR"),
        "battery-soc.jsonl",
        ours.as_bytes(),
    );
}
