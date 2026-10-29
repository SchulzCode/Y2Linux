//! `battery.py`: battery SOC policy; estimates never create measured current or temperature.
//! The profile (profile.rs), the estimate (soc.rs), the displayed percentage (display.rs) and the
//! published records (status.rs); here the BAT0 sample that feeds them.
// SPDX-License-Identifier: GPL-2.0-only
mod display;
mod profile;
mod soc;
mod status;

pub use profile::load_profile;
pub use soc::Soc;
pub use status::{BatteryStatus, DisplayRecord, UI_STATE};

use y2p_core::{
    power_text::{python_split, sense_current},
    root::python_int,
    Root,
};

// Stock dod_init (0xc039be10) keeps the saved UI_SOC whatever the boot sample
// says after boot_reason 3..6 (BR_WDT, BR_WDT_BY_PASS_PWK, BR_TOOL_BY_PASS_PWK,
// BR_2SEC_REBOOT). The PMIC stayed on, so 0x724 still holds the OCV of the last
// cold start (F54, 98499743: an update restart at 95 % showed 13 % at 4.02 V).
pub const BOOT_METADATA: &str = "/sys/firmware/y2_boot/metadata";
const WARM_BOOT_REASONS: [i64; 4] = [3, 4, 5, 6];

/// `boot_kind(text)`: 'warm' (SoC reset, PMIC on), 'charged' (LK's charging screen ran first: charge
/// added while the saved value waited) or 'cold'; `None` if unknown.
pub fn boot_kind(text: Option<&str>) -> Option<&'static str> {
    let mut fields: Vec<(&str, &str)> = Vec::new();
    for field in python_split(text.unwrap_or_default()) {
        if let Some((key, value)) = field.split_once('=') {
            match fields.iter_mut().find(|(k, _)| *k == key) {
                Some(entry) => entry.1 = value,
                None => fields.push((key, value)),
            }
        }
    }
    let get = |key: &str| fields.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
    if get("valid") != Some("1") {
        return None;
    }
    if get("offline") == Some("1") {
        return Some("charged");
    }
    let reason = python_int(get("boot_reason")?)?;
    Some(if WARM_BOOT_REASONS.contains(&reason) {
        "warm"
    } else {
        "cold"
    })
}

/// One BAT0 reading (`sample()`'s arguments to `Soc.observe`).
#[derive(Clone, Debug, Default)]
pub struct Reading {
    pub voltage: Option<i64>,
    pub present: Option<i64>,
    pub status: Option<String>,
    pub hardware_percent: Option<i64>,
    pub current: Option<i64>,
    pub temperature: Option<i64>,
    pub counter: Option<i64>,
    pub charge_current: Option<i64>,
    pub boot_ocv: Option<i64>,
    pub boot: Option<&'static str>,
}

/// `sample(ctx, engine)`: one BAT0 reading on CLOCK_BOOTTIME (physical elapsed time incl. suspend).
pub fn sample(root: &Root, engine: &mut Soc) -> BatteryStatus {
    let now = root.clock().boottime_ns() as f64 / 1e9;
    sample_at(root, engine, now)
}

pub fn sample_at(root: &Root, engine: &mut Soc, now: f64) -> BatteryStatus {
    let base = "/sys/class/power_supply/BAT0/";
    let int = |name: &str| root.integer(&format!("{base}{name}"));
    // Read BAT0 only: PMIC/CPU temperatures and USB configured limits cannot
    // become pack sensor values. Missing hardware properties stay null.
    let reading = Reading {
        voltage: int("voltage_now"),
        present: int("present"),
        status: root.read(&format!("{base}status")),
        hardware_percent: int("capacity"),
        current: int("current_now"),
        temperature: int("temp").map(|t| t * 100),
        counter: int("charge_counter"),
        charge_current: sense_current(root.read(&format!("{base}charging_state")).as_deref()),
        boot_ocv: int("voltage_ocv"),
        boot: if engine.percent.is_none() {
            boot_kind(root.read(BOOT_METADATA).as_deref())
        } else {
            None
        },
    };
    engine.observe(now, &reading)
}

#[cfg(test)]
#[path = "battery_soc_tests.rs"]
mod soc_tests;

#[cfg(test)]
#[path = "battery_tests.rs"]
mod battery_tests;
