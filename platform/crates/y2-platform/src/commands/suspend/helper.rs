//! `product_sleep.py` (`/usr/lib/y2-platform/product-sleep.py <fd>`): the owned product suspend that
//! `sleep request` launches with its `/run/y2/suspend.lock` open file description; no polling while
//! asleep and no editable bypass on 04.
//!
//! The established PCM wakes every 30 minutes for a battery safety check. Only an unambiguous timer
//! return can re-enter without restoring the screen/radios. All other wakes restore the application.
//! Critical operations use real flock ownership and bounded kernel leases, never the mere existence
//! of a PID file.
//!
//! The PM writes are single unbuffered `write(2)`s in this order: `wakeup_count`, backstop `30`,
//! `mem`, backstop `0`. Python's SIGTERM/SIGINT handler raises `RuntimeError('sleep_helper_interrupted')`
//! wherever the helper is; here the handler sets a flag that the same work checks between steps
//! (`interrupted`), and a signal during the restore (Python's `finally`) is raised once the restore
//! has finished (Python would abandon the restore part-way).
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    admission::{
        admission, application_starting, battery_and_cable, failed_before_freeze, guard_record,
        lock_holders, operation_state, retry_later, spm_state,
    },
    receipt::{self, record, wake_irqs, Stage},
    request, traceback, E, INTERRUPTED,
};
use serde_json::{Map, Value};
use std::{
    fs::{File, OpenOptions},
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use y2p_core::py::PyValue;
use y2p_core::{
    command,
    py::{is_true, truthy},
    root::python_int,
    suspend_text::{is_decimal, Regex},
    Root,
};

const OPERATION_LOCKS: [(&str, &str); 3] = [
    ("activity_operation", "/run/y2/activity.lock"),
    ("firmware_update", "/data/updates/.stage.lock"),
    ("maintenance", "/data/system/platform/.maintenance.lock"),
];
// Report 4fed1b6a (8.12): a sleep entered 8 s after Reborn started, before `application_ready`
// armed the power features, hung right after resume. A sleep requested that early now waits (screen
// already off) for the stage, at most this long, then goes on as before.
const APPLICATION_READY_WAIT_S: f64 = 60.0;
const RADIO_QUIESCE_SECONDS: f64 = 25.0;
// Fresh wakeup_count handshakes after an event that was neither Power, RTC nor a cable change,
// before the entry is refused as a wake in progress.
const STALE_COUNT_RETRIES: u32 = 2;

// glibc's interval timers (not in the libc crate for every target).
unsafe extern "C" {
    fn getitimer(which: libc::c_int, value: *mut libc::itimerval) -> libc::c_int;
    fn setitimer(
        which: libc::c_int,
        value: *const libc::itimerval,
        old: *mut libc::itimerval,
    ) -> libc::c_int;
}

static ALARM: AtomicBool = AtomicBool::new(false);

extern "C" fn stop(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

extern "C" fn alarm(_: libc::c_int) {
    ALARM.store(true, Ordering::SeqCst);
}

/// `signal.signal(sig, handler)` as CPython installs it (SA_ONSTACK, no SA_RESTART): the old action.
fn install(sig: libc::c_int, handler: extern "C" fn(libc::c_int)) -> libc::sigaction {
    // SAFETY: zeroed sigaction structs are valid; the handlers only store to atomics.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handler as usize;
        action.sa_flags = libc::SA_ONSTACK;
        libc::sigemptyset(&mut action.sa_mask);
        let mut old: libc::sigaction = std::mem::zeroed();
        libc::sigaction(sig, &action, &mut old);
        old
    }
}

use super::{interrupted as check, MASKED};

/// `time.sleep(seconds)`: a signal handler that raises ends it.
fn sleep(seconds: f64) -> Result<(), E> {
    #[cfg(test)]
    if let Some(result) = seams::with(|s| &mut s.sleep, |f| f(seconds)) {
        return result;
    }
    let until = std::time::Instant::now() + Duration::from_secs_f64(seconds);
    loop {
        let left = until.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return check();
        }
        let request = libc::timespec {
            tv_sec: left.as_secs() as libc::time_t,
            tv_nsec: left.subsec_nanos() as libc::c_long,
        };
        // SAFETY: a valid timespec; the remainder is not needed (the deadline is recomputed).
        if unsafe { libc::nanosleep(&request, std::ptr::null_mut()) } != 0 {
            check()?;
        }
    }
}

