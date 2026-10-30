//! When the automatic setup may start: no update waiting for its health check, and a charger or
//! enough battery.
// SPDX-License-Identifier: GPL-2.0-only
use super::super::sys::{get, isdigit, read};
use crate::commands::power::battery::interpolate_int;
use serde_json::{json, Value};
use y2p_core::{
    py::{exact_int, PyValue},
    Root,
};

// On battery: at least the percentage the plan named, read like the shown one (stock
// oam_init: the PMIC's power-on OCV, latched before the boot load pulls the voltage down).
const MIN_PERCENT: f64 = 25.0;

/// A battery profile table `[[uv, value], ...]`: 2..128 rows, strictly ascending voltage, bounded
/// values (increasing too when `increasing`), as the power daemon accepts it.
fn ascending_table(points: Option<&Value>, lower: i64, upper: i64, increasing: bool) -> bool {
    let Some(Value::Array(points)) = points else {
        return false;
    };
    if !(2..=128).contains(&points.len()) {
        return false;
    }
    let mut previous = (0i64, lower - 1);
    for point in points {
        let Some([uv, value]) = point
            .as_array()
            .map(Vec::as_slice)
            .and_then(|p| <&[Value; 2]>::try_from(p).ok())
        else {
            return false;
        };
        let (Some(uv), Some(value)) = (exact_int(Some(uv)), exact_int(Some(value))) else {
            return false;
        };
        if !(2_500_000..=4_300_000).contains(&uv)
            || !(lower..=upper).contains(&value)
            || uv <= previous.0
            || (increasing && value <= previous.1)
        {
            return false;
        }
        previous = (uv, value);
    }
    true
}

/// The voltage → percent table of the battery profile the power daemon uses (the calibrated one on
/// Y2DATA, else the image's), or `None` for no valid profile.
fn load_profile(root: &Root) -> Option<Vec<(i64, i64)>> {
    let value = match root.json("/data/system/platform/battery-profile.json") {
        None | Some(Value::Null) => root.json("/etc/y2linux/battery-profile.json"),
        some => some,
    };
    let Some(Value::Object(value)) = value else {
        return None;
    };
    if !get(&value, "schema").py_eq(&json!(1)) {
        return None;
    }
    let points = value.get("voltage_soc");
    if !ascending_table(points, 0, 100, true) {
        return None;
    }
    let points: Vec<(i64, i64)> = points?
        .as_array()?
        .iter()
        .filter_map(|p| Some((p[0].as_i64()?, p[1].as_i64()?)))
        .collect();
    if points.first()?.1 != 0 || points.last()?.1 != 100 {
        return None;
    }
    // Stock r_profile (POWER-FINAL-16): internal resistance by voltage, used
    // with the charger-path current exactly where stock oam_run uses it.
    let resistance = value.get("voltage_resistance_milliohm");
    if resistance.is_some_and(|r| !r.is_null()) && !ascending_table(resistance, 20, 1000, false) {
        return None;
    }
    let capacity = value.get("design_capacity_uah");
    if capacity.is_some_and(|c| !c.is_null())
        && !exact_int(capacity).is_some_and(|c| (100_000..=5_000_000).contains(&c))
    {
        return None;
    }
    for (key, lower, upper) in [
        ("filter_seconds", 1, 300),
        ("rest_seconds", 10, 3600),
        ("rest_band_uv", 1000, 100_000),
        ("charging_offset_uv", 0, 200_000),
        ("seconds_per_percent", 10, 600),
    ] {
        if !exact_int(value.get(key)).is_some_and(|v| (lower..=upper).contains(&v)) {
            return None;
        }
    }
    if !value.get("provisional").is_some_and(Value::is_boolean)
        || !value.get("source").is_some_and(Value::is_string)
    {
        return None;
    }
    Some(points)
}

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
    let profile = load_profile(root);
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
