//! The battery profile: the voltage/SOC curve and stock r_profile, validated as loaded (an invalid
//! override fails closed: no estimate), and interpolation on them.
// SPDX-License-Identifier: GPL-2.0-only
use serde::{de::DeserializeOwned, Deserialize, Deserializer};
use serde_json::Value;
use y2p_core::{lenient, Root};

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

/// `[[uv, value], ...]`: 2..128 rows, strictly ascending voltage, bounded values (increasing too
/// when `increasing`).
fn ascending_table(rows: Table, lower: i64, upper: i64, increasing: bool) -> Option<Table> {
    if !(2..=128).contains(&rows.len()) {
        return None;
    }
    let mut previous = (0, lower - 1);
    for &(uv, value) in &rows {
        if !(2500000..=4300000).contains(&uv)
            || !(lower..=upper).contains(&value)
            || uv <= previous.0
            || (increasing && value <= previous.1)
        {
            return None;
        }
        previous = (uv, value);
    }
    Some(rows)
}

/// The profile as stored: integers only where it has integers (a float or a flag is none), a table
/// only of `[int, int]` rows.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Stored {
    #[serde(deserialize_with = "lenient::integer")]
    schema: Option<i64>,
    #[serde(deserialize_with = "lenient::or_none")]
    voltage_soc: Option<Table>,
    #[serde(deserialize_with = "optional")]
    voltage_resistance_milliohm: Option<Option<Table>>,
    #[serde(deserialize_with = "optional")]
    design_capacity_uah: Option<Option<i64>>,
    #[serde(deserialize_with = "lenient::integer")]
    filter_seconds: Option<i64>,
    #[serde(deserialize_with = "lenient::integer")]
    rest_seconds: Option<i64>,
    #[serde(deserialize_with = "lenient::integer")]
    rest_band_uv: Option<i64>,
    #[serde(deserialize_with = "lenient::integer")]
    charging_offset_uv: Option<i64>,
    #[serde(deserialize_with = "lenient::integer")]
    seconds_per_percent: Option<i64>,
    #[serde(deserialize_with = "lenient::or_none")]
    provisional: Option<bool>,
    #[serde(deserialize_with = "lenient::text")]
    source: Option<String>,
    #[serde(deserialize_with = "lenient::integer")]
    calibrated_resistance_milliohm: Option<i64>,
}

/// An optional part: missing or `null` is none (`None`); anything else must read as `T`
/// (`Some(None)` when it does not).
fn optional<'de, D: Deserializer<'de>, T: DeserializeOwned>(
    deserializer: D,
) -> Result<Option<Option<T>>, D::Error> {
    Ok(match Value::deserialize(deserializer)? {
        Value::Null => None,
        value => Some(T::deserialize(value).ok()),
    })
}

/// The validated profile: the override on Y2DATA when there is one, else the image's; `None` when
/// it is invalid (no estimate rather than a wrong one).
pub fn load_profile(root: &Root) -> Option<Profile> {
    let value = match root.json(OVERRIDE) {
        None | Some(Value::Null) => root.json(PROFILE),
        found => found,
    };
    let Some(Value::Object(value)) = value else {
        return None;
    };
    let stored: Stored = lenient::read(value);
    if stored.schema != Some(1) {
        return None;
    }
    let voltage_soc = ascending_table(stored.voltage_soc?, 0, 100, true)?;
    if voltage_soc[0].1 != 0 || voltage_soc[voltage_soc.len() - 1].1 != 100 {
        return None;
    }
    let resistance = match stored.voltage_resistance_milliohm {
        None => None,
        Some(table) => Some(ascending_table(table?, 20, 1000, false)?),
    };
    let design_capacity_uah = match stored.design_capacity_uah {
        None => None,
        Some(capacity) => Some(capacity.filter(|c| (100000..=5000000).contains(c))?),
    };
    let bounded =
        |value: Option<i64>, lower: i64, upper: i64| value.filter(|v| (lower..=upper).contains(v));
    Some(Profile {
        voltage_soc,
        resistance,
        design_capacity_uah,
        filter_seconds: bounded(stored.filter_seconds, 1, 300)?,
        rest_seconds: bounded(stored.rest_seconds, 10, 3600)?,
        rest_band_uv: bounded(stored.rest_band_uv, 1000, 100000)?,
        charging_offset_uv: bounded(stored.charging_offset_uv, 0, 200000)?,
        seconds_per_percent: bounded(stored.seconds_per_percent, 10, 600)?,
        provisional: stored.provisional?,
        source: stored.source?,
        calibrated_resistance: stored.calibrated_resistance_milliohm,
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
pub fn interpolate_int(points: &[(i64, i64)], uv: i64) -> f64 {
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
