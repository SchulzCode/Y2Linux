//! The power journal row every suspend receipt writes (`PowerJournal(ctx).observe('Suspend', battery,
//! event='suspend_<stage>')` in `suspend_record.record`): bounded incident history on Y2DATA, compact
//! JSON in insertion order (not sorted), one durable row per stage.
//!
//! A fresh `PowerJournal` changes nothing in `observe_usb` (its first call only takes the UDC
//! signature) and always writes, because an event is given. Every failure is counted and dropped:
//! a journal failure must never delay or disable battery protection.
// SPDX-License-Identifier: GPL-2.0-only
use super::receipt::{Product, Receipt, Summary};
use super::{diagnostics, E};
use serde::Serialize;
use serde_json::Value;
use y2p_core::{
    json::{dumps, Object},
    power_journal::append_bounded,
    Root,
};

pub const PATH: &str = "/data/system/platform/power-journal.jsonl";
// Two bounded files. POWER-FINAL-16: rows grew to ~6.6 KB and refusal rows add to the five-minute
// snapshot, so 2 MiB kept only ~4.5 h (archive 109 lost the morning). 8 MiB keeps more than a day
// per file; no change in write rate.
const LIMIT: u64 = 8_388_608;

/// A plain value in a row: its objects sorted (a dict that came from a sorted record).
pub fn plain(value: Value) -> Value {
    y2p_core::json::sorted(value)
}

/// One `suspend_<stage>` row. `battery` is `/run/y2/power.json`'s `battery` (or `{}`).
pub fn suspend(root: &Root, battery: &Value, event: &str) -> Result<bool, E> {
    let now = super::monotonic();
    // signature = (state, battery.get('status'), battery.get('charging')): outside the try.
    if !battery.is_object() {
        return Err(E::Invalid("invalid_battery_record".to_owned()));
    }
    match row(root, battery, event, now) {
        Ok(written) => Ok(written),
        Err(error) if error.is_recorded_failure() => Ok(false),
        Err(error) => Err(error),
    }
}

/// One `power-journal.jsonl` row of a suspend stage (compact JSON in this order; the power
/// daemon's rows have the same fields).
#[derive(Serialize)]
struct Row<'a> {
    schema: u8,
    event: &'a str,
    wall_time: f64,
    monotonic_s: f64,
    boottime_s: f64,
    boot_id: Option<String>,
    build: Value,
    state: &'static str,
    battery: Value,
    shutdown: Option<Value>,
    usb: Object,
    usb_net: diagnostics::UsbNet,
    charger: Option<String>,
    backlight: Object,
    radios: Object,
    idle_policy: Value,
    idle_counters: Value,
    sleep: Product,
    suspend: Summary,
    kernel_pm: Option<String>,
}

fn row(root: &Root, battery: &Value, event: &str, now: f64) -> Result<bool, E> {
    // Do not create persistent state on an unverified/missing data mount.
    if !y2p_core::mounts::data_ready(root) {
        return Ok(false);
    }
    // A receipt that does not read drops the row (counted, as an OS error is).
    let receipt = Receipt::read(root).map_err(|error| E::value(error.text()))?;
    let boot_id = root.boot_id();
    let crate::commands::power::journal::Devices {
        usb,
        backlight,
        radios,
    } = crate::commands::power::journal::devices(root);
    let usb_net = diagnostics::usb_net(root)?;
    let idle_policy = root
        .json("/run/y2/cpu-idle-policy.json")
        .unwrap_or(Value::Null);
    let idle_counters = Value::Object(crate::commands::power::journal::idle_counters(root));
    let sleep = super::super::sleep::product(&receipt, boot_id.as_deref());
    let row = Row {
        schema: 1,
        event,
        wall_time: super::wall(),
        monotonic_s: now,
        boottime_s: super::boottime(),
        boot_id,
        build: plain(
            root.json("/etc/y2linux/versions.json")
                .unwrap_or(Value::Null),
        ),
        state: "Suspend",
        battery: plain(battery.clone()),
        shutdown: None,
        usb,
        usb_net,
        charger: root.read("/sys/class/power_supply/BAT0/charging_state"),
        backlight,
        radios,
        idle_policy: plain(idle_policy),
        idle_counters,
        sleep,
        suspend: receipt.summary(),
        kernel_pm: root.read_limited("/sys/firmware/y2_pm/state", 8192),
    };
    let encoded = dumps(&row).map_err(|e| E::value(e.to_string()))? + "\n";
    if encoded.len() > 16384 {
        return Err(E::value("power_journal_row_too_large"));
    }
    append_bounded(root, PATH, encoded.as_bytes(), LIMIT, true).map_err(E::from)?;
    Ok(true)
}
