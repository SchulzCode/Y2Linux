//! `power.Coordinator`: the shutdown intent (`/run/y2/shutdown.json`), Reborn's acknowledgement and
//! the ordered, bounded shutdown steps with their pmsg trail (F39/F50 watchdog, F57 long-press reset).
//!
//! Below a fixture root nothing here signals a process or starts a power command: the steps' commands
//! get the fixture answers (`y2p_core::command`) and the kill step runs only on the real root, as in
//! Python, whose fixture `Context` answers `fixture` and whose kill is guarded by `ctx.root == '/'`.
// SPDX-License-Identifier: GPL-2.0-only
use super::crash;
use crate::reply::{atomic_json, private_directory, Failure};
use serde_json::Value;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    time::Duration,
};
use y2p_core::{
    clock, command,
    json::{Object, ObjectExt},
    power_identity::process_identity,
    py,
    py::PyValue,
    Root,
};

// F39: the radio stop, one step per sub-command of the S41 script.
const RADIO_STOP: [&str; 3] = ["wifi", "bluetooth", "daemon"];
const WATCHDOG_POWERS_OFF: &str = "/sys/class/power_supply/BAT0/watchdog_powers_off";
const LONG_PRESS_RESET: &str = "/sys/firmware/y2_pmic/long_press_reset";
// F39 (471831b6, c14a3860): 2 of 55 update restarts lost these journal writes
// and one never restarted. A stage write is a Python start and an fsync, which a
// busy shutdown stretches past 1 s. Every step also goes to pstore pmsg (RAM, it
// cannot stall); the next boot's retained capture or the recovery keeps it.
const STAGE_SECONDS: f64 = 3.0;

/// A step's `{ok, reason}` (its output is never kept).
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub ok: bool,
    pub reason: Option<String>,
}

impl Step {
    fn of(ok: bool, reason: Option<&str>) -> Step {
        Step {
            ok,
            reason: reason.map(str::to_owned),
        }
    }
}

/// `shutdown_trail(ctx, step, result)`: one pmsg line per shutdown step; best effort, never fails.
pub fn shutdown_trail(root: &Root, step: &str, result: Option<&Step>) {
    let mut line = format!("{:.2} shutdown {step}", clock::monotonic());
    if let Some(result) = result {
        if result.ok {
            line += " ok";
        } else {
            line += " ";
            line += result.reason.as_deref().unwrap_or("None");
        }
    }
    if let Ok(mut pmsg) = OpenOptions::new()
        .append(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root.path("/dev/pmsg0"))
    {
        let _ = pmsg.write_all((line + "\n").as_bytes());
    }
}

/// `open(path, 'w').write(text)`: created 0666 & umask, truncated, one write.
fn write_text(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(path)?;
    file.write_all(text.as_bytes())
}

/// `backlight_off(ctx)`: power down every panel backlight; bounded, never fails.
pub fn backlight_off(root: &Root) -> Step {
    let nodes: Vec<_> = root
        .glob("/sys/class/backlight/*")
        .into_iter()
        .map(|d| d.join("bl_power"))
        .filter(|n| n.exists())
        .collect();
    if nodes.is_empty() {
        return Step::of(false, Some("no_backlight"));
    }
    let failed = nodes
        .iter()
        .filter(|node| write_text(node, "4\n").is_err())
        .count();
    Step::of(failed == 0, (failed > 0).then_some("write_failed"))
}

