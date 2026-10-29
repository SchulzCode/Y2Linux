//! tests/test_cpu_fix02.py Fix02Harness: USB presence is not charging; only an active or failed
//! charger engine refuses system sleep.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;

use serde_json::Value;

fn field<'a>(value: &'a Object, key: &str) -> &'a Value {
    value.get(key).unwrap_or(&Value::Null)
}

#[test]
fn charger_presence_is_not_charging() {
    let hold =
        "phase=HOLD source=1 source_valid=1 charge_limit_ua=70000\nactive=0 online=1 present=1 \
                fault=0x0 sample_error=0 stop_error=0 paused=0 last_error=0";
    let category = |state: &str, usb, status| {
        field(&classify(Some(state), usb, Some(status)), "category")
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(category(hold, true, "Not charging"), "charger_hold");
    // The Fix01 physical state.
    assert_eq!(
        field(
            &classify(Some(hold), true, Some("Not charging")),
            "expect_refusal"
        ),
        &Value::Bool(false)
    );
    let active = hold
        .replace("phase=HOLD", "phase=CONSTANT_CURRENT")
        .replace("active=0", "active=1");
    let charging = classify(Some(&active), true, Some("Charging"));
    assert_eq!(field(&charging, "expect_refusal"), &Value::Bool(true));
    assert_eq!(field(&charging, "phase_consistent"), &Value::Bool(true));
    let failed = hold.replace("stop_error=0", "stop_error=-5");
    assert_eq!(
        category(&failed, true, "Not charging"),
        "charger_stop_error"
    );
    let inhibited = hold.replace("phase=HOLD", "phase=INHIBITED");
    assert_eq!(
        category(&inhibited, true, "Not charging"),
        "usb_present_not_charging"
    );
    assert_eq!(
        category(
            &inhibited.replace("online=1", "online=0"),
            false,
            "Discharging"
        ),
        "no_usb"
    );
    assert_eq!(
        field(&classify(None, true, None), "category").as_str(),
        Some("unknown")
    );
}
