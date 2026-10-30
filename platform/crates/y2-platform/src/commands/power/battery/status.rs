//! What a sample publishes and what survives a restart: the reading in power.json and the journal,
//! and `battery-ui.json`.
// SPDX-License-Identifier: GPL-2.0-only
use serde::Serialize;
use serde_json::Value;
use y2p_core::py::PyValue;

// POWER-FINAL-28: the displayed percentage survives a restart, as stock keeps
// UI_SOC in an MT6323 RTC spare register (set_rtc_spare_fg_value, ledger F29).
pub const UI_STATE: &str = "/data/system/platform/battery-ui.json";

/// Where a sample's percentage comes from (`battery.source`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BatterySource {
    Unavailable,
    FuelGauge,
    Hybrid,
    StockOam,
    VoltageEstimate,
}

/// How far a sample's percentage can be trusted (`battery.confidence`): the low-battery policy acts
/// on a percentage only from `hardware` or `calibrated_estimate`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Unavailable,
    Hardware,
    CalibratedEstimate,
    Provisional,
}

/// What one sample publishes (`battery` in power.json and in the journal rows), in this key order:
/// the reading, then for a voltage estimate the estimate's own fields.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BatteryStatus {
    pub percent: Option<i64>,
    pub source: BatterySource,
    pub confidence: Confidence,
    pub status: Option<String>,
    pub charging: bool,
    pub voltage_uv: Option<i64>,
    pub current_ua: Option<i64>,
    pub temperature_millicelsius: Option<i64>,
    pub charge_counter_uah: Option<i64>,
    pub provisional: bool,
    pub resting: bool,
    #[serde(flatten)]
    pub estimate: Option<Estimate>,
}

impl Default for BatteryStatus {
    /// No usable reading: no percentage, nothing measured.
    fn default() -> Self {
        Self {
            percent: None,
            source: BatterySource::Unavailable,
            confidence: Confidence::Unavailable,
            status: None,
            charging: false,
            voltage_uv: None,
            current_ua: None,
            temperature_millicelsius: None,
            charge_counter_uah: None,
            provisional: true,
            resting: false,
            estimate: None,
        }
    }
}

/// The voltage estimate's fields of a [`BatteryStatus`]; the capacity pair only with a design capacity.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Estimate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub design_capacity_uah: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining_uah_estimate: Option<i64>,
    pub estimate_percent: i64,
    pub anchor_percent: Option<f64>,
    pub full_latched: bool,
    pub filtered_voltage_uv: i64,
    pub start: Option<String>,
    pub calibration_source: String,
}

/// `battery-ui.json`: the displayed percentage kept across a restart.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct DisplayRecord {
    pub schema: i64,
    pub ui_percent: i64,
    pub anchor_percent: Option<f64>,
}

impl DisplayRecord {
    pub fn new((ui_percent, anchor_percent): (i64, Option<f64>)) -> Self {
        Self {
            schema: 1,
            ui_percent,
            anchor_percent,
        }
    }
}

/// What the first sample may restore from `battery-ui.json` (anything else in it is ignored).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Saved {
    /// An exact int (a float or a bool is not one).
    pub(super) ui_percent: Option<i64>,
    /// Any JSON number (not a bool).
    pub(super) anchor_percent: Option<f64>,
}

impl Saved {
    pub(super) fn of(value: Option<Value>) -> Option<Saved> {
        let Some(Value::Object(saved)) = value else {
            return None;
        };
        Some(Saved {
            ui_percent: saved.get("ui_percent").and_then(Value::exact_int),
            anchor_percent: match saved.get("anchor_percent") {
                Some(Value::Number(n)) => Some(n.as_f64().unwrap_or(f64::NAN)),
                _ => None,
            },
        })
    }
}
