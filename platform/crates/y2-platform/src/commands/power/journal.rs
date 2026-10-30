//! `power_journal.PowerJournal`: bounded incident history, driven by the existing safety sample; no new
//! timer. With it the parts of `sleep_diagnostics` the power sample drives (USB state change rows,
//! POWER-FINAL-14) and `sleep.status` as a journal row holds it.
//!
//! Rows are compact JSON lines, not sorted: fields in their declared order, records read from files
//! in their file order (the UserReports analyses and the report scrubber read them). A journal
//! failure never delays or disables battery protection: it is counted and the row dropped.
//!
//! The idle counters are counters.rs, the USB rows of the sleep diagnostics diagnostics.rs, the
//! sleep state sleep.rs.
// SPDX-License-Identifier: GPL-2.0-only
mod counters;
pub(crate) mod diagnostics;
mod sleep;
pub(crate) mod state;
pub(crate) mod usb;

pub use counters::{idle_counters, IdleCounters};
pub use diagnostics::Diagnostics;
#[cfg(test)]
pub(crate) use diagnostics::FOLLOW_UP_S;

use super::battery::BatteryStatus;
use super::coordinator::Intent;
use crate::commands::suspend::receipt::{Product, Summary};
use crate::commands::time::wall;
use serde::Serialize;
use serde_json::Value;
use sleep::sleep_status;
use std::collections::BTreeMap;
use y2p_core::mounts::data_ready;
use y2p_core::paths::{name as glob_name, parent_name};
use y2p_core::{
    json::dumps,
    json::{Object, ObjectExt},
    power_journal::append_bounded,
    root::read,
    Root,
};

pub const PATH: &str = "/data/system/platform/power-journal.jsonl";
const PERIOD: f64 = 300.0;
// Two bounded files. POWER-FINAL-16: rows grew to ~6.6 KB and refusal rows add
// to the five-minute snapshot, so 2 MiB kept only ~4.5 h (archive 109 lost the
// morning). 8 MiB keeps more than a day per file; no change in write rate.
const LIMIT: u64 = 8388608;
const ROW: usize = 16384;

/// A row that was not written (a file error, an unreadable record, a row too large): counted,
/// never passed on.
#[derive(Debug)]
pub struct Dropped;

/// A battery reading as a journal row holds it (in its own key order), and the two fields whose
/// change is a state change.
pub trait BatteryRow: Serialize {
    fn status(&self) -> Value;
    fn charging(&self) -> Value;
}

impl BatteryRow for BatteryStatus {
    fn status(&self) -> Value {
        Value::from(self.status.clone())
    }

    fn charging(&self) -> Value {
        Value::Bool(self.charging)
    }
}

/// A partial reading (the journal's own and the sleep diagnostics' tests): what it has.
impl BatteryRow for Object {
    fn status(&self) -> Value {
        self.value("status")
    }

    fn charging(&self) -> Value {
        self.value("charging")
    }
}

/// A battery reading a suspend stage's row holds: power.json's `battery` as it is.
impl BatteryRow for Value {
    fn status(&self) -> Value {
        self.get("status").cloned().unwrap_or(Value::Null)
    }

    fn charging(&self) -> Value {
        self.get("charging").cloned().unwrap_or(Value::Null)
    }
}

/// One `power-journal.jsonl` row (the power daemon's and a suspend stage's), in this key order.
#[derive(Serialize)]
pub(crate) struct Row<'a, B: BatteryRow> {
    schema: i64,
    event: &'a str,
    wall_time: f64,
    monotonic_s: f64,
    boottime_s: f64,
    boot_id: Option<String>,
    /// /etc/y2linux/versions.json as it is.
    build: Value,
    state: &'a str,
    battery: &'a B,
    shutdown: Option<&'a Intent>,
    #[serde(flatten)]
    devices: Devices,
    usb_net: usb::UsbNet,
    charger: Option<String>,
    idle_policy: Option<crate::commands::cpu_idle::Policy>,
    idle_counters: IdleCounters,
    sleep: Product,
    // Preserve completed intervals when a later refusal replaces
    // suspend-last.json. This shares the existing event cadence.
    suspend: Summary,
    kernel_pm: Option<String>,
}

/// A journal row now (on a verified data mount: the caller checks). A receipt that does not read
/// or a USB state that does not drops the row.
pub(crate) fn row<'a, B: BatteryRow>(
    root: &Root,
    event: &'a str,
    state: &'a str,
    battery: &'a B,
    shutdown: Option<&'a Intent>,
    now: f64,
) -> Result<Row<'a, B>, Dropped> {
    let usb_net = usb::usb_net(root).map_err(|_| Dropped)?;
    let boot_id = root.boot_id();
    let idle_counters = idle_counters(root);
    let (receipt, sleep) = sleep_status(root, boot_id.as_deref())?;
    Ok(Row {
        schema: 1,
        event,
        wall_time: wall(),
        monotonic_s: now,
        boottime_s: root.clock().boottime_ns() as f64 / 1e9,
        boot_id,
        build: root
            .json("/etc/y2linux/versions.json")
            .unwrap_or(Value::Null),
        state,
        battery,
        shutdown,
        devices: devices(root),
        usb_net,
        charger: root.read("/sys/class/power_supply/BAT0/charging_state"),
        idle_policy: crate::commands::cpu_idle::Policy::read(root),
        idle_counters,
        sleep,
        suspend: receipt.summary(),
        kernel_pm: root.read_limited("/sys/firmware/y2_pm/state", 8192),
    })
}

