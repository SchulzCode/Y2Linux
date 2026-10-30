//! The `power` section: the power supplies, the power daemon's battery record and the sleep receipt.
// SPDX-License-Identifier: GPL-2.0-only
use crate::commands::observation::util::{name, read};
use crate::commands::power::{
    battery::{BatterySource, BatteryStatus, Confidence},
    published::PowerRecord,
};
use crate::reply::Failure;
use serde::Serialize;
use serde_json::{json, Map, Value};
use y2p_core::{py::truthy, Root};

/// The power daemon's policy record while it renews it, else why it is not shown.
#[derive(Serialize)]
#[serde(untagged)]
pub enum LowBattery {
    Policy(Box<PowerRecord>),
    Unavailable {
        state: &'static str,
        reason: &'static str,
    },
}

/// The `power` section (in this key order).
#[derive(Serialize)]
pub struct Power {
    pub supplies: Vec<Supply>,
    units: Units,
    battery: Option<BatteryStatus>,
    measured_current_ua: Option<i64>,
    soc_percent: Option<i64>,
    soc_source: Option<BatterySource>,
    soc_confidence: Option<Confidence>,
    pack_temperature: Option<i64>,
    unavailable_reason: &'static str,
    normal_boot: Option<i64>,
    low_battery: LowBattery,
    pub sleep: Map<String, Value>,
}

#[derive(Serialize)]
pub struct Units {
    voltage: &'static str,
    configured_current: &'static str,
}

/// One power supply's attributes (null where a file is missing), then its name.
#[derive(Serialize)]
pub struct Supply {
    #[serde(rename = "type")]
    kind: Option<String>,
    status: Option<String>,
    health: Option<String>,
    online: Option<String>,
    present: Option<String>,
    usb_type: Option<String>,
    voltage_now: Option<String>,
    voltage_min_design: Option<String>,
    voltage_max_design: Option<String>,
    constant_charge_current: Option<String>,
    constant_charge_current_max: Option<String>,
    constant_charge_voltage: Option<String>,
    constant_charge_voltage_max: Option<String>,
    input_current_limit: Option<String>,
    name: String,
}

impl Supply {
    fn read(path: &std::path::Path) -> Self {
        let attribute = |field: &str| read(&path.join(field));
        Supply {
            kind: attribute("type"),
            status: attribute("status"),
            health: attribute("health"),
            online: attribute("online"),
            present: attribute("present"),
            usb_type: attribute("usb_type"),
            voltage_now: attribute("voltage_now"),
            voltage_min_design: attribute("voltage_min_design"),
            voltage_max_design: attribute("voltage_max_design"),
            constant_charge_current: attribute("constant_charge_current"),
            constant_charge_current_max: attribute("constant_charge_current_max"),
            constant_charge_voltage: attribute("constant_charge_voltage"),
            constant_charge_voltage_max: attribute("constant_charge_voltage_max"),
            input_current_limit: attribute("input_current_limit"),
            name: name(path),
        }
    }
}

pub fn power(root: &Root) -> Result<Power, Failure> {
    let supplies = root
        .glob("/sys/class/power_supply/*")
        .iter()
        .map(|path| Supply::read(path))
        .collect();
    // The policy record counts while the daemon renews it: this boot, within the last 5 s.
    let now = i128::from(root.clock().monotonic_ns());
    let record = PowerRecord::read(root).filter(|record| {
        record.boot_id.is_some()
            && record.boot_id == root.boot_id()
            && (0..5_000_000_000).contains(&(now - i128::from(record.monotonic_ns)))
    });
    let battery = record.as_ref().map(|record| record.battery.clone());
    let low_battery = match record {
        Some(record) => LowBattery::Policy(Box::new(record)),
        None => LowBattery::Unavailable {
            state: "Unavailable",
            reason: if root.path("/run/y2/power.json").exists() {
                "policy_not_running_or_stale"
            } else {
                "policy_not_running"
            },
        },
    };
    let mut receipt = root.json_object("/data/system/platform/suspend-last.json");
    if let Some(product) = receipt
        .get("product")
        .filter(|p| truthy(p) && !p.is_object())
    {
        let product = pairs(product)?;
        receipt.insert("product".into(), Value::Object(product));
    }
    let sleep = match crate::commands::sleep::product(&receipt, root.boot_id().as_deref()) {
        Ok(sleep) => sleep,
        Err(error @ Failure::Value(_)) => return Err(error),
        Err(_) => return Err(invalid_sleep_receipt()),
    };
    Ok(Power {
        supplies,
        units: Units {
            voltage: "uV",
            configured_current: "uA",
        },
        measured_current_ua: battery.as_ref().and_then(|b| b.current_ua),
        soc_percent: battery.as_ref().and_then(|b| b.percent),
        soc_source: battery.as_ref().map(|b| b.source),
        soc_confidence: battery.as_ref().map(|b| b.confidence),
        pack_temperature: battery.as_ref().and_then(|b| b.temperature_millicelsius),
        battery,
        unavailable_reason: "not_exposed_by_qualified_y2_battery_driver",
        normal_boot: root.integer("/sys/firmware/y2_boot/normal_boot"),
        low_battery,
        sleep,
    })
}