/// `product_sleep.main`: exit 0 slept and restored, 2 refused, 1 failed; an uncaught exception is a
/// traceback (exit 1). Nothing on stdout.
pub fn main(root: &Root, args: &[String]) -> crate::reply::Outcome {
    let Some(text) = args.first() else {
        traceback(&E::Crash("list index out of range".into()), 1)
    };
    let Some(fd) = python_int(text) else {
        traceback(
            &E::value(format!(
                "invalid literal for int() with base 10: {}",
                y2p_core::py::repr(text)
            )),
            1,
        )
    };
    install(libc::SIGTERM, stop);
    install(libc::SIGINT, stop);
    let result = i32::try_from(fd)
        .map_err(|_| E::Crash("signed integer is greater than maximum".into()))
        .and_then(|fd| execute(root, fd));
    // finally: os.close(fd)
    let closed = i32::try_from(fd).ok().map(|fd| {
        // SAFETY: closing the descriptor this programme was given, as Python's finally does.
        unsafe { libc::close(fd) }
    });
    if closed != Some(0) {
        let error = io::Error::from_raw_os_error(libc::EBADF);
        traceback(&E::os(&error, None), 1)
    }
    match result {
        Ok(code) => super::exit(code as i32),
        Err(error) => traceback(&error, 1),
    }
}

/// Python's `os.path.samestat(os.fstat(fd), path.stat())`.
fn inherited(root: &Root, fd: i32) -> Result<(), E> {
    // SAFETY: fstat into a zeroed stat buffer.
    let mut meta: libc::stat64 = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat64(fd, &mut meta) } != 0 {
        return Err(E::os(&io::Error::last_os_error(), None));
    }
    let path = root.path("/run/y2/suspend.lock");
    let lock = std::fs::metadata(&path).map_err(|e| E::os(&e, Some(&path)))?;
    if (meta.st_dev, meta.st_ino) != (lock.dev(), lock.ino()) {
        return Err(E::value("invalid_suspend_lock"));
    }
    // SAFETY: flock on the inherited descriptor.
    if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(E::os(&io::Error::last_os_error(), None));
    }
    Ok(())
}

/// `time.monotonic()`.
fn monotonic() -> f64 {
    #[cfg(test)]
    if let Some(now) = seams::with(|s| &mut s.monotonic, |f| f()) {
        return now;
    }
    super::monotonic()
}

/// `common.command(ctx, argv, timeout=20)`.
fn run(root: &Root, argv: &[&str]) -> command::Outcome {
    #[cfg(test)]
    if let Some(outcome) = seams::with(|s| &mut s.command, |f| f(argv)) {
        return outcome;
    }
    command::run(root, argv, 20.0)
}

fn execute(root: &Root, fd: i32) -> Result<i64, E> {
    check()?;
    // Require the actual inherited lock, never a user-supplied PID or stale file.
    inherited(root, fd)?;
    let mut run = Run::default();
    let body = run.body(root);
    let mut pending_error = None;
    match body {
        Ok(()) => {}
        Err(E::Refused(refusal)) => {
            run.result = 2;
            run.refusal = Some(refusal.clone());
            match record(
                root,
                Stage::new("product_refused", Some(2)).reason(&refusal),
            ) {
                // No kernel call followed a rejected wake-count handshake. Only
                // `wake_event_in_progress` is a Power/RTC wake; `wakeup_count_busy` is a kernel
                // wakeup source that did not settle.
                Ok(_) => {
                    if refusal == "wake_event_in_progress" || refusal == "wakeup_count_busy" {
                        run.restored_kernel = true;
                    }
                }
                Err(error) => pending_error = Some(error),
            }
        }
        Err(error @ (E::Os { .. } | E::Value { .. } | E::RuntimeError(_))) => {
            run.result = 1;
            let failure = Stage::new("product_failure", Some(1)).reason(error.text());
            if let Err(error) = record(root, failure) {
                pending_error = Some(error);
            }
        }
        Err(error) => pending_error = Some(error),
    }
    MASKED.store(true, Ordering::SeqCst);
    let finished = run.finally(root);
    MASKED.store(false, Ordering::SeqCst);
    // Every operation lock was held until radios and Reborn restored; released here.
    drop(std::mem::take(&mut run.held));
    finished?;
    if let Some(error) = pending_error {
        return Err(error);
    }
    check()?;
    Ok(run.result)
}