/// Append a row to the journal (`durable`: synced); a row over 16 KiB is dropped.
pub(crate) fn append(root: &Root, row: &impl Serialize, durable: bool) -> Result<(), Dropped> {
    let encoded = dumps(row).map_err(|_| Dropped)? + "\n";
    if encoded.len() > ROW {
        return Err(Dropped); // power_journal_row_too_large
    }
    append_bounded(root, PATH, encoded.as_bytes(), LIMIT, durable).map_err(|_| Dropped)
}

/// What a journal row records of the devices: each USB controller's state, each backlight's
/// `bl_power`, each radio's type and soft block.
#[derive(Serialize, Debug)]
pub(crate) struct Devices {
    pub usb: BTreeMap<String, Option<String>>,
    pub backlight: BTreeMap<String, Option<String>>,
    pub radios: BTreeMap<String, Radio>,
}

/// An rfkill radio.
#[derive(Serialize, Debug)]
pub(crate) struct Radio {
    #[serde(rename = "type")]
    kind: Option<String>,
    blocked: Option<String>,
}

pub(crate) fn devices(root: &Root) -> Devices {
    let limit = y2p_core::root::READ_LIMIT;
    let usb = root
        .glob("/sys/class/udc/*")
        .iter()
        .map(|path| (glob_name(path), read(&path.join("state"), limit)))
        .collect();
    let backlight = root
        .glob("/sys/class/backlight/*/bl_power")
        .iter()
        .map(|path| (parent_name(path), read(path, limit)))
        .collect();
    let radios = root
        .glob("/sys/class/rfkill/*")
        .iter()
        .map(|path| {
            let radio = Radio {
                kind: read(&path.join("type"), limit),
                blocked: read(&path.join("soft"), limit),
            };
            (glob_name(path), radio)
        })
        .collect();
    Devices {
        usb,
        backlight,
        radios,
    }
}

#[derive(Debug, Default)]
pub struct PowerJournal {
    next_snapshot: f64,
    next_event: f64,
    signature: Option<(String, Value, Value)>,
    pub failures: u64,
    pub diagnostics: Diagnostics,
}

impl PowerJournal {
    pub fn new() -> Self {
        Self::default()
    }

    /// `observe(state, battery, shutdown, event)`: a row on an event, a safety state change, every
    /// 300 s, or a state change at most once a minute.
    pub fn observe(
        &mut self,
        root: &Root,
        state: &str,
        battery: &impl BatteryRow,
        shutdown: Option<&Intent>,
        event: Option<&str>,
    ) -> bool {
        self.observe_at(
            root,
            state,
            battery,
            shutdown,
            event,
            root.clock().monotonic(),
        )
    }

    /// `observe(..., now=now)`: at this CLOCK_MONOTONIC time (seconds).
    pub fn observe_at(
        &mut self,
        root: &Root,
        state: &str,
        battery: &impl BatteryRow,
        shutdown: Option<&Intent>,
        event: Option<&str>,
        now: f64,
    ) -> bool {
        // POWER-FINAL-14: persist USB state changes (and one follow-up) so a
        // USB failure after a wake leaves evidence across a restart.
        self.diagnostics.observe_usb(root, now);
        let signature = (state.to_owned(), battery.status(), battery.charging());
        let changed = self
            .signature
            .as_ref()
            .is_none_or(|s| s.0 != signature.0 || s.1 != signature.1 || s.2 != signature.2);
        let safety = state == "ShutdownPending" && changed;
        if !(event.is_some_and(|e| !e.is_empty())
            || safety
            || now >= self.next_snapshot
            || (changed && now >= self.next_event))
        {
            return false;
        }
        self.next_snapshot = now + PERIOD;
        self.next_event = now + 60.0;
        self.signature = Some(signature);
        let durable = event.is_some_and(|e| !e.is_empty()) || safety;
        // Do not create persistent state on an unverified/missing data mount.
        if !data_ready(root) {
            return false;
        }
        let event = event.filter(|e| !e.is_empty()).unwrap_or(if safety {
            "state_change"
        } else {
            "snapshot"
        });
        match row(root, event, state, battery, shutdown, now)
            .and_then(|row| append(root, &row, durable))
        {
            Ok(()) => true,
            Err(Dropped) => {
                // A journal failure must never delay or disable battery protection.
                self.failures += 1;
                false
            }
        }
    }
}

#[cfg(test)]
#[path = "journal_tests.rs"]
mod journal_tests;