/// A sleep receipt whose product the snapshot cannot show: no object and no list of pairs, a key
/// that is a list or object, a state that is a list or object; or a key that is a number, bool or
/// null, which only refuses where the power section is printed (`printable`).
fn invalid_sleep_receipt() -> Failure {
    Failure::value("invalid_sleep_receipt")
}

/// Where a sleep product's key is no text (a list of pairs with a number key), its entry is kept
/// under this prefix: the snapshot holds it, but printing it is refused (`printable`).
const NON_TEXT_KEY: &str = "\u{0}non-str-key:";

/// The sleep product written as a list of pairs (two-item lists, two-character texts, two-key
/// objects, by their sorted keys), as `dict()` builds it; a pair of the wrong length is the
/// ValueError text `dict()` gives, anything else that is no pair `invalid_sleep_receipt`.
fn pairs(value: &Value) -> Result<Map<String, Value>, Failure> {
    let length = |index: usize, n: usize| {
        Failure::value(format!(
            "dictionary update sequence element #{index} has length {n}; 2 is required"
        ))
    };
    let items = match value {
        Value::Array(items) => items,
        // A text's items are its characters (length 1).
        Value::String(_) => return Err(length(0, 1)),
        _ => return Err(invalid_sleep_receipt()),
    };
    let mut result = Map::new();
    for (index, item) in items.iter().enumerate() {
        let (key, value) = match item {
            Value::Array(pair) if pair.len() == 2 => (pair[0].clone(), pair[1].clone()),
            Value::Array(other) => return Err(length(index, other.len())),
            Value::String(text) => {
                let chars: Vec<char> = text.chars().collect();
                if chars.len() != 2 {
                    return Err(length(index, chars.len()));
                }
                (json!(chars[0].to_string()), json!(chars[1].to_string()))
            }
            Value::Object(map) if map.len() == 2 => {
                let mut keys = map.keys().collect::<Vec<_>>();
                keys.sort();
                (json!(keys[0]), json!(keys[1]))
            }
            Value::Object(map) => return Err(length(index, map.len())),
            _ => return Err(invalid_sleep_receipt()),
        };
        let key = match key {
            Value::String(key) => key,
            Value::Array(_) | Value::Object(_) => return Err(invalid_sleep_receipt()),
            other => format!("{NON_TEXT_KEY}{other}"),
        };
        result.insert(key, value);
    }
    Ok(result)
}

/// Whether `value` holds a sleep product key that is no text.
fn holds_non_text_key(value: &Value) -> bool {
    match value {
        Value::Object(map) => map
            .iter()
            .any(|(k, v)| k.starts_with(NON_TEXT_KEY) || holds_non_text_key(v)),
        Value::Array(items) => items.iter().any(holds_non_text_key),
        _ => false,
    }
}

/// Whether a value holding (part of) a snapshot can be printed: one holding a sleep product key that
/// is no text is refused (`invalid_sleep_receipt`). A section that leaves out the power section
/// prints.
pub fn printable(value: &Map<String, Value>) -> Result<(), Failure> {
    if value.values().any(holds_non_text_key) {
        return Err(invalid_sleep_receipt());
    }
    Ok(())
}

/// `printable` for the power section alone (its sleep product).
pub fn printable_section(power: &Power) -> Result<(), Failure> {
    if power
        .sleep
        .iter()
        .any(|(k, v)| k.starts_with(NON_TEXT_KEY) || holds_non_text_key(v))
    {
        return Err(invalid_sleep_receipt());
    }
    Ok(())
}
