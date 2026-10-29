//! The service's slow work, each in its own process so that none can stall the loop (`media
//! reconcile`, `media verify`, `update auto-check`, `report work`, `firmware scan`): when each starts,
//! how long it may run, when it is tried again; and the hourly time floor, done in process.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    atomic_json, command, inventory, media, private_directory, process, py, radiofw, reports,
    sorted_object, this_boot, time, truthy, update, Child, Failure, Object, Py, Root, Start, Value,
    PLATFORM, RECONCILE_BACKOFF_S, RECONCILE_LIMIT_S, REPORT_CHECK_S, REPORT_FIRST_S,
    REPORT_LIMIT_S, REPORT_SEND_GAP_S, VERIFY_RETRY_S,
};
use y2p_core::json::ObjectExt;

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
    let lifecycle = root.json_object("/run/y2/media-lifecycle.json");
    let current = inventory(root);
    !py::eq(lifecycle.get("inventory"), Some(&current))
        || lifecycle.get("retry").is_some_and(truthy)
        || media::window_expired(root)
}

/// A child the service runs for one of its jobs (`{'process', 'started'}`), or when it may start one
/// again (`{'retry_at'}`; 0 for none).
#[derive(Default)]
pub(super) struct Run {
    pub(super) process: Option<(Child, f64)>,
    pub(super) retry_at: f64,
}

/// SD insertion/removal work (`media reconcile`) in its own process. It used to run inside this loop
/// with a 7 s deadline: a card whose check took longer was never mounted, and every pass started
/// another checker while the orphaned ones kept running (V1 code review B5).
pub(super) fn media_reconcile(root: &Root, run: Run, now: f64) -> Run {
    let mut run = run;
    if let Some((mut child, started)) = run.process.take() {
        if child.running() {
            if now - started < RECONCILE_LIMIT_S {
                run.process = Some((child, started));
                return run;
            }
            kill(child);
            return Run {
                process: None,
                retry_at: now + RECONCILE_BACKOFF_S,
            };
        }
        run = Run::default();
    }
    if now < run.retry_at || !media_reconcile_needed(root) {
        return run;
    }
    match start(root, &["media", "reconcile"]) {
        Ok(child) => Run {
            process: Some((child, now)),
            retry_at: 0.0,
        },
        Err(_) => Run::default(),
    }
}

pub(super) fn media_verify_needed(root: &Root) -> bool {
    let claim = root.json_object("/run/y2/media-mount.json");
    claim.get("read_only").and_then(Value::as_str) == Some("checking")
        && this_boot(root, claim.get("boot_id"))
}

/// The check of a card mounted while its check ran (media.verify) in its own process; it bounds its
/// checker itself (media.CHECK_SECONDS).
pub(super) fn media_verify(root: &Root, run: Run, now: f64) -> Run {
    let mut run = run;
    if let Some((mut child, started)) = run.process.take() {
        if child.running() {
            run.process = Some((child, started));
            return run;
        }
        run = Run {
            process: None,
            retry_at: now + VERIFY_RETRY_S,
        };
    }
    if now < run.retry_at || !media_verify_needed(root) {
        return run;
    }
    match start(root, &["media", "verify"]) {
        Ok(child) => Run {
            process: Some((child, now)),
            retry_at: 0.0,
        },
        Err(_) => Run {
            process: None,
            retry_at: now + VERIFY_RETRY_S,
        },
    }
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
    network: &Object,
    child: Option<Child>,
) -> Option<Child> {
    if let Some(mut child) = child {
        if child.running() {
            return Some(child);
        }
    }
    if network.get("state").and_then(Value::as_str) != Some("Online") {
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
    let retries = state.get("uploads");
    let values: Vec<Py> = queued
        .iter()
        .map(|name| match retries.and_then(|r| r.get(name)) {
            Some(entry @ Py::Dict(_)) => entry.get("next").cloned().unwrap_or(Py::int(0)),
            _ => Py::int(0),
        })
        .collect();
    if let Some(numbers) = values.iter().map(Py::number).collect::<Option<Vec<f64>>>() {
        return Ok(Due::At(numbers.into_iter().fold(f64::INFINITY, f64::min)));
    }
    match values.as_slice() {
        [Py::None] => Ok(Due::None),
        _ => Err(()),
    }
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
    network: &Object,
    run: Option<Reports>,
    now: f64,
) -> Result<Reports, Failure> {
    let mut run = run.unwrap_or(Reports {
        next_check: now + REPORT_FIRST_S,
        send_after: None,
        process: None,
    });
    if let Some((mut child, started)) = run.process.take() {
        if child.running() {
            if now - started < REPORT_LIMIT_S {
                run.process = Some((child, started));
                return Ok(run);
            }
            kill(child);
        }
    }
    if reports::upload_key(root).is_none()
        || !matches!(reports::state(root).get("enabled"), Some(Py::Bool(true)))
    {
        return Ok(run);
    }
    // An unreadable record never stops this loop.
    let Ok(due) = next_upload(root) else {
        return Ok(run);
    };
    let check = now >= run.next_check;
    let online = network.get("state").and_then(Value::as_str) == Some("Online");
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
    let (now, monotonic, elapsed) = (
        time::wall(),
        root.clock().monotonic(),
        root.clock().boottime_ns() as f64 / 1e9,
    );
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    // Never over a record that a sync or the owner wrote meanwhile.
    let _lock = crate::reply::lock(&runtime.join("time.lock"))?;
    let anchor = root.json_object("/run/y2/time.json");
    let source = anchor
        .get("source")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if !matches!(source.as_deref(), Some("ntp" | "manual"))
        || !time::same_boot(&anchor, &root.boot_id())
        || !time::plausible(now, time::floor(root))
        || time::drift(&anchor, now, monotonic, elapsed).abs() > 60.0
    {
        return Ok(());
    }
    let previous = root.json_object(time::LAST);
    let last = py::number(previous.get("epoch"));
    if last.is_some_and(|last| (0.0..time::RETAIN_S).contains(&(now - last)))
        && previous.get("source").and_then(Value::as_str) == source.as_deref()
    {
        return Ok(());
    }
    let mut value = sorted_object(&anchor);
    value
        .set("epoch", now)
        .set("monotonic_s", monotonic)
        .set("boottime_s", elapsed);
    let directory = root.path("/data/system/platform");
    private_directory(&directory)?;
    atomic_json(&directory.join("time-last.json"), &value, true)
}