/// `wait_gone(ctx, name, timeout)`: poll every 0.1 s until the path is gone.
fn wait_gone(root: &Root, name: &str, timeout: f64) -> Step {
    let deadline = clock::monotonic() + timeout;
    while root.path(name).exists() {
        if clock::monotonic() >= deadline {
            return Step::of(false, Some("still_present"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Step::of(true, None)
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
        let one_of = |key: &str, choices: &[&str]| {
            previous
                .get(key)
                .is_some_and(|v| choices.iter().any(|c| v.is_str(c)))
        };
        let valid = previous.value("boot_id").py_eq(&boot)
            && one_of("state", &["ShutdownPending", "Executing", "Failed"])
            && one_of("action", &["poweroff", "reboot"])
            && previous
                .get("deadline_monotonic_ns")
                .and_then(Value::exact_int)
                .is_some()
            && previous.get("id").and_then(Value::as_str).is_some()
            && previous.contains_key("client")
            && previous.contains_key("acknowledgement");
        let mut intent = valid.then_some(previous);
        if let Some(intent) = intent.as_mut() {
            if intent.value("state").is_str("Executing") {
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
        i128::from(clock::monotonic_ns())
    }

    fn intent(&mut self) -> &mut Object {
        self.intent.as_mut().expect("an intent")
    }

    fn deadline(&self) -> i128 {
        self.intent
            .as_ref()
            .and_then(|i| i.get("deadline_monotonic_ns"))
            .and_then(Value::exact_int)
            .map_or(0, i128::from)
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
        let id = py::uuid4().map_err(|e| {
            let (text, class) = py::os_error(&e, None);
            Failure::Os { text, class }
        })?;
        self.intent = Some(
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
        self.publish()?;
        Ok(self.intent.clone().expect("set"))
    }

    pub fn ack(
        &mut self,
        request_id: &Value,
        pid: i64,
        outcome: &Value,
    ) -> Result<Object, Failure> {
        let pending = self.intent.as_ref().is_some_and(|i| {
            i.value("state").is_str("ShutdownPending") && i.value("id").py_eq(request_id)
        });
        if !pending {
            return Err(Failure::value("stale_shutdown_ack"));
        }
        let current = process_identity(&self.root, "process", "/usr/bin/reborn");
        let same = current.as_ref().is_some_and(|c| {
            Value::Object(c.clone()).py_eq(&self.intent.as_ref().expect("pending").value("client"))
                && c.value("pid").py_eq(&Value::from(pid))
        });
        if !same {
            return Err(Failure::value("ack_client_identity_changed"));
        }
        if !(outcome.is_str("Ready") || outcome.is_str("Failed")) {
            return Err(Failure::value("invalid_ack_outcome"));
        }
        let now = self.clock() as i64;
        self.intent().set(
            "acknowledgement",
            Object::new()
                .with("outcome", outcome.clone())
                .with("monotonic_ns", now),
        );
        self.publish()?;
        Ok(self.intent.clone().expect("set"))
    }

    pub fn due(&self) -> bool {
        let Some(intent) = &self.intent else {
            return false;
        };
        if !intent.value("state").is_str("ShutdownPending") {
            return false;
        }
        if self.clock() >= self.deadline() {
            return true;
        }
        // `bool(ack and ack['outcome'] == 'Ready')`: a resumed intent may carry anything here.
        match intent.value("acknowledgement") {
            ack if !ack.truthy() => false,
            Value::Object(ack) => match ack.get("outcome") {
                Some(outcome) => outcome.is_str("Ready"),
                None => crash("KeyError: 'outcome'"),
            },
            Value::Array(_) => crash("TypeError: list indices must be integers or slices, not str"),
            Value::String(_) => crash("TypeError: string indices must be integers, not 'str'"),
            other => crash(&format!(
                "TypeError: '{}' object is not subscriptable",
                other.type_name()
            )),
        }
    }

    /// Seconds until the next sample or a pending deadline, as nanoseconds (0: no wait).
    pub fn wait_ns(&self, next_sample_ns: i128) -> i128 {
        // Socket activity wakes accept immediately. Only observation and a
        // pending shutdown need timed wakes; polling four times per second
        // provides neither faster requests nor better battery observations.
        let mut deadline = next_sample_ns;
        if self
            .intent
            .as_ref()
            .is_some_and(|i| i.value("state").is_str("ShutdownPending"))
        {
            deadline = deadline.min(self.deadline());
        }
        (deadline - self.clock()).max(0)
    }

    pub fn execute(&mut self) -> Result<Step, Failure> {
        // Only the actual daemon entry point calls this; fixture contexts can
        // never signal a host process or execute host power commands.
        self.intent().set("state", "Executing");
        self.publish()?;
        let (action, reason) = {
            let intent = self.intent();
            (intent.value("action"), intent.value("reason"))
        };
        shutdown_trail(
            &self.root,
            &format!("execute {} {}", action.py_str(), reason.py_str()),
            None,
        );
        let mut results: Vec<(String, Step)> = Vec::new();
        let watchdog = self.arm_watchdog();
        shutdown_trail(&self.root, "watchdog", Some(&watchdog));
        results.push(("watchdog".into(), watchdog));
        let long_press = self.long_press_reset_off();
        shutdown_trail(&self.root, "long_press_reset", Some(&long_press));
        results.push(("long_press_reset".into(), long_press));
        let pending = self.step(
            "journal_pending",
            Some(&["/usr/sbin/y2-platform", "boot-stage", "shutdown_pending"]),
            STAGE_SECONDS,
        );
        results.push(("journal_pending".into(), pending));
        // Reborn normally ends on its dark frame with the backlight already off
        // before it acknowledges. If it never did (hung, crashed, deadline
        // reached) the light goes out before the kill: dropping DRM master would
        // otherwise restore the stale fbdev/console buffer on a lit panel.
        let ready = match self.intent().value("acknowledgement") {
            ack if !ack.truthy() => false,
            Value::Object(ack) => ack.value("outcome").is_str("Ready"),
            other => crash(&format!(
                "AttributeError: '{}' object has no attribute 'get'",
                other.type_name()
            )),
        };
        if !ready {
            results.push(("backlight_early".into(), backlight_off(&self.root)));
        }
        let reborn = self.step("S90reborn", Some(&["/etc/init.d/S90reborn", "stop"]), 3.0);
        results.push(("S90reborn".into(), reborn));
        // Stop/reap failure cannot extend the battery grace indefinitely.
        self.begin("kill");
        for (name, executable) in [
            ("process", "/usr/bin/reborn"),
            ("supervisor", "/usr/libexec/reborn-supervise"),
        ] {
            let identity = process_identity(&self.root, name, executable);
            if let Some(identity) = identity.filter(|_| self.root.is_real()) {
                kill(&self.root, name, executable, &identity);
            }
        }
        // Reborn presents its shutdown transition, ends on a black frame and
        // turns the backlight off before acknowledging. If it crashed, hung or
        // was killed, the DRM last-close restores the fbdev buffer (stale,
        // console or white). Never hand init a lit panel.
        self.begin("backlight");
        results.push(("backlight".into(), backlight_off(&self.root)));
        // #53: a clock set by hand (or from the network) survives a restart
        // to the minute instead of to the last hourly record.
        let remember = self.step(
            "time_remember",
            Some(&["/usr/sbin/y2-platform", "time-remember"]),
            2.0,
        );
        results.push(("time_remember".into(), remember));
        let sync = self.step("sync", Some(&["/bin/busybox", "sync"]), 5.0);
        let synced = sync.ok;
        results.push(("sync".into(), sync));
        // F39: 5 shutdowns stalled the whole system in or right after the
        // radio stop (Wi-Fi connected, Bluetooth paging). It now runs after the
        // sync, one radio per step; a stall there leaves shutdown_complete
        // unwritten and ends in the watchdog reset.
        for name in RADIO_STOP {
            let argument = format!("stop-{name}");
            let step = self.step(
                &format!("S41_{name}"),
                Some(&["/etc/init.d/S41y2-connectivity", &argument]),
                3.0,
            );
            results.push((format!("S41_{name}"), step));
            if name == "wifi" {
                // Blocking y2-wifi removes the card in a kernel worker after
                // the write returned; wait for it so a stall names this step.
                let removed = self.step("S41_wifi_removed", None, 2.0);
                results.push(("S41_wifi_removed".into(), removed));
            }
        }
        if synced {
            let complete = self.step(
                "journal_complete",
                Some(&["/usr/sbin/y2-platform", "boot-stage", "shutdown_complete"]),
                STAGE_SECONDS,
            );
            results.push(("journal_complete".into(), complete));
        }
        let mut stop_results = Object::new();
        for (name, step) in &results {
            stop_results.set(
                name,
                Object::new()
                    .with("ok", step.ok)
                    .with("reason", step.reason.clone()),
            );
        }
        self.intent().set("stop_results", stop_results);
        self.publish()?;
        // BusyBox init owns rcK, unmount and final reboot(2). Never use -f.
        let action = action.as_str().unwrap_or_default().to_owned();
        let result = self.step(
            &format!("init_{action}"),
            Some(&["/bin/busybox", &action]),
            2.0,
        );
        if !result.ok {
            self.intent()
                .set("state", "Failed")
                .set("failure", "init_shutdown_request_failed");
            self.publish()?;
        }
        // The watchdog stays armed and is fed no more (this daemon's exit at
        // rcK closes it without 'V': one last ping, still running). rcK,
        // unmount and reboot(2) take ~5 s and reboot(2) stops it (mtk_wdt
        // stop_on_reboot); a stall there, or a failed request, ends in its reset.
        Ok(result)
    }

    fn begin(&mut self, name: &str) {
        shutdown_trail(&self.root, &format!("begin {name}"), None);
        if let Some(watchdog) = self.watchdog.as_mut() {
            let _ = watchdog.write(b".");
        }
    }

    /// One bounded shutdown step between pmsg 'begin' and result lines.
    fn step(&mut self, name: &str, argv: Option<&[&str]>, timeout: f64) -> Step {
        self.begin(name);
        let result = match argv {
            Some(argv) => {
                let outcome = command::run(&self.root, argv, timeout);
                Step {
                    ok: outcome.ok,
                    reason: outcome.reason,
                }
            }
            None => wait_gone(&self.root, "/sys/class/net/wlan0", timeout),
        };
        shutdown_trail(&self.root, name, Some(&result));
        result
    }

    /// F39: arm the SoC watchdog (31 s) for the whole shutdown.
    ///
    /// Its reset completes the requested transition whatever stalls: a restart (update: LK still finds
    /// MISC boot-recovery, written by `prepare` before the request, as after the F34 recovery
    /// watchdog), or for a power-off the loader's reason 3: charging mode on a charger, otherwise off.
    fn arm_watchdog(&mut self) -> Step {
        if self.watchdog.is_some() {
            return Step::of(true, None);
        }
        let mut failure = None;
        if self.intent().value("action").is_str("poweroff")
            && write_text(&self.root.path(WATCHDOG_POWERS_OFF), "1\n").is_err()
        {
            // Old kernel or no RGU mapping: the reset restarts instead.
            failure = Some("powers_off_unavailable");
        }
        match OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(self.root.path("/dev/watchdog"))
        {
            Ok(file) => self.watchdog = Some(file),
            Err(_) => return Step::of(false, Some("watchdog_unavailable")),
        }
        Step::of(failure.is_none(), failure)
    }

    /// F57 (496f54ad): Power still held through the 8-14 s power-off let the PMIC long-press reset
    /// cut it ~3 s after Reborn's ack. With the watchdog armed as the backstop, turn that reset off
    /// until the next start (the preloader sets it again). Without the watchdog it stays the only
    /// escape from a stalled shutdown. Never fails the shutdown.
    fn long_press_reset_off(&self) -> Step {
        if self.watchdog.is_none() {
            return Step::of(false, Some("kept_without_watchdog"));
        }
        let Ok(mut file) = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(self.root.path(LONG_PRESS_RESET))
        else {
            return Step::of(false, Some("long_press_reset_unavailable")); // older kernel
        };
        // `os.write(fd, b'0\n')`: one write, its count not checked.
        #[allow(clippy::unused_io_amount)]
        match file.write(b"0\n") {
            Ok(_) => Step::of(true, None),
            Err(_) => Step::of(false, Some("long_press_reset_failed")),
        }
    }
}

/// SIGKILL through a pidfd, after checking the identity once more (only on the real root).
fn kill(root: &Root, name: &str, executable: &str, identity: &Object) {
    let Some(pid) = identity.get("pid").and_then(Value::exact_int) else {
        return;
    };
    // SAFETY: pidfd_open takes a pid and flags and returns a new fd or -1.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) };
    if fd < 0 {
        return;
    }
    if process_identity(root, name, executable).as_ref() == Some(identity) {
        // SAFETY: `fd` is the pidfd opened above; no siginfo, no flags.
        unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                fd as libc::c_int,
                libc::SIGKILL,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            );
        }
    }
    // SAFETY: closing the pidfd this function opened.
    unsafe {
        libc::close(fd as libc::c_int);
    }
}

#[cfg(test)]
#[path = "coordinator_tests.rs"]
mod coordinator_tests;