#[derive(Default)]
struct Run {
    held: Vec<(&'static str, File)>,
    prepared: bool,
    radios_changed: bool,
    result: i64,
    pending: bool,
    attempted: bool,
    restored_kernel: bool,
    refusal: Option<String>,
}

impl Run {
    fn body(&mut self, root: &Root) -> Result<(), E> {
        self.held = operation_locks(root)?;
        check()?;
        admission(root)?;
        battery_and_cable(root)?;
        operation_state(root, false)?;
        let mut count = wakeup_count(root)?;
        let mut previous_irqs = wake_irqs(root.read("/proc/interrupts").as_deref());
        guard_record(root, true, false)?;
        self.pending = true;
        marker(root, "HELPER_REQUEST")?;
        match request::prepare_owned(root) {
            Ok(_) => {}
            Err(error) if error.is_value() => return Err(E::refused(error.text())),
            Err(error) if error.is_os() => {
                return Err(E::refused("reborn_preparation_unavailable"))
            }
            Err(error) => return Err(error),
        }
        self.prepared = true;
        check()?;
        wait_for_application(root, &previous_irqs)?;
        record(root, Stage::new("quiescing_radios", Some(0)))?;
        self.radios_changed = true;
        for radio in ["wifi", "bluetooth"] {
            quiesce_radio(root, radio)?;
        }
        let stopped = Regex::new(r"powered=0 functions=(?:0x0|0) ").expect("pattern");
        let deadline = monotonic() + 15.0;
        while !stopped.is_match(
            &root
                .read("/sys/devices/platform/18070000.connectivity/status")
                .unwrap_or_default(),
        ) {
            if monotonic() >= deadline {
                return Err(E::refused("radio_power_down"));
            }
            sleep(0.1)?;
        }
        let mut stale_counts = 0;
        loop {
            check()?;
            // Reading can block until an event finishes. The write rejects any intervening
            // Power/RTC/USB wake before the kernel entry.
            if count.is_none() {
                count = wakeup_count(root)?;
            }
            if wake_irqs(root.read("/proc/interrupts").as_deref()) != previous_irqs {
                return Err(E::refused("wake_event_in_progress"));
            }
            let Some(current) = count.clone().filter(|c| is_decimal(c)) else {
                return Err(E::refused("wakeup_count_unavailable"));
            };
            battery_and_cable(root)?;
            operation_state(root, true)?;
            record(root, Stage::new("syncing", Some(0)))?;
            service(root, &["/bin/sync"])?;
            record(root, Stage::new("kernel_suspend", Some(0)))?;
            guard_record(root, true, true)?;
            self.pending = true;
            if let Err(error) =
                control_write(root, "/sys/power/wakeup_count", &format!("{current}\n"))
            {
                if !error.is_os() {
                    return Err(error);
                }
                // An event completed since the count was read. Archives 155 (06:38 self-wake)
                // and 156 (refused Power sleep): a routine BAT0 notification during preparation.
                // The next pass reads a fresh count and repeats the Power/RTC IRQ and cable
                // checks, so a real wake still cancels entry.
                stale_counts += 1;
                if stale_counts > STALE_COUNT_RETRIES {
                    return Err(E::refused("wakeup_count_busy"));
                }
                count = None;
                continue;
            }
            stale_counts = 0;
            self.restored_kernel = false;
            self.attempted = true;
            if let Err(error) = enter(root) {
                let E::Os { errno, .. } = &error else {
                    return Err(error);
                };
                if matches!(*errno, Some(libc::EBUSY) | Some(libc::EAGAIN)) {
                    self.restored_kernel = true;
                    return Err(E::refused("kernel_suspend_refused"));
                }
                if failed_before_freeze(root) {
                    // The write returned and no task froze: nothing is left unproven. 8.10 kept
                    // this pending and refused every sleep on that image (237 refusals in 4.4 h on
                    // battery).
                    self.restored_kernel = true;
                    retry_later(root)?;
                    return Err(E::refused("kernel_prepare_refused"));
                }
                return Err(error);
            }
            // Read as Python does (the count is not used again: the next pass reads a fresh one).
            wakeup_count(root)?;
            let value = record(root, Stage::new("kernel_resumed", Some(0)))?;
            let product = value.get("product").and_then(Value::as_object);
            self.restored_kernel = product.is_some_and(|p| is_true(p.get("kernel_completed")));
            if !self.restored_kernel {
                return Err(E::RuntimeError("kernel_exit_not_proven".into()));
            }
            if !timer_only(&value, &spm_state(root)) {
                break;
            }
            // No display, input reactivation, radio restore or library scan during a maintenance
            // return. Safety daemon gets a normal tick; the following admission also reads the
            // ADC directly.
            previous_irqs = value
                .get("wake_irqs_after")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            record(root, Stage::new("maintenance_wake", Some(0)))?;
            sleep(2.0)?;
            // The safety tick may publish a normal BAT0 notification. Begin a new handshake after
            // that pause; reusing the pre-tick count would reject an otherwise safe next cycle.
            // Preserve previous_irqs so Power/RTC during the pause still cancel entry, and the
            // atomic count store still rejects any later event.
            count = None;
        }
        Ok(())
    }

