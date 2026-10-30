//! `y2-platform power-daemon`: the battery and shutdown owner. Every second it samples
//! BAT0 (CLOCK_BOOTTIME for the SOC), keeps the displayed percentage across restarts, runs the
//! low-battery policy, journals, publishes `/run/y2/power.json` (Reborn's `power.rs` reads it directly:
//! same boot, at most 5 s old; rewritten when it changes and at least every 2 s) and executes a due
//! shutdown; between samples it serves one client of
//! `/run/y2/power.sock` (shutdown requests and Reborn's acknowledgement).
//!
//! Below a fixture root `Y2_DAEMON_TICKS=n` ends the loop after n iterations (exit 0): the golden
//! cases' bounded runs (tests/platform_cases/power.py).
// SPDX-License-Identifier: GPL-2.0-only
use super::coordinator::Intent;
use super::published::PowerRecord;
use crate::reply::At;
mod requests;

use super::{
    battery::{load_profile, sample, BatteryStatus, DisplayRecord, Soc, UI_STATE},
    coordinator::Coordinator,
    journal::PowerJournal,
    policy::{load_policy, BatteryState, LowBattery, Policy},
    quiet_exit, unix,
};
use crate::reply::{atomic_json, private_directory, unlink_missing_ok, Failure, Outcome};
use requests::serve_client;
use std::{
    io::Write,
    os::unix::fs::FileTypeExt,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use y2p_core::sys::{set_signal, Handler};
use y2p_core::Root;

pub const SOCKET: &str = "/run/y2/power.sock";

static RUNNING: AtomicBool = AtomicBool::new(true);

/// How old an unchanged power.json may get before it is rewritten (Reborn refuses one 5 s old).
const RENEW_NS: u64 = 2_000_000_000;

/// Failures the daemon outlives, on stderr (the supervisor's log) when one starts or its text
/// changes: a write that fails every second leaves one line. The battery protection and the
/// shutdown coordination go on; a daemon that ended here would only be restarted into the same
/// failure, and a low-battery count would start over each time.
#[derive(Default)]
struct Notes(Vec<(&'static str, String)>);

impl Notes {
    fn check(&mut self, what: &'static str, result: Result<impl Sized, Failure>) {
        let Err(error) = result else {
            self.0.retain(|(w, _)| *w != what);
            return;
        };
        let message = error.message();
        if self.0.iter().any(|(w, m)| *w == what && *m == message) {
            return;
        }
        let _ = writeln!(std::io::stderr(), "power-daemon: {what}: {message}");
        self.0.retain(|(w, _)| *w != what);
        self.0.push((what, message));
    }
}

/// What the loop keeps from one iteration to the next.
struct Daemon<'r> {
    root: &'r Root,
    coordinator: Coordinator,
    journal: PowerJournal,
    battery: LowBattery,
    soc: Soc,
    /// The display state last written to battery-ui.json.
    ui_saved: Option<(i64, Option<f64>)>,
    policy: Option<Policy>,
    /// Why there is no policy (power.json `reason`).
    reason: Option<&'static str>,
    state: BatteryState,
    battery_status: BatteryStatus,
    /// What the power.json last written said (see [`Published`]) and when.
    published: Option<(Published, u64)>,
    notes: Notes,
}

impl Daemon<'_> {
    /// One sample: BAT0, the kept display value, the low-battery policy, the journal, power.json.
    fn sample(&mut self) {
        let root = self.root;
        // SOC uses physical elapsed time including suspend. Keep the
        // shutdown grace/sampling scheduler on its existing clock.
        let (status, reading) = sample(root, &mut self.soc);
        self.battery_status = status;
        let voltage = self.battery_status.voltage_uv;
        // 28: keep the displayed percentage across a restart (stock
        // RTC spare register); written only when it changes.
        if let Some(ui_state) = self.soc.state().filter(|s| Some(*s) != self.ui_saved) {
            let record = DisplayRecord::new(ui_state);
            if atomic_json(&root.path(UI_STATE), &record, true).is_ok() {
                self.ui_saved = Some(ui_state);
            }
        }
        self.state = self.battery.observe(
            self.policy.as_ref(),
            voltage,
            // The pack's presence of this sample (one read of BAT0/present per second).
            reading.present,
            &self.battery_status,
        );
        if self.state == BatteryState::ShutdownPending && self.coordinator.intent.is_none() {
            let grace = self.policy.as_ref().map_or(10, |p| p.grace_seconds);
            // An intent that could not be published is still pending here: it runs after
            // its grace all the same.
            let requested = self
                .coordinator
                .request(Some("poweroff"), Some("low_battery"), grace);
            self.notes.check("low-battery shutdown request", requested);
        }
        self.journal.observe(
            root,
            self.state.as_str(),
            &self.battery_status,
            self.coordinator.intent.as_ref(),
            None,
        );
        let now = root.clock().monotonic_ns();
        let content = Published::of(self);
        if !publish_due(self.published.as_ref(), &content, now) {
            return;
        }
        let record = PowerRecord {
            schema: 1,
            boot_id: self.coordinator.boot_id.clone(),
            monotonic_ns: now,
            state: self.state,
            voltage_uv: voltage,
            reason: self.reason.map(str::to_owned),
            thresholds_enabled: self.policy.is_some(),
            battery: self.battery_status.clone(),
            qualification_reference: self
                .policy
                .as_ref()
                .and_then(|p| p.qualification_reference.clone()),
            shutdown: self.coordinator.intent.clone(),
        };
        let written = atomic_json(&root.path("/run/y2/power.json"), &record, false);
        self.published = written.is_ok().then_some((content, now));
        self.notes.check("/run/y2/power.json", written);
    }

    /// A shutdown whose grace ended or that Reborn acknowledged: its journal row, then its steps.
    fn execute_due(&mut self) {
        if !self.coordinator.due() {
            return;
        }
        self.journal.observe(
            self.root,
            self.state.as_str(),
            &self.battery_status,
            self.coordinator.intent.as_ref(),
            Some("shutdown_execute"),
        );
        self.coordinator.execute();
    }
}

/// What a power.json says apart from its time, the raw voltage (the ADC moves by a step or two every
/// sample) and the voltage estimate's figures made from it: a record that says nothing new waits for
/// `RENEW_NS` (measured on 174: estimate_percent 97/98 and remaining_uah_estimate alternated every
/// second near a step, so the file was still written every second). Readers take the voltage from
/// a record at most 2 s old (Reborn refuses one 5 s old, sleep admission one 10 s old).
#[derive(PartialEq)]
struct Published {
    state: BatteryState,
    reason: Option<&'static str>,
    thresholds_enabled: bool,
    battery: BatteryStatus,
    shutdown: Option<Intent>,
}

impl Published {
    fn of(daemon: &Daemon) -> Self {
        let mut battery = daemon.battery_status.clone();
        battery.voltage_uv = None;
        // The voltage estimate's own figures follow the raw voltage (near a step they flicker
        // between two values every sample); the shown percent and every state stay in the test.
        if let Some(estimate) = &mut battery.estimate {
            estimate.filtered_voltage_uv = 0;
            estimate.estimate_percent = 0;
            estimate.remaining_uah_estimate = None;
        }
        Published {
            state: daemon.state,
            reason: daemon.reason,
            thresholds_enabled: daemon.policy.is_some(),
            battery,
            shutdown: daemon.coordinator.intent.clone(),
        }
    }
}

/// Whether power.json is written: what it says changed since `published`, or what was published
/// is `RENEW_NS` old.
fn publish_due<T: PartialEq>(published: Option<&(T, u64)>, content: &T, now: u64) -> bool {
    published.is_none_or(|(last, at)| last != content || now.saturating_sub(*at) >= RENEW_NS)
}

extern "C" fn stop(_: libc::c_int) {
    RUNNING.store(false, Ordering::SeqCst);
}

/// `signal.signal(SIGTERM/SIGINT, …)`: end the loop at its next iteration.
fn handle_signals() {
    for signal in [libc::SIGTERM, libc::SIGINT] {
        // SAFETY: the handler only stores to an atomic.
        let _ = unsafe { set_signal(signal, Handler::Call(stop), 0) };
    }
}

/// `Y2_DAEMON_TICKS` below a fixture root.
fn tick_limit(root: &Root) -> Option<u64> {
    if root.is_real() {
        return None;
    }
    std::env::var("Y2_DAEMON_TICKS").ok()?.parse().ok()
}

pub fn serve(root: &Root) -> Outcome {
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    let lock_path = runtime.join("power.lock");
    let _lock = y2p_core::lock::try_exclusive(&lock_path)
        .at(&lock_path)?
        .ok_or_else(Failure::busy)?;
    let address = root.path(SOCKET);
    if address.exists() {
        let meta = std::fs::symlink_metadata(&address).at(&address)?;
        if !meta.file_type().is_socket() {
            return Err(Failure::value("unsafe_power_socket_path"));
        }
        std::fs::remove_file(&address).at(&address)?;
    }
    let coordinator = Coordinator::new(root);
    let journal = PowerJournal::new();
    let battery = LowBattery::default();
    let soc = Soc::new(load_profile(root), root.json(UI_STATE));
    let (policy, reason) = load_policy(root);
    let mut daemon = Daemon {
        root,
        coordinator,
        journal,
        battery,
        soc,
        ui_saved: None,
        policy,
        reason,
        state: BatteryState::Unavailable,
        battery_status: BatteryStatus::default(),
        published: None,
        notes: Notes::default(),
    };
    RUNNING.store(true, Ordering::SeqCst);
    handle_signals();
    let server = unix::listen(&address)?;
    let ticks = tick_limit(root);
    let clock = root.clock();
    let now = || i128::from(clock.monotonic_ns());
    let mut iterations = 0u64;
    let mut next_sample: i128 = 0;
    while RUNNING.load(Ordering::SeqCst) {
        if now() >= next_sample {
            next_sample = now() + 1_000_000_000;
            daemon.sample();
        }
        daemon.execute_due();
        let wait = daemon.coordinator.wait_ns(next_sample);
        iterations += 1;
        if ticks.is_some_and(|limit| iterations >= limit) {
            break;
        }
        if wait == 0 {
            continue;
        }
        let Some(client) = unix::accept(&server, Duration::from_nanos(wait as u64))? else {
            continue;
        };
        serve_client(client, &mut daemon.coordinator);
    }
    drop(server);
    unlink_missing_ok(&address)?;
    quiet_exit(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn power_record_written_on_change_and_renewed_before_reborn_finds_it_stale() {
        let first = json!({"state": "Normal"});
        assert!(publish_due(None, &first, 0));
        let published = (first.clone(), 10_000_000_000);
        assert!(!publish_due(Some(&published), &first, 11_000_000_000));
        assert!(publish_due(Some(&published), &first, 12_000_000_000));
        let changed = json!({"state": "Low"});
        assert!(publish_due(Some(&published), &changed, 10_500_000_000));
        // A monotonic clock never runs back; if it did, an unchanged record would stay as it is.
        assert!(!publish_due(Some(&published), &first, 9_000_000_000));
    }
}
