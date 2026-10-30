//! The shutdown coordinator: one intent per boot (`/run/y2/shutdown.json`, resumed after a daemon
//! restart), its grace and Reborn's acknowledgement; the steps that execute it are steps.rs.
// SPDX-License-Identifier: GPL-2.0-only
mod steps;

#[cfg(test)]
use steps::{LONG_PRESS_RESET, WATCHDOG_POWERS_OFF};

pub use y2p_contract::power::{Acknowledgement, Intent, IntentState, ShutdownAction};

use crate::reply::{atomic_json, private_directory, Failure};
use serde_json::Value;
use std::{fs::File, io::Write};
use y2p_core::{lenient::Shaped, power_identity::process_identity, py, Root};

/// Who asked for a shutdown (`reason`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownReason {
    User,
    LowBattery,
    Update,
    Service,
}

impl ShutdownReason {
    pub fn as_str(self) -> &'static str {
        match self {
            ShutdownReason::User => "user",
            ShutdownReason::LowBattery => "low_battery",
            ShutdownReason::Update => "update",
            ShutdownReason::Service => "service",
        }
    }

    /// The reason a request names (only these exact texts).
    pub fn named(text: &str) -> Option<ShutdownReason> {
        Some(match text {
            "user" => ShutdownReason::User,
            "low_battery" => ShutdownReason::LowBattery,
            "update" => ShutdownReason::Update,
            "service" => ShutdownReason::Service,
            _ => return None,
        })
    }
}

pub struct Coordinator {
    root: Root,
    watchdog: Option<File>,
    pub boot_id: Option<String>,
    pub intent: Option<Intent>,
}

impl Coordinator {
    pub fn new(root: &Root) -> Coordinator {
        let boot_id = root.boot_id();
        let previous = match root.json("/run/y2/shutdown.json") {
            Some(Value::Object(o)) => serde_json::from_value::<Intent>(Value::Object(o)).ok(),
            _ => None,
        };
        let mut intent = previous.filter(|p| p.boot_id.is(boot_id.as_deref()));
        if let Some(intent) = intent.as_mut() {
            if intent.acknowledgement_odd() {
                // A damaged file: the shutdown goes on as unacknowledged (the panel is blanked
                // before Reborn is stopped) once its deadline has passed.
                let ack = serde_json::to_string(&intent.acknowledgement).unwrap_or_default();
                let _ = writeln!(
                    std::io::stderr(),
                    "power-daemon: resumed shutdown intent has no valid acknowledgement ({ack}); \
                     waiting for its deadline"
                );
            }
            if intent.state == IntentState::Executing {
                // Continue the already requested transition after a daemon crash,
                // retaining its original deadline. No new boot is auto-rebooted.
                intent.state = IntentState::ShutdownPending;
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
            &self.intent,
            false,
        )
    }

    pub fn request(
        &mut self,
        action: Option<&str>,
        reason: Option<&str>,
        grace: i64,
    ) -> Result<Intent, Failure> {
        let (Some(action), Some(reason)) = (
            action.and_then(ShutdownAction::named),
            reason.and_then(ShutdownReason::named),
        ) else {
            return Err(Failure::value("invalid_shutdown_request"));
        };
        if !(3..=30).contains(&grace) {
            return Err(Failure::value("invalid_grace"));
        }
        if let Some(intent) = &self.intent {
            if intent.action != action {
                return Err(Failure::value("different_shutdown_already_pending"));
            }
            return Ok(intent.clone());
        }
        let now = self.clock();
        let intent = self.intent.insert(Intent {
            schema: Some(1),
            id: py::uuid4()?,
            boot_id: self.boot_id.clone().into(),
            state: IntentState::ShutdownPending,
            action,
            reason: Some(reason.as_str().into()),
            requested_monotonic_ns: Some(now as i64),
            deadline_monotonic_ns: (now + i128::from(grace) * 1_000_000_000) as i64,
            client: process_identity(&self.root, "process", "/usr/bin/reborn"),
            acknowledgement: None,
            failure: None,
            stop_results: None,
        });
        let answer = intent.clone();
        self.publish()?;
        Ok(answer)
    }

    /// Reborn's answer (`request_id` none when the request's id was no text).
    pub fn ack(
        &mut self,
        request_id: Option<&str>,
        pid: i64,
        outcome: Option<&str>,
    ) -> Result<Intent, Failure> {
        let Some(intent) = self
            .intent
            .as_mut()
            .filter(|i| i.pending() && Some(i.id.as_str()) == request_id)
        else {
            return Err(Failure::value("stale_shutdown_ack"));
        };
        let current = process_identity(&self.root, "process", "/usr/bin/reborn");
        let same = current.is_some_and(|c| intent.client == Some(c) && c.pid == pid);
        if !same {
            return Err(Failure::value("ack_client_identity_changed"));
        }
        let Some(outcome) = outcome.filter(|o| matches!(*o, "Ready" | "Failed")) else {
            return Err(Failure::value("invalid_ack_outcome"));
        };
        let now = self.root.clock().monotonic_ns() as i64;
        intent.acknowledgement = Some(Shaped::Is(Acknowledgement {
            outcome: Some(outcome.into()),
            monotonic_ns: Some(now),
        }));
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
