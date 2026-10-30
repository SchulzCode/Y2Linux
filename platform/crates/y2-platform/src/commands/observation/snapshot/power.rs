//! The `power` section: the power supplies, the power daemon's battery record and the sleep receipt.
// SPDX-License-Identifier: GPL-2.0-only
use crate::commands::observation::util::{name, read};
use crate::commands::power::{
    battery::{BatterySource, BatteryStatus, Confidence},
    published::PowerRecord,
};
use crate::reply::Failure;
use serde::Serialize;
use y2p_core::Root;

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
    pub sleep: crate::commands::suspend::receipt::Product,
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
    let sleep = match crate::commands::suspend::receipt::Receipt::read(root) {
        Ok(receipt) => crate::commands::sleep::product(&receipt, root.boot_id().as_deref()),
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
