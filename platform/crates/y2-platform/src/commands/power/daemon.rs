//! `y2-platform power-daemon` (`power.serve`): the battery and shutdown owner. Every second it samples
//! BAT0 (CLOCK_BOOTTIME for the SOC), keeps the displayed percentage across restarts, runs the
//! low-battery policy, journals, publishes `/run/y2/power.json` (Reborn's `power.rs` reads it directly:
//! same boot, at most 5 s old) and executes a due shutdown; between samples it serves one client of
//! `/run/y2/power.sock` (shutdown requests and Reborn's acknowledgement).
//!
//! Below a fixture root `Y2_DAEMON_TICKS=n` ends the loop after n iterations (exit 0), at the point
//! where tests/platform_cases/power.py stops the Python daemon.
// SPDX-License-Identifier: GPL-2.0-only
mod requests;

use super::{
    battery::{load_profile, sample, BatteryStatus, DisplayRecord, Soc, UI_STATE},
    coordinator::Coordinator,
    journal::PowerJournal,
    policy::{load_policy, LowBattery, Policy},
    quiet_exit, unix,
};
use crate::reply::{atomic_json, private_directory, unlink_missing_ok, Failure, Outcome};
use requests::serve_client;
use serde::Serialize;
use serde_json::Value;
use std::{
    io::Write,
    os::unix::fs::FileTypeExt,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use y2p_core::sys::{set_signal, Handler};
use y2p_core::{json::Object, Root};

pub const SOCKET: &str = "/run/y2/power.sock";

static RUNNING: AtomicBool = AtomicBool::new(true);

/// `/run/y2/power.json` (written sorted; Reborn's power.rs reads it).
#[derive(Serialize)]
struct PowerRecord<'a> {
    schema: i64,
    boot_id: Option<&'a str>,
    monotonic_ns: u64,
    state: &'a str,
    voltage_uv: Option<i64>,
    reason: Option<&'a str>,
    thresholds_enabled: bool,
    battery: &'a BatteryStatus,
    qualification_reference: Option<&'a Value>,
    shutdown: Option<&'a Object>,
}

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
    state: &'static str,
    battery_status: BatteryStatus,
    notes: Notes,
}

impl Daemon<'_> {
    /// One sample: BAT0, the kept display value, the low-battery policy, the journal, power.json.
    fn sample(&mut self) {
        let root = self.root;
        // SOC uses physical elapsed time including suspend. Keep the
        // shutdown grace/sampling scheduler on its existing clock.
        self.battery_status = sample(root, &mut self.soc);
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
            root.integer("/sys/class/power_supply/BAT0/present"),
            &self.battery_status,
        );
        if self.state == "ShutdownPending" && self.coordinator.intent.is_none() {
            let grace = self.policy.as_ref().map_or(10, |p| p.grace_seconds);
            // An intent that could not be published is still pending here: it runs after
            // its grace all the same.
            let requested =
                self.coordinator
                    .request(&"poweroff".into(), &"low_battery".into(), grace);
            self.notes.check("low-battery shutdown request", requested);
        }
        self.journal.observe(
            root,
            self.state,
            &self.battery_status,
            self.coordinator.intent.as_ref(),
            None,
        );
        let record = PowerRecord {
            schema: 1,
            boot_id: self.coordinator.boot_id.as_deref(),
            monotonic_ns: root.clock().monotonic_ns(),
            state: self.state,
            voltage_uv: voltage,
            reason: self.reason,
            thresholds_enabled: self.policy.is_some(),
            battery: &self.battery_status,
            qualification_reference: self.policy.as_ref().map(|p| &p.qualification_reference),
            shutdown: self.coordinator.intent.as_ref(),
        };
        self.notes.check(
            "/run/y2/power.json",
            atomic_json(&root.path("/run/y2/power.json"), &record, false),
        );
    }

    /// A shutdown whose grace ended or that Reborn acknowledged: its journal row, then its steps.
    fn execute_due(&mut self) {
        if !self.coordinator.due() {
            return;
        }
        self.journal.observe(
            self.root,
            self.state,
            &self.battery_status,
            self.coordinator.intent.as_ref(),
            Some("shutdown_execute"),
        );
        self.coordinator.execute();
    }
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
    let _lock = match y2p_core::lock::try_exclusive(&lock_path) {
        Ok(Some(lock)) => lock,
        Ok(None) => return Err(unix::os(std::io::Error::from_raw_os_error(libc::EAGAIN))),
        Err(error) => return Err(Failure::os(error, &lock_path)),
    };
    let address = root.path(SOCKET);
    if address.exists() {
        let meta = std::fs::symlink_metadata(&address).map_err(|e| Failure::os(e, &address))?;
        if !meta.file_type().is_socket() {
            return Err(Failure::value("unsafe_power_socket_path"));
        }
        std::fs::remove_file(&address).map_err(|e| Failure::os(e, &address))?;
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
        state: "Unavailable",
        battery_status: BatteryStatus::default(),
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
