//! The service's slow work, each in its own process so that none can stall the loop (`media
//! reconcile`, `media verify`, `update auto-check`, `report work`, `firmware scan`): when each starts,
//! how long it may run, when it is tried again; and the hourly time floor, done in process.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    command, inventory, media, process, radiofw, reports, time, update, Child, Failure, Root,
    Start, Value, PLATFORM, RECONCILE_BACKOFF_S, RECONCILE_LIMIT_S, REPORT_CHECK_S, REPORT_FIRST_S,
    REPORT_LIMIT_S, REPORT_SEND_GAP_S, VERIFY_RETRY_S,
};

/// `y2-platform <args>` in its own session, its standard streams on /dev/null.
pub(super) fn start(root: &Root, args: &[&str]) -> Result<Child, Failure> {
    let mut argv = vec![PLATFORM];
    argv.extend_from_slice(args);
    process::spawn(root, &argv, Start::Service)
}

/// A started child of the service whose limit ran out: the whole tree is SIGKILLed (V1 code review B5)
/// and reaped for at most a second.
pub(super) fn kill(mut child: Child) {
    command::kill_tree(child.pid());
    child.wait(1.0);
}

/// Mirror media.reconcile's own early return without mounting anything. An expired write window
/// needs a reconcile too (its protect step): without one the card stayed writable until the next
/// insertion or removal.
pub(super) fn media_reconcile_needed(root: &Root) -> bool {
    !media::settled(&media::lifecycle(root), &inventory(root)) || media::window_expired(root)
}

/// A child the service runs for one of its jobs (`{'process', 'started'}`), or when it may start one
/// again (`{'retry_at'}`; 0 for none).
#[derive(Default)]
pub(super) struct Run {
    pub(super) process: Option<(Child, f64)>,
    pub(super) retry_at: f64,
}

/// What became of a job's child since the last pass.
#[derive(Debug, PartialEq)]
pub(super) enum Slot {
    /// None was running.
    Empty,
    /// Still running within its limit: it is kept.
    Running,
    /// It ended by itself.
    Ended,
    /// It ran past its limit: its whole tree was killed.
    Overrun,
}

/// Look at a job's child: keep it while it runs within `limit` seconds of its start (no limit: it
/// bounds itself), kill it once it ran longer, forget it once it ended.
pub(super) fn poll(process: &mut Option<(Child, f64)>, now: f64, limit: Option<f64>) -> Slot {
    let Some((mut child, started)) = process.take() else {
        return Slot::Empty;
    };
    if !child.running() {
        return Slot::Ended;
    }
    if limit.is_some_and(|limit| now - started >= limit) {
        kill(child);
        return Slot::Overrun;
    }
    *process = Some((child, started));
    Slot::Running
}

/// A periodic deadline of the loop (maintenance, NTP, the radio policy): due once `now` reaches it,
/// then again `period` later.
#[derive(Default)]
pub(super) struct Timer(f64);

impl Timer {
    pub(super) fn due(&mut self, now: f64, period: f64) -> bool {
        let due = now >= self.0;
        if due {
            self.0 = now + period;
        }
        due
    }
}

/// SD insertion/removal work (`media reconcile`) in its own process. It used to run inside this loop
/// with a 7 s deadline: a card whose check took longer was never mounted, and every pass started
/// another checker while the orphaned ones kept running (V1 code review B5).
pub(super) fn media_reconcile(root: &Root, run: Run, now: f64) -> Run {
    let mut run = run;
    match poll(&mut run.process, now, Some(RECONCILE_LIMIT_S)) {
        Slot::Running => return run,
        Slot::Overrun => {
            run.retry_at = now + RECONCILE_BACKOFF_S;
            return run;
        }
        Slot::Ended => run.retry_at = 0.0,
        Slot::Empty => {}
    }
    if now < run.retry_at || !media_reconcile_needed(root) {
        return run;
    }
    run.retry_at = 0.0;
    run.process = start(root, &["media", "reconcile"])
        .ok()
        .map(|child| (child, now));
    run
}

pub(super) fn media_verify_needed(root: &Root) -> bool {
    crate::commands::storage::records::MediaRecord::claim(root).checking(root)
}

/// The check of a card mounted while its check ran (media.verify) in its own process; it bounds its
/// checker itself (media.CHECK_SECONDS).
pub(super) fn media_verify(root: &Root, run: Run, now: f64) -> Run {
    let mut run = run;
    match poll(&mut run.process, now, None) {
        Slot::Running => return run,
        Slot::Ended | Slot::Overrun => run.retry_at = now + VERIFY_RETRY_S,
        Slot::Empty => {}
    }
    if now < run.retry_at || !media_verify_needed(root) {
        return run;
    }
    match start(root, &["media", "verify"]) {
        Ok(child) => (run.process, run.retry_at) = (Some((child, now)), 0.0),
        Err(_) => run.retry_at = now + VERIFY_RETRY_S,
    }
    run
}

