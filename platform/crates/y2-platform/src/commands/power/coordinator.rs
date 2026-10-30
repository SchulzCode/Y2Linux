//! The shutdown coordinator: one intent per boot (`/run/y2/shutdown.json`, resumed after a daemon
//! restart), its grace and Reborn's acknowledgement; the steps that execute it are steps.rs.
// SPDX-License-Identifier: GPL-2.0-only
mod steps;

#[cfg(test)]
use steps::{LONG_PRESS_RESET, WATCHDOG_POWERS_OFF};

use crate::reply::{atomic_json, private_directory, Failure};
use serde_json::Value;
use std::{fs::File, io::Write};
use y2p_core::{
    json::{Object, ObjectExt},
    power_identity::process_identity,
    py,
    py::PyValue,
    Root,
};

/// What the coordinator reads of an intent: one it built, or one resumed from shutdown.json after a
/// daemon restart (which may hold anything a file can).
pub trait IntentFields {
    fn state(&self) -> Option<&str>;
    /// The deadline in CLOCK_MONOTONIC ns (0 when it is not an int).
    fn deadline_ns(&self) -> i128;
    /// Whether Reborn acknowledged with `Ready`. Anything but a dict with that outcome (null, or what
    /// a damaged resumed intent holds) is no acknowledgement: the deadline still ends the grace.
    fn ready(&self) -> bool;
    /// An acknowledgement that is set but is no dict with an outcome (only a damaged file has one).
    fn acknowledgement_odd(&self) -> bool;

    fn pending(&self) -> bool {
        self.state() == Some("ShutdownPending")
    }
}

impl IntentFields for Object {
    fn state(&self) -> Option<&str> {
        self.get("state").and_then(Value::as_str)
    }

    fn deadline_ns(&self) -> i128 {
        self.get("deadline_monotonic_ns")
            .and_then(Value::exact_int)
            .map_or(0, i128::from)
    }

    fn ready(&self) -> bool {
        match self.get("acknowledgement") {
            Some(Value::Object(ack)) => ack.get("outcome").is_some_and(|o| o.is_str("Ready")),
            _ => false,
        }
    }

    fn acknowledgement_odd(&self) -> bool {
        match self.get("acknowledgement") {
            Some(Value::Object(ack)) => !ack.contains_key("outcome"),
            Some(ack) => ack.truthy(),
            None => false,
        }
    }
}

pub struct Coordinator {
    root: Root,
    watchdog: Option<File>,
    pub boot_id: Option<String>,
    pub intent: Option<Object>,
}

impl Coordinator {
    pub fn new(root: &Root) -> Coordinator {
        let boot_id = root.boot_id();
        let previous = match root.json("/run/y2/shutdown.json") {
            Some(Value::Object(o)) => o,
            _ => Object::new(),
        };
        let boot = Value::from(boot_id.clone());
        let valid = previous.value("boot_id").py_eq(&boot)
            && matches!(
                previous.state(),
                Some("ShutdownPending" | "Executing" | "Failed")
            )
            && matches!(
                previous.get("action").and_then(Value::as_str),
                Some("poweroff" | "reboot")
            )
            && previous
                .get("deadline_monotonic_ns")
                .and_then(Value::exact_int)
                .is_some()
            && previous.get("id").and_then(Value::as_str).is_some()
            && previous.contains_key("client")
            && previous.contains_key("acknowledgement");
        let mut intent = valid.then_some(previous);
        if let Some(intent) = intent.as_mut() {
            if intent.acknowledgement_odd() {
                // A damaged file: the shutdown goes on as unacknowledged (the panel is blanked
                // before Reborn is stopped) once its deadline has passed.
                let _ = writeln!(
                    std::io::stderr(),
                    "power-daemon: resumed shutdown intent has no valid acknowledgement ({}); \
                     waiting for its deadline",
                    intent.value("acknowledgement")
                );
            }
            if intent.state() == Some("Executing") {
                // Continue the already requested transition after a daemon crash,
                // retaining its original deadline. No new boot is auto-rebooted.
                intent.set("state", "ShutdownPending");
            }
        }
        Coordinator {
            root: root.clone(),
            watchdog: None,
            boot_id,
            intent,
        }
    }

