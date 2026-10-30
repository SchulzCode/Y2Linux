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
mod diagnostics;
mod sleep;
pub(crate) mod state;
pub(crate) mod usb;

pub use counters::idle_counters;
#[cfg(test)]
pub(crate) use diagnostics::FOLLOW_UP_S;
pub use diagnostics::{usb_net, Diagnostics};

use super::battery::BatteryStatus;
use crate::commands::time::wall;
use serde::Serialize;
use serde_json::Value;
use sleep::sleep_status;
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

/// `ctx.json(name)` without a default.
fn json(root: &Root, name: &str) -> Value {
    root.json(name).unwrap_or(Value::Null)
}

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

/// One `power-journal.jsonl` row, in this key order.
#[derive(Serialize)]
struct Row<'a, B: BatteryRow> {
    schema: i64,
    event: &'a str,
    wall_time: f64,
    monotonic_s: f64,
    boottime_s: f64,
    boot_id: Option<String>,
    build: Value,
    state: &'a str,
    battery: &'a B,
    shutdown: Option<&'a Object>,
    usb: Object,
    usb_net: Object,
    charger: Option<String>,
    backlight: Object,
    radios: Object,
    idle_policy: Value,
    idle_counters: Object,
    sleep: Object,
    // Preserve completed intervals when a later refusal replaces
    // suspend-last.json. This shares the existing event cadence.
    suspend: Object,
    kernel_pm: Option<String>,
}

/// What a journal row (this daemon's and the suspend stages') records of the devices: each USB
/// controller's state, each backlight's `bl_power`, each radio's type and soft block.
pub(crate) struct Devices {
    pub usb: Object,
    pub backlight: Object,
    pub radios: Object,
}

pub(crate) fn devices(root: &Root) -> Devices {
    let limit = y2p_core::root::READ_LIMIT;
    let mut usb = Object::new();
    for path in root.glob("/sys/class/udc/*") {
        usb.set(&glob_name(&path), read(&path.join("state"), limit));
    }
    let mut backlight = Object::new();
    for path in root.glob("/sys/class/backlight/*/bl_power") {
        backlight.set(&parent_name(&path), read(&path, limit));
    }
    let mut radios = Object::new();
    for path in root.glob("/sys/class/rfkill/*") {
        radios.set(
            &glob_name(&path),
            Object::new()
                .with("type", read(&path.join("type"), limit))
                .with("blocked", read(&path.join("soft"), limit)),
        );
    }
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
        shutdown: Option<&Object>,
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
        shutdown: Option<&Object>,
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
        match self.row(root, state, battery, shutdown, event, safety, now) {
            Ok(Some(row)) => match self.append(root, &row, durable) {
                Ok(()) => true,
                Err(Dropped) => {
                    // A journal failure must never delay or disable battery protection.
                    self.failures += 1;
                    false
                }
            },
            Ok(None) => false,
            Err(Dropped) => {
                self.failures += 1;
                false
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn row<'a, B: BatteryRow>(
        &self,
        root: &Root,
        state: &'a str,
        battery: &'a B,
        shutdown: Option<&'a Object>,
        event: Option<&'a str>,
        safety: bool,
        now: f64,
    ) -> Result<Option<Row<'a, B>>, Dropped> {
        // Do not create persistent state on an unverified/missing data mount.
        if !data_ready(root) {
            return Ok(None);
        }
        let receipt = match root.json("/data/system/platform/suspend-last.json") {
            Some(Value::Object(o)) => o,
            _ => Object::new(),
        };
        let Devices {
            usb,
            backlight,
            radios,
        } = devices(root);
        let usb_net = usb_net(root)?;
        let boot_id = root.boot_id();
        let idle = idle_counters(root);
        let sleep = sleep_status(&receipt, boot_id.as_deref())?;
        let mut suspend = Object::new();
        for key in [
            "mode",
            "stage",
            "first_entry",
            "resume",
            "cycles",
            "suspended_seconds",
            "maintenance_wakes",
            "error",
        ] {
            suspend.set(key, receipt.value(key));
        }
        // POWER-FINAL-18: 17 lost why two requests were refused.
        suspend.set(
            "reason",
            match receipt.value("product") {
                Value::Object(p) => p.value("reason"),
                _ => Value::Null, // a falsy product (`or {}`); a truthy non-dict was dropped above
            },
        );
        Ok(Some(Row {
            schema: 1,
            event: event.filter(|e| !e.is_empty()).unwrap_or(if safety {
                "state_change"
            } else {
                "snapshot"
            }),
            wall_time: wall(),
            monotonic_s: now,
            boottime_s: root.clock().boottime_ns() as f64 / 1e9,
            boot_id,
            build: json(root, "/etc/y2linux/versions.json"),
            state,
            battery,
            shutdown,
            usb,
            usb_net,
            charger: root.read("/sys/class/power_supply/BAT0/charging_state"),
            backlight,
            radios,
            idle_policy: json(root, "/run/y2/cpu-idle-policy.json"),
            idle_counters: idle,
            sleep,
            suspend,
            kernel_pm: root.read_limited("/sys/firmware/y2_pm/state", 8192),
        }))
    }

    fn append(&self, root: &Root, row: &impl Serialize, durable: bool) -> Result<(), Dropped> {
        let encoded = dumps(row).map_err(|_| Dropped)? + "\n";
        if encoded.len() > ROW {
            return Err(Dropped); // power_journal_row_too_large
        }
        append_bounded(root, PATH, encoded.as_bytes(), LIMIT, durable).map_err(|_| Dropped)
    }
}

#[cfg(test)]
#[path = "journal_tests.rs"]
mod journal_tests;
