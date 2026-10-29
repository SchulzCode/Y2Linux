//! The low-battery policy test of tests/test_platform_power_contract.py, on the port.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::commands::power::battery::BatteryStatus;
use crate::testing::Fixture;

#[test]
fn thresholds_disabled_missing_sensor_and_synthetic_debounce() {
    let fixture = Fixture::new();
    let (policy, reason) = load_policy(&fixture.root);
    assert!(policy.is_none());
    assert_eq!(reason, Some("thresholds_disabled_PHYSICAL_GATE"));
    // Deliberately artificial units/values: no hardware threshold recommendation.
    fixture.put_json(
        POLICY,
        &serde_json::json!({"schema": 1, "enabled": true, "critical_uv": 10, "low_uv": 20,
            "recover_uv": 30, "critical_samples": 2, "grace_seconds": 3,
            "qualification_reference": "synthetic test"}),
    );
    let (configured, reason) = load_policy(&fixture.root);
    assert_eq!(reason, None);
    let configured = configured.as_ref();
    let mut battery = LowBattery::default();
    let none = BatteryStatus::default(); // observe(policy, voltage, present) without a SOC
    assert_eq!(
        battery.observe(configured, None, Some(1), &none),
        "Unavailable"
    );
    for (voltage, expected) in [
        (31, "Normal"),
        (19, "Low"),
        (22, "Low"),
        (9, "Critical"),
        (9, "ShutdownPending"),
    ] {
        assert_eq!(
            battery.observe(configured, Some(voltage), Some(1), &none),
            expected,
            "{voltage}"
        );
    }
    assert_eq!(
        battery.observe(None, Some(9), Some(1), &none),
        "Unavailable"
    );
}