    /// Python's `finally`: keep every operation lock held until radios and Reborn restore.
    fn finally(&mut self, root: &Root) -> Result<(), E> {
        if self.prepared {
            // POWER-FINAL-17: screen first. Wi-Fi/Bluetooth firmware, patches and RF calibration
            // take ≈1.6 s (archive 110) and the UI used to wait for them; it does not depend on the
            // radios. A failed early restore is retried by the completion record, as before.
            // 4fed1b6a: nothing durable followed `kernel_resumed`; these two light rows say whether
            // a stall is in the screen restore or after it.
            progress(root, "screen_restoring")?;
            // F47: a refusal leaves the screen dark (it went dark with the request); only a wake —
            // Power or RTC during the preparation, or a real return from the kernel — lights it.
            // Lit after every refusal, a kernel that refused each sleep turned the screen on every
            // two minutes for hours. Failures still light it to explain.
            let dark =
                self.result == 2 && self.refusal.as_deref() != Some("wake_event_in_progress");
            let screen = screen_first(root, dark)?;
            progress(
                root,
                if screen.as_ref().is_some_and(|s| !s.is_empty()) {
                    "screen_restored"
                } else {
                    "screen_restore_failed"
                },
            )?;
            // F24: nothing sleeps any more. A Play right after waking took Reborn's shared activity
            // lease and was refused as busy (ALSA -16) for the ≈1.6 s of the radio restore below;
            // wired playback does not need the radios. The other operation locks stay.
            if let Some((_, file)) = self
                .held
                .iter()
                .find(|(owner, _)| *owner == "activity_operation")
            {
                file.unlock().map_err(|e| E::os(&e, None))?;
            }
            match self.restore_radios(root) {
                Ok(()) => {}
                Err(
                    error @ (E::Os { .. } | E::Value { .. } | E::Refused(_) | E::RuntimeError(_)),
                ) => {
                    let _ = error;
                    self.result = 1;
                }
                Err(error) => return Err(error),
            }
            let complete = format!("complete_result_{}", self.result);
            let stage = Stage {
                restore_ack: screen,
                dark,
                ..Stage::new(&complete, Some(self.result))
            };
            let value = record(root, stage)?;
            if let Some(result) = value.get("result").filter(|r| truthy(r)) {
                self.result = result.as_i64().unwrap_or(1);
            }
            if self.result == 0 {
                marker(root, "COMPLETE")?;
            }
        }
        // Clear whenever no kernel attempt is left unproven: a refusal, a proven kernel return,
        // or any failure before kernel entry (12 left result 1 pending here, refusing all sleep
        // until a reflash).
        if self.pending && (self.restored_kernel || !self.attempted) {
            guard_record(root, false, false)?;
        }
        Ok(())
    }

