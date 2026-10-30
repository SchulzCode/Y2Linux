//! What a sample publishes and what survives a restart: the reading in power.json and the journal,
//! and `battery-ui.json`.
// SPDX-License-Identifier: GPL-2.0-only
use serde::Serialize;
use serde_json::Value;
use y2p_core::py::PyValue;

// POWER-FINAL-28: the displayed percentage survives a restart, as stock keeps
// UI_SOC in an MT6323 RTC spare register (set_rtc_spare_fg_value, ledger F29).
pub const UI_STATE: &str = "/data/system/platform/battery-ui.json";

pub use y2p_contract::power::{BatterySource, BatteryStatus, Confidence, Estimate};

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
