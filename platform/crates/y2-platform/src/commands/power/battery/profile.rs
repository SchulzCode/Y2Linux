//! The battery profile: the voltage/SOC curve and stock r_profile, validated as loaded (an invalid
//! override fails closed: no estimate), and interpolation on them.
// SPDX-License-Identifier: GPL-2.0-only
use serde_json::Value;
use y2p_core::{
    py::{eq_int, exact_int},
    Root,
};

pub const PROFILE: &str = "/etc/y2linux/battery-profile.json";
pub const OVERRIDE: &str = "/data/system/platform/battery-profile.json";

pub type Table = Vec<(i64, i64)>;

/// A validated battery profile (`load_profile`).
#[derive(Clone, Debug)]
pub struct Profile {
    pub voltage_soc: Table,
    /// Stock r_profile (POWER-FINAL-16).
    pub resistance: Option<Table>,
    pub design_capacity_uah: Option<i64>,
    pub filter_seconds: i64,
    pub rest_seconds: i64,
    pub rest_band_uv: i64,
    pub charging_offset_uv: i64,
    pub seconds_per_percent: i64,
    pub provisional: bool,
    pub source: String,
    /// `p.get('calibrated_resistance_milliohm')` when it is an int.
    pub calibrated_resistance: Option<i64>,
}

/// `[[uv, value], ...]`: 2..128 rows, strictly ascending voltage, bounded values.
pub fn ascending_table(
    points: Option<&Value>,
    lower: i64,
    upper: i64,
    increasing: bool,
) -> Option<Table> {
    let rows = points?.as_array()?;
    if !(2..=128).contains(&rows.len()) {
        return None;
    }
    let mut previous = (0, lower - 1);
    let mut table = Vec::new();
    for row in rows {
        let pair = row.as_array().filter(|p| p.len() == 2)?;
        let (uv, value) = (exact_int(Some(&pair[0]))?, exact_int(Some(&pair[1]))?);
        if !(2500000..=4300000).contains(&uv)
            || !(lower..=upper).contains(&value)
            || uv <= previous.0
            || (increasing && value <= previous.1)
        {
            return None;
        }
        previous = (uv, value);
        table.push(previous);
    }
    Some(table)
}

pub fn load_profile(root: &Root) -> Option<Profile> {
    let value = match root.json(OVERRIDE) {
        None | Some(Value::Null) => root.json(PROFILE),
        found => found,
    };
    let value = value?;
    let value = value.as_object()?;
    if !eq_int(value.get("schema"), 1) {
        return None;
    }
    let voltage_soc = ascending_table(value.get("voltage_soc"), 0, 100, true)?;
    if voltage_soc[0].1 != 0 || voltage_soc[voltage_soc.len() - 1].1 != 100 {
        return None;
    }
    let resistance = match value.get("voltage_resistance_milliohm") {
        None | Some(Value::Null) => None,
        table => Some(ascending_table(table, 20, 1000, false)?),
    };
    let design_capacity_uah = match value.get("design_capacity_uah") {
        None | Some(Value::Null) => None,
        capacity => Some(exact_int(capacity).filter(|c| (100000..=5000000).contains(c))?),
    };
    let bounded = |name: &str, lower: i64, upper: i64| {
        exact_int(value.get(name)).filter(|v| (lower..=upper).contains(v))
    };
    let filter_seconds = bounded("filter_seconds", 1, 300)?;
    let rest_seconds = bounded("rest_seconds", 10, 3600)?;
    let rest_band_uv = bounded("rest_band_uv", 1000, 100000)?;
    let charging_offset_uv = bounded("charging_offset_uv", 0, 200000)?;
    let seconds_per_percent = bounded("seconds_per_percent", 10, 600)?;
    let provisional = value.get("provisional")?.as_bool()?;
    let source = value.get("source")?.as_str()?.to_owned();
    Some(Profile {
        voltage_soc,
        resistance,
        design_capacity_uah,
        filter_seconds,
        rest_seconds,
        rest_band_uv,
        charging_offset_uv,
        seconds_per_percent,
        provisional,
        source,
        calibrated_resistance: exact_int(value.get("calibrated_resistance_milliohm")),
    })
}

/// Inverse of `interpolate()` on the ascending voltage/SOC curve.
pub fn voltage_at(points: &Table, percent: f64) -> f64 {
    if percent <= points[0].1 as f64 {
        return points[0].0 as f64;
    }
    for pair in points.windows(2) {
        let ((low, a), (high, b)) = (pair[0], pair[1]);
        if percent <= b as f64 {
            return low as f64 + (high - low) as f64 * (percent - a as f64) / (b - a) as f64;
        }
    }
    points[points.len() - 1].0 as f64
}

/// `interpolate(points, uv)` for a float voltage.
pub fn interpolate(points: &Table, uv: f64) -> f64 {
    if uv <= points[0].0 as f64 {
        return points[0].1 as f64;
    }
    for pair in points.windows(2) {
        let ((low, a), (high, b)) = (pair[0], pair[1]);
        if uv <= high as f64 {
            return a as f64 + (b - a) as f64 * (uv - low as f64) / (high - low) as f64;
        }
    }
    points[points.len() - 1].1 as f64
}

/// `interpolate(points, uv)` for an int voltage: `(b - a) * (uv - low) / (high - low)` is int/int true
/// division in Python, exact operands divided once.
pub fn interpolate_int(points: &Table, uv: i64) -> f64 {
    if uv <= points[0].0 {
        return points[0].1 as f64;
    }
    for pair in points.windows(2) {
        let ((low, a), (high, b)) = (pair[0], pair[1]);
        if uv <= high {
            return a as f64 + ((b - a) * (uv - low)) as f64 / (high - low) as f64;
        }
    }
    points[points.len() - 1].1 as f64
}