    fn restore_radios(&self, root: &Root) -> Result<(), E> {
        record(root, Stage::new("restoring_radios", Some(self.result)))?;
        marker(root, "RADIOS_RESTORING")?;
        if self.radios_changed {
            for (radio, directory) in [("wifi", "network"), ("bluetooth", "bluetooth")] {
                if root.read(&format!("/data/{directory}/enabled")).as_deref() == Some("1") {
                    service(root, &["/usr/sbin/y2-radio", radio, "on-runtime"])?;
                }
            }
        }
        Ok(())
    }
}

/// `operation_locks`: every critical operation's flock, exclusively, without waiting.
fn operation_locks(root: &Root) -> Result<Vec<(&'static str, File)>, E> {
    let mut held = Vec::new();
    for (owner, name) in OPERATION_LOCKS {
        let path = root.path(name);
        let parent = path.parent().unwrap_or(Path::new("/"));
        y2p_core::record::private_directory(parent).map_err(|e| E::os(&e, Some(parent)))?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)
            .map_err(|e| E::os(&e, Some(&path)))?;
        match file.try_lock() {
            Ok(()) => held.push((owner, file)),
            Err(std::fs::TryLockError::WouldBlock) => {
                let meta = file.metadata().map_err(|e| E::os(&e, None))?;
                let holders = lock_holders(root, &meta);
                let mut reason = owner.to_owned();
                if !holders.is_empty() {
                    reason = format!("{owner}:{}", holders.join(","));
                }
                return Err(E::refused(reason));
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(E::os(&error, None)),
        }
    }
    Ok(held)
}

/// `command(ctx, argv)`: a platform service the sleep needs, bounded at 20 s.
fn service(root: &Root, argv: &[&str]) -> Result<(), E> {
    let outcome = run(root, argv);
    check()?;
    if !outcome.ok {
        let name = Path::new(argv[0])
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        return Err(E::RuntimeError(format!("sleep_service_failed:{name}")));
    }
    Ok(())
}

/// Runtime-off one radio, waiting out a platform Bluetooth operation.
///
/// After a Power wake the reconnect owner holds bt-operation.lock while it reconnects the headset,
/// and `bluetooth-power off` refuses at once. Retry within a bounded window instead of failing the
/// whole sleep request.
fn quiesce_radio(root: &Root, radio: &str) -> Result<u32, E> {
    let argv = ["/usr/sbin/y2-radio", radio, "off-runtime"];
    let window = RADIO_QUIESCE_SECONDS;
    #[cfg(test)]
    let window = seams::SEAMS.with(|s| s.borrow().radio_quiesce_seconds.unwrap_or(window));
    let deadline = monotonic() + window;
    let mut attempts = 0;
    loop {
        attempts += 1;
        let outcome = run(root, &argv);
        check()?;
        if outcome.ok {
            return Ok(attempts);
        }
        if monotonic() >= deadline {
            return Err(E::RuntimeError(format!(
                "sleep_service_failed:y2-radio:{radio}:attempts={attempts}"
            )));
        }
        sleep(1.0)?;
    }
}

/// Defer kernel entry until `application_ready`, bounded. A Power or RTC wake during the wait
/// cancels the sleep like any wake before entry.
fn wait_for_application(root: &Root, previous_irqs: &Map<String, Value>) -> Result<(), E> {
    if !application_starting(root) {
        return Ok(());
    }
    record(root, Stage::new("waiting_application_ready", Some(0)))?;
    let deadline = monotonic() + APPLICATION_READY_WAIT_S;
    while application_starting(root) && monotonic() < deadline {
        sleep(0.25)?;
        if &wake_irqs(root.read("/proc/interrupts").as_deref()) != previous_irqs {
            return Err(E::refused("wake_event_in_progress"));
        }
    }
    Ok(())
}

/// Ask Reborn to restore before the radios are; `dark` keeps the screen off (a refused sleep nobody
/// woke, F47).
fn screen_first(root: &Root, dark: bool) -> Result<Option<Map<String, Value>>, E> {
    match receipt::application_rpc(root, "sleep_restore", dark) {
        Ok(ack) => Ok(Some(ack)),
        Err(error) if error.is_os() || error.is_value() => Ok(None),
        Err(error) => Err(error),
    }
}

/// One durable row in the sleep receipt without the sysfs reads of a full record (a stalled driver
/// could block those too). Never raises (an OSError, ValueError or TypeError is dropped).
fn progress(root: &Root, stage: &str) -> Result<(), E> {
    match receipt::progress(root, stage) {
        Err(error) if !error.is_recorded_failure() => Err(error),
        _ => Ok(()),
    }
}

fn marker(root: &Root, stage: &str) -> Result<(), E> {
    control_write(root, "/sys/firmware/y2_pm/stage", &format!("{stage}\n"))
}

/// Issue one sysfs command; a buffered close can replay a rejected write.
///
/// Python's TextIOWrapper/BufferedWriter retry a failed flush during close. For PM that can
/// suspend after EBUSY, yet still report the original refusal to the caller. This is one
/// `write(2)` on an unbuffered descriptor. A short write is also a failure (EIO), never a reason to
/// issue the remaining command again.
fn control_write(root: &Root, name: &str, value: &str) -> Result<(), E> {
    #[cfg(test)]
    seams::with(|s| &mut s.control, |f| f(root, name, value)).transpose()?;
    let path = root.path(name);
    // open(..., 'wb', buffering=0): O_WRONLY|O_CREAT|O_TRUNC|O_CLOEXEC, 0666.
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o666)
        .custom_flags(libc::O_CLOEXEC)
        .open(&path)
        .map_err(|e| E::os(&e, Some(&path)))?;
    let data = value.as_bytes();
    loop {
        let written = store(&file, data);
        if written < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EINTR) {
                // PEP 475: retried unless the signal handler raised.
                check()?;
                continue;
            }
            return Err(E::os(&error, None));
        }
        if written as usize != data.len() {
            return Err(E::Os {
                class: "OSError",
                text: format!("[Errno 5] short PM control write: {name}"),
                errno: Some(libc::EIO),
            });
        }
        return Ok(());
    }
}

