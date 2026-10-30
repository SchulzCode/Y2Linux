//! `charger_state`: the suspend-relevant charger state, separate from mere USB presence.
//!
//! The kernel refuses system sleep in y2_charger_prepare_pm only while the charger engine is active or
//! failed to stop (mt6323-charger.c). USB online with a HOLD/FULL/INHIBITED engine or a "Not charging"
//! battery is NOT a refusal condition. Fix01's harness equated USB online with charging.
//! (Read by the owner's qualification tools and the power audit; the power daemon does not use it.)
// SPDX-License-Identifier: GPL-2.0-only
use crate::{
    json::{Object, ObjectExt},
    power_text::charger_fields,
    root, Root,
};

pub const ACTIVE_PHASES: [&str; 3] = ["PRECHARGE", "CONSTANT_CURRENT", "CONSTANT_VOLTAGE"];

/// `classify(charging_state, usb_online, battery_status)`, keys in Python's order.
pub fn classify(
    charging_state: Option<&str>,
    usb_online: bool,
    battery_status: Option<&str>,
) -> Object {
    let fields = charger_fields(charging_state);
    let get = |key: &str| {
        fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    };
    let phase = get("phase");
    let active = get("active") == Some("1");
    let stop_error = !matches!(get("stop_error"), None | Some("0"));
    let category = if charging_state.is_none() {
        "unknown"
    } else if stop_error {
        "charger_stop_error"
    } else if active {
        "charging_active"
    } else if matches!(phase, Some("HOLD" | "FULL")) {
        "charger_hold"
    } else if usb_online {
        "usb_present_not_charging"
    } else {
        "no_usb"
    };
    // `if phase`: an empty phase cannot occur (the pattern needs one character).
    let consistent = phase.map(|p| ACTIVE_PHASES.contains(&p) == active);
    Object::new()
        .with("category", category)
        .with("usb_present", usb_online)
        .with("phase", phase)
        .with("charging_active", active)
        .with("stop_error", stop_error)
        .with("battery_status", battery_status)
        .with("expect_refusal", active || stop_error)
        .with("phase_consistent", consistent)
}

/// `observe(ctx)`: BAT0's charging state, any supply online.
pub fn observe(root: &Root) -> Object {
    let online = root
        .glob("/sys/class/power_supply/*/online")
        .iter()
        .any(|p| root::read(p, root::READ_LIMIT).as_deref() == Some("1"));
    classify(
        root.read("/sys/class/power_supply/BAT0/charging_state")
            .as_deref(),
        online,
        root.read("/sys/class/power_supply/BAT0/status").as_deref(),
    )
}

#[cfg(test)]
#[path = "charger_state_tests.rs"]
mod charger_state_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::{dumps, Separators};

    #[test]
    fn categories_follow_charger_state_py() {
        let text = |state: Option<&str>, usb| {
            dumps(&classify(state, usb, Some("Charging")), Separators::Spaced).unwrap()
        };
        assert_eq!(
            text(Some("phase=CONSTANT_CURRENT active=1 stop_error=0"), true),
            "{\"category\":\"charging_active\",\"usb_present\":true,\"phase\":\"CONSTANT_CURRENT\",\
             \"charging_active\":true,\"stop_error\":false,\"battery_status\":\"Charging\",\
             \"expect_refusal\":true,\"phase_consistent\":true}"
        );
        assert!(text(Some("phase=HOLD active=0"), true).contains("\"charger_hold\""));
        assert!(text(Some("phase=IDLE active=0 stop_error=-5"), false)
            .contains("\"charger_stop_error\""));
        assert!(text(Some("active=0"), true).contains("\"usb_present_not_charging\""));
        assert!(text(Some(""), false).contains("\"no_usb\""));
        assert!(text(None, true).contains("\"unknown\""));
    }
}
