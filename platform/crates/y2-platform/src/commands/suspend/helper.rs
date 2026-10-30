//! The product sleep helper (`y2-platform __product-sleep <fd>`): the owned product suspend that
//! `sleep request` launches with its `/run/y2/suspend.lock` open file description; no polling while
//! asleep and no editable bypass on 04.
//!
//! The established PCM wakes every 30 minutes for a battery safety check. Only an unambiguous timer
//! return can re-enter without restoring the screen/radios. All other wakes restore the application.
//! Critical operations use real flock ownership and bounded kernel leases, never the mere existence
//! of a PID file.
//!
//! The PM writes are single unbuffered `write(2)`s in this order: `wakeup_count`, backstop `30`,
//! `mem`, backstop `0` (`kernel`). A SIGTERM/SIGINT sets a flag that the work checks between steps
//! (`interrupted`: the run ends as `sleep_helper_interrupted`); during the restore it is checked only
//! once the restore has finished, so a signal never leaves Reborn quiesced or the radios off.
// SPDX-License-Identifier: GPL-2.0-only
mod kernel;

use super::{
    admission::{
        admission, application_starting, battery_and_cable, failed_before_freeze, guard_record,
        operation_state, retry_later, spm_state,
    },
    fail, int_literal,
    receipt::{self, record, wake_irqs, Receipt, Stage},
    request, E, INTERRUPTED,
};
use kernel::enter;
pub use kernel::{Kernel, Pm};
use serde_json::{Map, Value};
use std::os::fd::{AsFd, BorrowedFd};
use std::{fs::File, io, os::unix::fs::MetadataExt, path::Path, sync::atomic::Ordering};
use y2p_core::lock;
use y2p_core::py::PyValue;
use y2p_core::sys::{set_signal, Disposition, Handler};
use y2p_core::{command, py::is_true, root::parse_int, suspend_text::is_decimal, Root};

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