/// One write(2) of `data`.
fn store(file: &File, data: &[u8]) -> isize {
    #[cfg(test)]
    if let Some(result) = seams::with(|s| &mut s.store, |f| f(data)) {
        return match result {
            Ok(written) => written as isize,
            Err(errno) => {
                // SAFETY: this thread's errno, as the replaced write(2) sets it.
                unsafe { *libc::__errno_location() = errno };
                -1
            }
        };
    }
    // SAFETY: one write of `data` to the open descriptor.
    unsafe { libc::write(file.as_raw_fd(), data.as_ptr().cast(), data.len()) }
}

/// The kernel entry: the backstop armed immediately before the blocking `mem` write and cleared
/// immediately after it (Python's `try: enter(ctx) finally: backstop 0`).
fn enter(root: &Root) -> Result<(), E> {
    // The already qualified one-shot RGU backstop pauses during actual SPM residence and bounds
    // stalls in entry/restoration. A reset remains a failed sleep, quarantined by durable intent.
    control_write(root, "/sys/firmware/y2_pm/backstop_s", "30\n")?;
    let entered = kernel_sleep(root);
    let cleared = control_write(root, "/sys/firmware/y2_pm/backstop_s", "0\n");
    cleared?;
    entered
}

fn kernel_sleep(root: &Root) -> Result<(), E> {
    #[cfg(test)]
    if let Some(result) = seams::with(|s| &mut s.enter, |f| f(root)) {
        return result;
    }
    let result = control_write(root, "/sys/power/state", "mem\n");
    if result.is_ok() {
        check()?;
    }
    result
}

fn timer_only(value: &Map<String, Value>, spm: &Map<String, Value>) -> bool {
    let empty = Map::new();
    let p = value
        .get("product")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let before = value
        .get("wake_irqs_before")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let after = value
        .get("wake_irqs_after")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    is_true(p.get("kernel_completed"))
        && is_true(p.get("same_boot"))
        && spm.get("timer_wake").and_then(Value::as_str) == Some("1")
        && ["power", "rtc"].iter().all(|name| match before.get(*name) {
            Some(b) => after.get(*name).is_some_and(|a| a.py_eq(b)),
            None => false,
        })
}

/// `wakeup_count(ctx, timeout=2)`: sysfs `wakeup_count_show` calls `pm_get_wakeup_count(...,
/// true)`, so even O_NONBLOCK cannot bound an active wake source. Interrupt that read (SIGALRM)
/// rather than leaving the product helper stuck before it reaches kernel PM.
fn wakeup_count(root: &Root) -> Result<Option<String>, E> {
    let mut running: libc::itimerval = unsafe { std::mem::zeroed() };
    // SAFETY: getitimer into a zeroed itimerval.
    unsafe { getitimer(libc::ITIMER_REAL, &mut running) };
    if running.it_value.tv_sec != 0 || running.it_value.tv_usec != 0 {
        return Err(E::refused("sleep_deadline_in_progress"));
    }
    ALARM.store(false, Ordering::SeqCst);
    let old = install(libc::SIGALRM, alarm);
    let timer = |seconds: libc::time_t| {
        let value = libc::itimerval {
            it_interval: libc::timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            it_value: libc::timeval {
                tv_sec: seconds,
                tv_usec: 0,
            },
        };
        // SAFETY: a valid itimerval.
        unsafe { setitimer(libc::ITIMER_REAL, &value, std::ptr::null_mut()) };
    };
    timer(2);
    let result = read_count(root);
    timer(0);
    // SAFETY: restore the previous action.
    unsafe { libc::sigaction(libc::SIGALRM, &old, std::ptr::null_mut()) };
    result
}

