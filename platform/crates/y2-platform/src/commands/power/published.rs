//! What other components read of `/run/y2/power.json` (the power daemon's `PowerRecord`, renewed at
//! least every 2 s): suspend admission, the SD card repair's battery check and the update's battery
//! gate. Read leniently: an odd field reads as absent, and each reader keeps its own rule for that.
// SPDX-License-Identifier: GPL-2.0-only
use serde::{Deserialize, Deserializer};
use serde_json::Value;
pub use y2p_contract::power::PowerRecord;
use y2p_core::{
    lenient::{self, BootId},
    py, Root,
};

/// The record at `/run/y2/power.json`; none when missing or not of this shape.
pub fn record(root: &Root) -> Option<PowerRecord> {
    serde_json::from_str(&root.read("/run/y2/power.json")?).ok()
}

/// The record's fields these readers use.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct PowerState {
    pub boot_id: BootId,
    /// When it was sampled: an integer (any size a JSON integer has), else none.
    #[serde(deserialize_with = "wide_integer")]
    pub monotonic_ns: Option<i128>,
    #[serde(deserialize_with = "lenient::text")]
    pub state: Option<String>,
    pub battery: Battery,
}

/// The record's `battery`: the daemon writes an object; a record of another kind holds something else.
#[derive(Default)]
pub enum Battery {
    /// Missing, null or another value Python takes as false.
    #[default]
    Absent,
    /// A value that is no object but true for Python (a non-empty text or list, a number ≠ 0).
    Odd,
    Reading(BatteryReading),
}

/// The battery reading's fields these readers use.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct BatteryReading {
    #[serde(deserialize_with = "lenient::integer")]
    pub filtered_voltage_uv: Option<i64>,
    #[serde(deserialize_with = "lenient::integer")]
    pub percent: Option<i64>,
    #[serde(deserialize_with = "lenient::is_true")]
    pub charging: bool,
    #[serde(deserialize_with = "lenient::text")]
    pub status: Option<String>,
    #[serde(deserialize_with = "lenient::text")]
    pub confidence: Option<String>,
}

impl PowerState {
    /// The record at `/run/y2/power.json` (all absent when it is missing or no object).
    pub fn read(root: &Root) -> PowerState {
        lenient::read(root.json_object("/run/y2/power.json"))
    }

    /// Whether the record was written this boot.
    pub fn is_this_boot(&self, root: &Root) -> bool {
        self.boot_id.is(root.boot_id().as_deref())
    }
}

impl Battery {
    /// The reading's fields, all absent for anything but an object.
    pub fn reading(&self) -> &BatteryReading {
        static NONE: BatteryReading = BatteryReading {
            filtered_voltage_uv: None,
            percent: None,
            charging: false,
            status: None,
            confidence: None,
        };
        match self {
            Battery::Reading(reading) => reading,
            _ => &NONE,
        }
    }
}

impl BatteryReading {
    /// Charging, or the charger reports Charging or Full.
    pub fn charging_or_full(&self) -> bool {
        self.charging || matches!(self.status.as_deref(), Some("Charging" | "Full"))
    }
}

impl<'de> Deserialize<'de> for Battery {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Value::deserialize(deserializer)? {
            Value::Object(object) => Battery::Reading(lenient::read(object)),
            other if py::truthy(&other) => Battery::Odd,
            _ => Battery::Absent,
        })
    }
}

fn wide_integer<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<i128>, D::Error> {
    Ok(match Value::deserialize(deserializer)? {
        Value::Number(n) => n
            .as_i64()
            .map(i128::from)
            .or_else(|| n.as_u64().map(i128::from)),
        _ => None,
    })
}
