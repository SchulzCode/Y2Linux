//! When the automatic setup may start: no update waiting for its health check, and a charger or
//! enough battery.
// SPDX-License-Identifier: GPL-2.0-only
use super::super::sys::{isdigit, read};
use crate::commands::power::battery::{interpolate_int, load_profile};
use y2p_core::Root;

// On battery: at least the percentage the plan named, read like the shown one (stock
// oam_init: the PMIC's power-on OCV, latched before the boot load pulls the voltage down).
const MIN_PERCENT: f64 = 25.0;

/// Why the setup must not start on its own now (it runs at a later start).
pub(super) fn automatic_blocked(root: &Root) -> Option<&'static str> {
    if crate::commands::service::update::journal_state(root).installing() {
        return Some("update_in_progress"); // 180 s health window: never a long step here
    }
    let battery = root.path("/sys/class/power_supply/BAT0");
    if matches!(
        read(&battery.join("status")).as_deref(),
        Some("Charging" | "Full")
    ) || root
        .read("/sys/class/power_supply/y2-usb-input/online")
        .as_deref()
        == Some("1")
    {
        return None;
    }
    // The profile the power daemon uses (the calibrated one on Y2DATA, else the image's).
    let profile = load_profile(root).map(|profile| profile.voltage_soc);
    let mut voltage = read(&battery.join("voltage_ocv"));
    if !voltage.as_deref().is_some_and(isdigit) {
        voltage = read(&battery.join("voltage_now")); // loaded: lower, so never too early
    }
    let (Some(profile), Some(voltage)) = (profile, voltage.filter(|v| isdigit(v))) else {
        return Some("battery_unknown");
    };
    let uv = voltage.parse::<i64>().unwrap_or(i64::MAX);
    if interpolate_int(&profile, uv) >= MIN_PERCENT {
        None
    } else {
        Some("battery_low")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_interpolate_between_rows() {
        let points = [(3_400_000, 0), (3_800_000, 50), (4_200_000, 100)];
        assert_eq!(interpolate_int(&points, 3_000_000), 0.0);
        assert_eq!(interpolate_int(&points, 3_600_000), 25.0);
        assert_eq!(interpolate_int(&points, 4_300_000), 100.0);
        assert_eq!(interpolate_int(&points, 3_700_001), 37.500125);
    }
}