/// `ctx.read('/sys/power/wakeup_count')` in-process, with the alarm's raise.
fn read_count(root: &Root) -> Result<Option<String>, E> {
    let raised = || -> Result<(), E> {
        if ALARM.swap(false, Ordering::SeqCst) {
            return Err(E::refused("wakeup_count_busy"));
        }
        check()
    };
    let path = root.path("/sys/power/wakeup_count");
    let Ok(file) = File::open(&path) else {
        raised()?;
        return Ok(None);
    };
    let limit = y2p_core::root::READ_LIMIT;
    let cap = (limit + 1) * 4;
    let mut bytes = Vec::new();
    let mut block = [0u8; 8192];
    // SAFETY: the descriptor stays owned by `file`; ManuallyDrop avoids a second close.
    let mut reader = std::mem::ManuallyDrop::new(unsafe { File::from_raw_fd(file.as_raw_fd()) });
    while bytes.len() < cap {
        match reader.read(&mut block) {
            Ok(0) => break,
            Ok(n) => bytes.extend_from_slice(&block[..n.min(cap - bytes.len())]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => raised()?,
            Err(_) => {
                raised()?;
                return Ok(None);
            }
        }
    }
    drop(file);
    raised()?;
    let text = String::from_utf8_lossy(&bytes);
    if text.chars().count() > limit {
        return Ok(None);
    }
    Ok(Some(y2p_core::root::python_strip(&text).to_owned()))
}

/// Where Python's tests patch this module (`execute`'s `enter`, `time.sleep`, `time.monotonic`,
/// `control_write`, the raw PM store, `command`, `RADIO_QUIESCE_SECONDS`) and `product_sleep.launch`:
/// per thread, in unit tests only.
#[cfg(test)]
pub(crate) mod seams {
    use super::{Root, E};
    use std::{cell::RefCell, fs::File};
    use y2p_core::command::Outcome;

    /// One replaced call.
    pub type Seam<F> = Option<Box<F>>;
    pub type Enter = dyn FnMut(&Root) -> Result<(), E>;
    pub type Sleep = dyn FnMut(f64) -> Result<(), E>;
    pub type Control = dyn FnMut(&Root, &str, &str) -> Result<(), E>;
    pub type Store = dyn FnMut(&[u8]) -> Result<usize, i32>;
    pub type Command = dyn FnMut(&[&str]) -> Outcome;
    pub type Launch = dyn FnMut(&Root, &File) -> Result<(), E>;

    #[derive(Default)]
    pub struct Seams {
        /// The kernel entry (`kernel_sleep`); the backstop writes around it stay.
        pub enter: Seam<Enter>,
        pub sleep: Seam<Sleep>,
        pub monotonic: Seam<dyn FnMut() -> f64>,
        /// Runs before each PM control write; an error is raised instead of the write.
        pub control: Seam<Control>,
        /// The one write(2) of a PM control write: the bytes it wrote, or its errno.
        pub store: Seam<Store>,
        pub command: Seam<Command>,
        pub radio_quiesce_seconds: Option<f64>,
        /// `sleep request`'s helper start.
        pub launch: Seam<Launch>,
    }

    thread_local! {
        pub static SEAMS: RefCell<Seams> = RefCell::new(Seams::default());
    }

    /// Call the seam `pick` names, if set (taken out meanwhile: it may run code that asks for another).
    pub fn with<F: ?Sized, R>(
        pick: fn(&mut Seams) -> &mut Seam<F>,
        call: impl FnOnce(&mut F) -> R,
    ) -> Option<R> {
        let mut seam = SEAMS.with(|s| pick(&mut s.borrow_mut()).take())?;
        let result = call(&mut seam);
        SEAMS.with(|s| *pick(&mut s.borrow_mut()) = Some(seam));
        Some(result)
    }

    pub fn reset() {
        SEAMS.with(|s| *s.borrow_mut() = Seams::default());
    }
}

#[cfg(test)]
#[path = "helper_tests.rs"]
mod helper_tests;
