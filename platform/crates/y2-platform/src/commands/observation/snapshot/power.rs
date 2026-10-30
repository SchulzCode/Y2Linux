//! The `power` section: the power supplies, the power daemon's battery record and the sleep receipt.
// SPDX-License-Identifier: GPL-2.0-only
use crate::commands::observation::util::{fields, is_int, name, read};
use crate::reply::Failure;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use y2p_core::{
    json::{Object, ObjectExt},
    py::truthy,
    Root,
};

/// When `/run/y2/power.json` (the power daemon's policy record, renewed every second) was written.
#[derive(Deserialize, Default)]
#[serde(default)]
struct PolicyStamp {
    boot_id: Value,
    monotonic_ns: Value,
}

/// The power daemon's battery reading (power.json `battery`).
#[derive(Deserialize, Default)]
#[serde(default)]
struct Battery {
    current_ua: Value,
    percent: Value,
    source: Value,
    confidence: Value,
    temperature_millicelsius: Value,
}

/// The `power` section (in this key order).
#[derive(Serialize)]
struct Power {
    supplies: Vec<Supply>,
    units: Units,
    battery: Map<String, Value>,
    measured_current_ua: Value,
    soc_percent: Value,
    soc_source: Value,
    soc_confidence: Value,
    pack_temperature: Value,
    unavailable_reason: &'static str,
    normal_boot: Option<i64>,
    low_battery: Map<String, Value>,
    sleep: Map<String, Value>,
}

#[derive(Serialize)]
struct Units {
    voltage: &'static str,
    configured_current: &'static str,
}

/// One power supply's attributes (null where a file is missing), then its name.
#[derive(Serialize)]
struct Supply {
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

pub fn power(root: &Root) -> Result<Value, Failure> {
    let supplies = root
        .glob("/sys/class/power_supply/*")
        .iter()
        .map(|path| Supply::read(path))
        .collect();
    // The policy record counts while the daemon renews it: this boot, within the last 5 s.
    let mut policy = root.json_object("/run/y2/power.json");
    let stamp: PolicyStamp = fields(&policy);
    let fresh = is_int(Some(&stamp.monotonic_ns)).is_some_and(|t| {
        (0..5_000_000_000).contains(&(i128::from(root.clock().monotonic_ns()) - t))
    });
    if !root.is_this_boot(Some(&stamp.boot_id)) || !fresh {
        policy = Object::new()
            .with("state", "Unavailable")
            .with("reason", "policy_not_running_or_stale");
    }
    // No battery is an empty one; a battery that is no object is a broken record.
    let battery = match policy.get("battery") {
        None => Map::new(),
        Some(Value::Object(map)) => map.clone(),
        Some(_) => return Err(Failure::value("invalid_power_record")),
    };
    let reading: Battery = fields(&battery);
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
    let section = Power {
        supplies,
        units: Units {
            voltage: "uV",
            configured_current: "uA",
        },
        battery,
        measured_current_ua: reading.current_ua,
        soc_percent: reading.percent,
        soc_source: reading.source,
        soc_confidence: reading.confidence,
        pack_temperature: reading.temperature_millicelsius,
        unavailable_reason: "not_exposed_by_qualified_y2_battery_driver",
        normal_boot: root.integer("/sys/firmware/y2_boot/normal_boot"),
        low_battery: policy,
        sleep,
    };
    serde_json::to_value(section).map_err(|_| Failure::value("power_section_unserializable"))
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