/// True once health-ack reports a non-pending state for this boot.
pub(super) fn health_settled(answer: &command::Outcome) -> bool {
    if !answer.ok {
        return false;
    }
    match serde_json::from_str::<Value>(answer.output.as_deref().unwrap_or_default()) {
        Ok(Value::Object(value)) => {
            !matches!(value.get("state"), None | Some(Value::Null))
                && !matches!(
                    value.get("state").and_then(Value::as_str),
                    Some("PendingHealth" | "Failed")
                )
        }
        _ => false,
    }
}

/// Start the daily release check (update.automatic_check) in its own process when Wi-Fi is online and
/// it is due; the download may take seconds and must never stall this loop. Returns the still-running
/// child, if any.
pub(super) fn daily_update_check(
    root: &Root,
    network: &crate::commands::observation::snapshot::Wifi,
    child: Option<Child>,
) -> Option<Child> {
    if let Some(mut child) = child {
        if child.running() {
            return Some(child);
        }
    }
    if network.state != "Online" {
        return None;
    }
    let last = update::LastCheck::read(root);
    let now = time::wall();
    if update::within(last.checked(), now, update::CHECK_INTERVAL)
        || update::within(last.attempted(), now, update::ATTEMPT_INTERVAL)
    {
        return None;
    }
    start(root, &["update", "auto-check"]).ok()
}

/// When the earliest queued report may be sent.
pub(crate) enum Due {
    None,
    At(f64),
}

/// When the earliest queued report may be sent; Err when the retry times are not all numbers
/// (strings, a list): the record is unreadable.
pub(crate) fn next_upload(root: &Root) -> Result<Due, ()> {
    // A glob names each file once.
    let queued: Vec<String> = reports::queued(root)
        .iter()
        .map(|p| reports::name(p))
        .collect();
    if queued.is_empty() {
        return Ok(Due::None);
    }
    let state = reports::state(root);
    let next = queued
        .iter()
        .map(|name| state.uploads.get(name).map_or(0.0, |retry| retry.next))
        .fold(f64::INFINITY, f64::min);
    Ok(Due::At(next))
}

/// `error_reports`'s state: `{'next_check', 'send_after', 'process', 'started'}`.
pub(super) struct Reports {
    pub(super) next_check: f64,
    pub(super) send_after: Option<f64>,
    pub(super) process: Option<(Child, f64)>,
}

/// Start `report work` (new triggers, then the upload queue) in its own process: building a report
/// takes ≈10 s of every core and an upload waits on the network, so neither may stall this loop. Only
/// with reports on and an upload key in this build; due every REPORT_CHECK_S, or sooner when a queued
/// report may be sent and Wi-Fi is online.
pub(super) fn error_reports(
    root: &Root,
    network: &crate::commands::observation::snapshot::Wifi,
    run: Option<Reports>,
    now: f64,
) -> Result<Reports, Failure> {
    let mut run = run.unwrap_or(Reports {
        next_check: now + REPORT_FIRST_S,
        send_after: None,
        process: None,
    });
    if poll(&mut run.process, now, Some(REPORT_LIMIT_S)) == Slot::Running {
        return Ok(run);
    }
    if reports::upload_key(root).is_none() || !reports::state(root).enabled() {
        return Ok(run);
    }
    // An unreadable record never stops this loop.
    let Ok(due) = next_upload(root) else {
        return Ok(run);
    };
    let check = now >= run.next_check;
    let online = network.state == "Online";
    let send = match due {
        Due::None => false,
        _ if !online || now < run.send_after.unwrap_or(0.0) => false,
        Due::At(at) => at <= time::wall(),
    };
    if !(check || send) {
        return Ok(run);
    }
    if check {
        run.next_check = now + REPORT_CHECK_S;
    }
    run.send_after = Some(now + REPORT_SEND_GAP_S);
    if let Ok(child) = start(root, &["report", "work"]) {
        run.process = Some((child, now));
    }
    Ok(run)
}

/// While the player has no radio firmware, look for the owner's Firmware/system.img (and keep the
/// folder visible) in its own process: the extraction reads a 518 MB image and must never stall this
/// loop.
pub(super) fn radio_firmware(
    root: &Root,
    child: Option<Child>,
    ready: bool,
    maintenance: bool,
) -> (Option<Child>, bool) {
    // Re-checked every maintenance pass: firmware removed while running is noticed too (found by the
    // owner test of 49, archive 154), and so is a system.img copied after the firmware was installed:
    // the scan deletes it.
    if ready && !(maintenance && (!radiofw::present(root) || radiofw::leftover_waiting(root))) {
        return (child, true);
    }
    if let Some(mut child) = child {
        if child.running() {
            return (Some(child), false);
        }
    }
    let waiting = radiofw::candidates(root).iter().any(|path| path.exists());
    if radiofw::present(root) && !(maintenance && radiofw::leftover_waiting(root)) {
        return (None, true);
    }
    if !(waiting || maintenance) {
        return (None, false);
    }
    (start(root, &["firmware", "scan"]).ok(), false)
}

/// Keep the durable time floor (time-last.json) near the running clock, at most once an hour, only for a clock set from the network or by hand this boot that nobody stepped
/// since: a restart without Wi-Fi then resumes about where it was rather than from the last sync or the
/// build date.
pub(super) fn remember_time(root: &Root) -> Result<(), Failure> {
    time::remember_floor(root, true).map(drop)
}