    fn clock(&self) -> i128 {
        i128::from(self.root.clock().monotonic_ns())
    }

    pub fn publish(&self) -> Result<(), Failure> {
        private_directory(&self.root.path("/run/y2"))?;
        atomic_json(
            &self.root.path("/run/y2/shutdown.json"),
            &Value::from(self.intent.clone()),
            false,
        )
    }

    pub fn request(
        &mut self,
        action: &Value,
        reason: &Value,
        grace: i64,
    ) -> Result<Object, Failure> {
        let valid = ["poweroff", "reboot"].iter().any(|a| action.is_str(a))
            && ["user", "low_battery", "update", "service"]
                .iter()
                .any(|r| reason.is_str(r));
        if !valid {
            return Err(Failure::value("invalid_shutdown_request"));
        }
        if !(3..=30).contains(&grace) {
            return Err(Failure::value("invalid_grace"));
        }
        if let Some(intent) = &self.intent {
            if !intent.value("action").py_eq(action) {
                return Err(Failure::value("different_shutdown_already_pending"));
            }
            return Ok(intent.clone());
        }
        let now = self.clock();
        let id = py::uuid4()?;
        let intent = self.intent.insert(
            Object::new()
                .with("schema", 1i64)
                .with("id", id)
                .with("boot_id", self.boot_id.clone())
                .with("state", "ShutdownPending")
                .with("action", action.clone())
                .with("reason", reason.clone())
                .with("requested_monotonic_ns", now as i64)
                .with(
                    "deadline_monotonic_ns",
                    (now + i128::from(grace) * 1_000_000_000) as i64,
                )
                .with(
                    "client",
                    process_identity(&self.root, "process", "/usr/bin/reborn"),
                )
                .with("acknowledgement", Value::Null)
                .with("failure", Value::Null),
        );
        let answer = intent.clone();
        self.publish()?;
        Ok(answer)
    }

    pub fn ack(
        &mut self,
        request_id: &Value,
        pid: i64,
        outcome: &Value,
    ) -> Result<Object, Failure> {
        let Some(intent) = self
            .intent
            .as_mut()
            .filter(|i| i.pending() && i.value("id").py_eq(request_id))
        else {
            return Err(Failure::value("stale_shutdown_ack"));
        };
        let current = process_identity(&self.root, "process", "/usr/bin/reborn");
        let same = current.as_ref().is_some_and(|c| {
            Value::Object(c.clone()).py_eq(&intent.value("client"))
                && c.value("pid").py_eq(&Value::from(pid))
        });
        if !same {
            return Err(Failure::value("ack_client_identity_changed"));
        }
        if !(outcome.is_str("Ready") || outcome.is_str("Failed")) {
            return Err(Failure::value("invalid_ack_outcome"));
        }
        let now = self.root.clock().monotonic_ns() as i64;
        intent.set(
            "acknowledgement",
            Object::new()
                .with("outcome", outcome.clone())
                .with("monotonic_ns", now),
        );
        let answer = intent.clone();
        self.publish()?;
        Ok(answer)
    }

    pub fn due(&self) -> bool {
        let Some(intent) = self.intent.as_ref().filter(|i| i.pending()) else {
            return false;
        };
        if self.clock() >= intent.deadline_ns() {
            return true;
        }
        intent.ready()
    }

    /// Seconds until the next sample or a pending deadline, as nanoseconds (0: no wait).
    pub fn wait_ns(&self, next_sample_ns: i128) -> i128 {
        // Socket activity wakes accept immediately. Only observation and a
        // pending shutdown need timed wakes; polling four times per second
        // provides neither faster requests nor better battery observations.
        let mut deadline = next_sample_ns;
        if let Some(intent) = self.intent.as_ref().filter(|i| i.pending()) {
            deadline = deadline.min(intent.deadline_ns());
        }
        (deadline - self.clock()).max(0)
    }
}

#[cfg(test)]
#[path = "coordinator_tests.rs"]
mod coordinator_tests;
