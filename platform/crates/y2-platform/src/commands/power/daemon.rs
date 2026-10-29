//! `y2-platform power-daemon` (`power.serve`): the battery and shutdown owner. Every second it samples
//! BAT0 (CLOCK_BOOTTIME for the SOC), keeps the displayed percentage across restarts, runs the
//! low-battery policy, journals, publishes `/run/y2/power.json` (Reborn's `power.rs` reads it directly:
//! same boot, at most 5 s old) and executes a due shutdown; between samples it serves one client of
//! `/run/y2/power.sock` (shutdown requests and Reborn's acknowledgement).
//!
//! Below a fixture root `Y2_DAEMON_TICKS=n` ends the loop after n iterations (exit 0), at the point
//! where tests/platform_cases/power.py stops the Python daemon.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    battery::{load_profile, sample, BatteryStatus, DisplayRecord, Soc, UI_STATE},
    coordinator::Coordinator,
    journal::PowerJournal,
    policy::{load_policy, LowBattery},
    quiet_exit, unix,
};
use crate::reply::{atomic_json, private_directory, unlink_missing_ok, Failure, Outcome};
use serde::Serialize;
use serde_json::Value;
use std::{
    io::Write,
    os::unix::{fs::FileTypeExt, net::UnixStream},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use y2p_core::sys::{set_signal, Handler};
use y2p_core::{
    json::{Object, ObjectExt},
    py::PyValue,
    Root,
};

pub const SOCKET: &str = "/run/y2/power.sock";
const REQUEST_LIMIT: usize = 2048;

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

/// One request on the power socket.
enum Request {
    /// Reborn's answer to a pending intent.
    Ack { id: Value, outcome: Value },
    /// A transition (`shutdown` or Reborn); the reason defaults to `user`.
    Transition { action: Value, reason: Value },
}

impl Request {
    /// The request's fields, or Python's `str(error)` of the lookup that failed.
    fn of(request: Value) -> Result<Request, String> {
        let Value::Object(mut request) = request else {
            return Err(format!(
                "'{}' object has no attribute 'get'",
                request.type_name()
            ));
        };
        if request.value("operation").is_str("ack") {
            let id = request.shift_remove("id").ok_or("'id'")?;
            let outcome = request.shift_remove("outcome").ok_or("'outcome'")?;
            return Ok(Request::Ack { id, outcome });
        }
        Ok(Request::Transition {
            action: request.value("action"),
            reason: request
                .shift_remove("reason")
                .unwrap_or_else(|| "user".into()),
        })
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
    let mut coordinator = Coordinator::new(root);
    let mut journal = PowerJournal::new();
    let mut battery = LowBattery::default();
    let mut soc = Soc::new(load_profile(root), root.json(UI_STATE));
    let mut ui_saved = None;
    let (policy, reason) = load_policy(root);
    RUNNING.store(true, Ordering::SeqCst);
    handle_signals();
    let server = unix::listen(&address)?;
    let ticks = tick_limit(root);
    let clock = root.clock();
    let now = || i128::from(clock.monotonic_ns());
    let mut iterations = 0u64;
    let mut next_sample: i128 = 0;
    let mut state = "Unavailable";
    let mut battery_status = BatteryStatus::default();
    let mut notes = Notes::default();
    while RUNNING.load(Ordering::SeqCst) {
        if now() >= next_sample {
            next_sample = now() + 1_000_000_000;
            // SOC uses physical elapsed time including suspend. Keep the
            // shutdown grace/sampling scheduler on its existing clock.
            battery_status = sample(root, &mut soc);
            let voltage = battery_status.voltage_uv;
            // 28: keep the displayed percentage across a restart (stock
            // RTC spare register); written only when it changes.
            if let Some(ui_state) = soc.state().filter(|s| Some(*s) != ui_saved) {
                let record = DisplayRecord::new(ui_state);
                if atomic_json(&root.path(UI_STATE), &record, true).is_ok() {
                    ui_saved = Some(ui_state);
                }
            }
            state = battery.observe(
                policy.as_ref(),
                voltage,
                root.integer("/sys/class/power_supply/BAT0/present"),
                &battery_status,
            );
            if state == "ShutdownPending" && coordinator.intent.is_none() {
                let grace = policy.as_ref().map_or(10, |p| p.grace_seconds);
                // An intent that could not be published is still pending here: it runs after
                // its grace all the same.
                let requested =
                    coordinator.request(&"poweroff".into(), &"low_battery".into(), grace);
                notes.check("low-battery shutdown request", requested);
            }
            journal.observe(
                root,
                state,
                &battery_status,
                coordinator.intent.as_ref(),
                None,
            );
            let record = PowerRecord {
                schema: 1,
                boot_id: coordinator.boot_id.as_deref(),
                monotonic_ns: clock.monotonic_ns(),
                state,
                voltage_uv: voltage,
                reason,
                thresholds_enabled: policy.is_some(),
                battery: &battery_status,
                qualification_reference: policy.as_ref().map(|p| &p.qualification_reference),
                shutdown: coordinator.intent.as_ref(),
            };
            notes.check(
                "/run/y2/power.json",
                atomic_json(&runtime.join("power.json"), &record, false),
            );
        }
        if coordinator.due() {
            journal.observe(
                root,
                state,
                &battery_status,
                coordinator.intent.as_ref(),
                Some("shutdown_execute"),
            );
            coordinator.execute();
        }
        let wait = coordinator.wait_ns(next_sample);
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
        serve_client(client, &mut coordinator);
    }
    drop(server);
    unlink_missing_ok(&address)?;
    quiet_exit(0)
}

/// One request line from one client; the answer is one line, `{"ok": true, "result": intent}` or
/// `{"ok": false, "error": str(error)}`.
fn serve_client(mut client: UnixStream, coordinator: &mut Coordinator) {
    let result = handle(&mut client, coordinator);
    let answer = match result {
        Ok(intent) => Object::new().with("ok", true).with("result", intent),
        Err(error) => Object::new().with("ok", false).with("error", error),
    };
    if let Ok(text) = y2p_core::json::dumps(&answer, y2p_core::json::Separators::Spaced) {
        let _ = unix::send_all(&mut client, (text + "\n").as_bytes());
    }
}

/// The request, as `str(error)` of every error Python catches here (ValueError, OSError, KeyError,
/// TypeError, AttributeError).
fn handle(client: &mut UnixStream, coordinator: &mut Coordinator) -> Result<Object, String> {
    unix::timeouts(client, Duration::from_millis(200)).map_err(|e| e.message())?;
    let (pid, uid) = unix::peer(client).map_err(|e| e.message())?;
    if uid != 0 {
        return Err("owner_privilege_required".into());
    }
    let mut data = Vec::new();
    while !data.contains(&b'\n') {
        let mut part = [0u8; REQUEST_LIMIT + 1];
        let n = unix::recv(client, &mut part).map_err(|e| e.message())?;
        if n == 0 || data.len() + n > REQUEST_LIMIT {
            return Err("invalid_power_request".into());
        }
        data.extend_from_slice(&part[..n]);
    }
    // Accepted difference: text that is not JSON is refused with serde's message, not json's.
    let request = serde_json::from_slice::<Value>(&data).map_err(|e| e.to_string())?;
    match Request::of(request)? {
        Request::Ack { id, outcome } => coordinator.ack(&id, pid, &outcome),
        Request::Transition { action, reason } => coordinator.request(&action, &reason, 10),
    }
    .map_err(|e| e.message())
}