extern "C" fn stop(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

/// Install `handler` for `sig` (SA_ONSTACK, no SA_RESTART: a blocking read or wait returns EINTR so
/// the flag is seen): the old action.
fn install(sig: libc::c_int, handler: extern "C" fn(libc::c_int)) -> Option<Disposition> {
    // SAFETY: the handlers (stop, alarm) only store to atomics.
    unsafe { set_signal(sig, Handler::Call(handler), libc::SA_ONSTACK) }.ok()
}

use super::{interrupted as check, MASKED};

/// `__product-sleep <fd>`: exit 0 slept and restored, 2 refused, 1 failed (also for a descriptor
/// it cannot use; nothing changed then). Nothing on stdout; a failure is one line on stderr.
pub fn main(root: &Root, args: &[String]) -> crate::reply::Outcome {
    const NAME: &str = "__product-sleep";
    let Some(text) = args.first() else {
        fail(NAME, &E::value("descriptor_required"))
    };
    let Some(fd) = parse_int(text).and_then(|fd| i32::try_from(fd).ok()) else {
        fail(NAME, &int_literal(text))
    };
    if fd < 0 {
        fail(
            NAME,
            &E::os(&io::Error::from_raw_os_error(libc::EBADF), None),
        )
    }
    install(libc::SIGTERM, stop);
    install(libc::SIGINT, stop);
    let result = execute(root, fd, &mut Kernel);
    // The descriptor is closed after the run whatever its outcome; one that was not open fails.
    // SAFETY: closing the descriptor this programme was given, which nothing else owns.
    let closed = unsafe { libc::close(fd) } == 0;
    match result {
        Ok(code) if closed => super::exit(code as i32),
        Ok(_) => fail(
            NAME,
            &E::os(&io::Error::from_raw_os_error(libc::EBADF), None),
        ),
        Err(error) => fail(NAME, &error),
    }
}

/// The descriptor is the request's `/run/y2/suspend.lock` (same device and inode) and its flock is
/// held: the helper sleeps only for the request that admitted it.
fn inherited(root: &Root, fd: i32) -> Result<(), E> {
    // SAFETY: the descriptor this programme was given stays open until it closes it after the run
    // (one that is not open fails fstat with EBADF).
    let inherited = unsafe { BorrowedFd::borrow_raw(fd) };
    let meta = y2p_core::sys::fstat(inherited)?;
    let path = root.path("/run/y2/suspend.lock");
    let lock = std::fs::metadata(&path).map_err(|e| E::os(&e, Some(&path)))?;
    if (meta.dev, meta.ino) != (lock.dev(), lock.ino()) {
        return Err(E::value("invalid_suspend_lock"));
    }
    match lock::try_fd(inherited, lock::Kind::Exclusive) {
        Ok(true) => Ok(()),
        Ok(false) => Err(E::os(&lock::would_block(), None)),
        Err(error) => Err(E::from(error)),
    }
}

/// The helper's deadlines run on the root's monotonic clock.
fn monotonic(root: &Root) -> f64 {
    root.clock().monotonic()
}

fn execute(root: &Root, fd: i32, pm: &mut dyn Pm) -> Result<i64, E> {
    check()?;
    // Require the actual inherited lock, never a user-supplied PID or stale file.
    inherited(root, fd)?;
    let mut run = Run::default();
    let body = run.body(root, pm);
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
        // Every other failure (an OS, value or runtime error, a record the helper cannot read) is
        // the product failure; the restore below runs either way.
        Err(error) => {
            run.result = 1;
            let failure = Stage::new("product_failure", Some(1)).reason(error.text());
            if let Err(error) = record(root, failure) {
                pending_error = Some(error);
            }
        }
    }
    MASKED.store(true, Ordering::SeqCst);
    let finished = run.finally(root, pm);
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
    fn body(&mut self, root: &Root, pm: &mut dyn Pm) -> Result<(), E> {
        self.held = operation_locks(root)?;
        check()?;
        admission(root)?;
        battery_and_cable(root)?;
        operation_state(root, false)?;
        let mut count = pm.wakeup_count(root)?;
        let mut previous_irqs = wake_irqs(root.read("/proc/interrupts").as_deref());
        guard_record(root, true, false)?;
        self.pending = true;
        marker(root, pm, "HELPER_REQUEST")?;
        request::acknowledge_prepare(root).map_err(preparation_refused)?;
        // Reborn has quiesced: from here on the restore always runs.
        self.prepared = true;
        request::record_prepared(root).map_err(preparation_refused)?;
        check()?;
        wait_for_application(root, pm, &previous_irqs)?;
        record(root, Stage::new("quiescing_radios", Some(0)))?;
        self.radios_changed = true;
        for radio in ["wifi", "bluetooth"] {
            quiesce_radio(root, pm, radio)?;
        }
        // The connectivity chip is off once its status says so (functions as 0 or 0x0).
        let stopped = |status: String| {
            ["powered=0 functions=0x0 ", "powered=0 functions=0 "]
                .iter()
                .any(|off| status.contains(off))
        };
        let deadline = monotonic(root) + 15.0;
        while !stopped(
            root.read("/sys/devices/platform/18070000.connectivity/status")
                .unwrap_or_default(),
        ) {
            if monotonic(root) >= deadline {
                return Err(E::refused("radio_power_down"));
            }
            pm.pause(0.1)?;
        }
        let mut stale_counts = 0;
        loop {
            check()?;
            // Reading can block until an event finishes. The write rejects any intervening
            // Power/RTC/USB wake before the kernel entry.
            if count.is_none() {
                count = pm.wakeup_count(root)?;
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
            if let Err(error) = pm.control(root, "/sys/power/wakeup_count", &format!("{current}\n"))
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
            if let Err(error) = enter(root, pm) {
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
            // The read after the return completes the handshake; its count is not used (the next
            // pass reads a fresh one).
            pm.wakeup_count(root)?;
            let value = record(root, Stage::new("kernel_resumed", Some(0)))?;
            self.restored_kernel = value.kernel_completed();
            if !self.restored_kernel {
                return Err(E::RuntimeError("kernel_exit_not_proven".into()));
            }
            if !timer_only(&value, &spm_state(root)) {
                break;
            }
            // No display, input reactivation, radio restore or library scan during a maintenance
            // return. Safety daemon gets a normal tick; the following admission also reads the
            // ADC directly.
            previous_irqs = value.wake_irqs_after();
            record(root, Stage::new("maintenance_wake", Some(0)))?;
            pm.pause(2.0)?;
            // The safety tick may publish a normal BAT0 notification. Begin a new handshake after
            // that pause; reusing the pre-tick count would reject an otherwise safe next cycle.
            // Preserve previous_irqs so Power/RTC during the pause still cancel entry, and the
            // atomic count store still rejects any later event.
            count = None;
        }
        Ok(())
    }

    /// The restore, after every outcome of `body`: the screen first, then the radios, the completion
    /// record and the quarantine guard. Each step runs even when an earlier one failed (a helper that
    /// stopped half-way would leave Reborn quiesced or the radios off); the first failure is returned
    /// afterwards. Every operation lock stays held until the radios and Reborn are restored.
    fn finally(&mut self, root: &Root, pm: &mut dyn Pm) -> Result<(), E> {
        let mut first: Option<E> = None;
        if self.prepared {
            // POWER-FINAL-17: screen first. Wi-Fi/Bluetooth firmware, patches and RF calibration
            // take ≈1.6 s (archive 110) and the UI used to wait for them; it does not depend on the
            // radios. A failed early restore is retried by the completion record, as before.
            // 4fed1b6a: nothing durable followed `kernel_resumed`; these two light rows say whether
            // a stall is in the screen restore or after it.
            progress(root, "screen_restoring");
            // F47: a refusal leaves the screen dark (it went dark with the request); only a wake —
            // Power or RTC during the preparation, or a real return from the kernel — lights it.
            // Lit after every refusal, a kernel that refused each sleep turned the screen on every
            // two minutes for hours. Failures still light it to explain.
            let dark =
                self.result == 2 && self.refusal.as_deref() != Some("wake_event_in_progress");
            let screen = screen_first(root, dark);
            let answered = screen.as_ref().is_some_and(|s| !s.is_empty());
            progress(
                root,
                if answered {
                    "screen_restored"
                } else {
                    "screen_restore_failed"
                },
            );
            // F24: nothing sleeps any more. A Play right after waking took Reborn's shared activity
            // lease and was refused as busy (ALSA -16) for the ≈1.6 s of the radio restore below;
            // wired playback does not need the radios. The other operation locks stay. (A lease
            // that cannot be released here is released when the helper exits.)
            if let Some((_, file)) = self
                .held
                .iter()
                .find(|(owner, _)| *owner == "activity_operation")
            {
                keep(&mut first, file.unlock().map_err(E::from));
            }
            // A radio that did not come back is a failed sleep, not a reason to stop restoring.
            if self.restore_radios(root, pm).is_err() {
                self.result = 1;
            }
            let complete = format!("complete_result_{}", self.result);
            let stage = Stage {
                restore_ack: screen,
                dark,
                ..Stage::new(&complete, Some(self.result))
            };
            match record(root, stage) {
                Ok(value) => {
                    if let Some(result) = value.failed_result() {
                        self.result = result.as_i64().unwrap_or(1);
                    }
                    if self.result == 0 {
                        keep(&mut first, marker(root, pm, "COMPLETE"));
                    }
                }
                Err(error) => {
                    self.result = 1;
                    // The completion record asks Reborn again when the early restore got no
                    // answer; one that failed may have stopped before it asked.
                    if !answered {
                        let _ = receipt::application_rpc(root, "sleep_restore", dark);
                    }
                    keep(&mut first, Err(error));
                }
            }
        }
        // Clear whenever no kernel attempt is left unproven: a refusal, a proven kernel return,
        // or any failure before kernel entry (12 left result 1 pending here, refusing all sleep
        // until a reflash).
        if self.pending && (self.restored_kernel || !self.attempted) {
            keep(&mut first, guard_record(root, false, false));
        }
        first.map_or(Ok(()), Err)
    }

    fn restore_radios(&self, root: &Root, pm: &mut dyn Pm) -> Result<(), E> {
        record(root, Stage::new("restoring_radios", Some(self.result)))?;
        marker(root, pm, "RADIOS_RESTORING")?;
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
        E::private_directory(parent)?;
        let file = lock::open(&path).map_err(|e| E::os(&e, Some(&path)))?;
        match lock::try_fd(file.as_fd(), lock::Kind::Exclusive) {
            Ok(true) => held.push((owner, file)),
            Ok(false) => {
                let meta = file.metadata()?;
                let holders = y2p_core::lock::flock_holders(root, &meta);
                let mut reason = owner.to_owned();
                if !holders.is_empty() {
                    reason = format!("{owner}:{}", holders.join(","));
                }
                return Err(E::refused(reason));
            }
            Err(error) => return Err(E::from(error)),
        }
    }
    Ok(held)
}

/// `command(ctx, argv)`: a platform service the sleep needs, bounded at 20 s.
fn service(root: &Root, argv: &[&str]) -> Result<(), E> {
    let outcome = command::run(root, argv, 20.0);
    check()?;
    if !outcome.ok {
        let name = Path::new(argv.first().copied().unwrap_or_default())
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
fn quiesce_radio(root: &Root, pm: &mut dyn Pm, radio: &str) -> Result<u32, E> {
    let argv = ["/usr/sbin/y2-radio", radio, "off-runtime"];
    let deadline = monotonic(root) + RADIO_QUIESCE_SECONDS;
    let mut attempts = 0;
    loop {
        attempts += 1;
        let outcome = command::run(root, &argv, 20.0);
        check()?;
        if outcome.ok {
            return Ok(attempts);
        }
        if monotonic(root) >= deadline {
            return Err(E::RuntimeError(format!(
                "sleep_service_failed:y2-radio:{radio}:attempts={attempts}"
            )));
        }
        pm.pause(1.0)?;
    }
}

/// Defer kernel entry until `application_ready`, bounded. A Power or RTC wake during the wait
/// cancels the sleep like any wake before entry.
fn wait_for_application(
    root: &Root,
    pm: &mut dyn Pm,
    previous_irqs: &Map<String, Value>,
) -> Result<(), E> {
    if !application_starting(root) {
        return Ok(());
    }
    record(root, Stage::new("waiting_application_ready", Some(0)))?;
    let deadline = monotonic(root) + APPLICATION_READY_WAIT_S;
    while application_starting(root) && monotonic(root) < deadline {
        pm.pause(0.25)?;
        if &wake_irqs(root.read("/proc/interrupts").as_deref()) != previous_irqs {
            return Err(E::refused("wake_event_in_progress"));
        }
    }
    Ok(())
}

/// Ask Reborn to restore before the radios are; `dark` keeps the screen off (a refused sleep nobody
/// woke, F47). `None` without an acknowledgement (the completion record asks again).
fn screen_first(root: &Root, dark: bool) -> Option<Map<String, Value>> {
    receipt::application_rpc(root, "sleep_restore", dark).ok()
}

/// One durable row in the sleep receipt without the sysfs reads of a full record (a stalled driver
/// could block those too). A row that cannot be written is left out.
fn progress(root: &Root, stage: &str) {
    let _ = receipt::progress(root, stage);
}

/// Keep the first failure of the restore; the later steps still run.
fn keep(first: &mut Option<E>, result: Result<(), E>) {
    if let Err(error) = result {
        first.get_or_insert(error);
    }
}

/// Reborn did not prepare (it said why, or it could not be asked): a refusal; any other failure
/// stays one.
fn preparation_refused(error: E) -> E {
    if error.is_value() {
        E::refused(error.text())
    } else if error.is_os() {
        E::refused("reborn_preparation_unavailable")
    } else {
        error
    }
}

fn marker(root: &Root, pm: &mut dyn Pm, stage: &str) -> Result<(), E> {
    pm.control(root, "/sys/firmware/y2_pm/stage", &format!("{stage}\n"))
}

fn timer_only(value: &Receipt, spm: &Map<String, Value>) -> bool {
    let empty = Map::new();
    let p = value.product().unwrap_or(&empty);
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

#[cfg(test)]
#[path = "helper_tests.rs"]
mod helper_tests;
