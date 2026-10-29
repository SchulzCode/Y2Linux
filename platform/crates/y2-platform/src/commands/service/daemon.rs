//! `y2-platform service-daemon` (service.py): bounded readiness/maintenance work, separate from the
//! battery deadline owner. Every pass publishes `network.json` (Wi-Fi with the supplicant events, the
//! F52 self-heal and the DNS probe run in process), `coexistence.json` and `time-status.json`, ticks the
//! USB service and the Wi-Fi power policy, and starts the slow work in its own processes: `media
//! reconcile` and `media verify` (SD admission, F49), `firmware scan`, and on maintenance passes `space
//! --cleanup`, the hourly time floor (#53), the daily `update auto-check`, `report work`, `update
//! health-ack` and `boot-stage application_ready`.
//!
//! Below a fixture root the loop does not wait and ends after `Y2_DAEMON_TICKS` passes, and the
//! children are recorded, never started (process.rs), as tests/platform_cases/service.py
//! `daemon_ticks` stops the Python daemon.
// SPDX-License-Identifier: GPL-2.0-only
use super::process::{self, Child, Start};
use super::{radiofw, usb::UsbService};
use crate::commands::connectivity::{
    observe_wifi::wifi,
    radio_policy::wifi_policy,
    wifi::{probe_dns, DnsProbeSchedule, EventMonitor, WifiRecovery},
};
use crate::commands::reports::{pyjson::Py, state as reports};
use crate::commands::{
    storage::media::{self, inventory},
    time,
};
use crate::reply::{atomic_json, private_directory, Failure, Outcome};
use serde_json::{json, Map, Value};
use std::{
    env, io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
    sync::atomic::{AtomicBool, AtomicI32, Ordering},
};
use y2p_core::py::PyValue;
use y2p_core::sys::{set_signal, Disposition, Handler};
use y2p_core::{
    clock, command,
    json::{sorted_object, Object, ObjectExt},
    py::{self, is_true, truthy},
    root::read,
    Root,
};

// Fix02 background attribution: every iteration used to start a complete Python interpreter for
// `media reconcile` (every 3 s) plus two more every 30 s. On the Y2 that recurring start-up work drove
// schedutil to 1040 MHz and >10% samples during screen-off idle. The reconcile subprocess is still the
// only owner of mount/unmount work; it is now started only when its own no-op condition (unchanged SD
// inventory, no retry) is false.
const INTERVAL_S: f64 = 3.0;
/// Reborn: network.json stale after 15 s (an Online record after 45 s).
const DARK_INTERVAL_S: f64 = 10.0;
const MAINTENANCE_S: f64 = 30.0;
const DARK_MAINTENANCE_S: f64 = 300.0;
// A card's read-only admission check alone may take 60 s (media.read_only_check); a run that is still
// busy after this is stopped, and the same card is tried again only after RECONCILE_BACKOFF_S.
const RECONCILE_LIMIT_S: f64 = 150.0;
const RECONCILE_BACKOFF_S: f64 = 600.0;
/// A background check (`media verify`, F49) that ended without a verdict (the player slept, the card
/// was busy) starts again after this.
const VERIFY_RETRY_S: f64 = 30.0;
// Error reports (ERROR-REPORTS-PLAN): look for new triggers this often, the first time shortly after
// start (a crash record of the previous boot); a run that builds and sends reports is stopped after
// REPORT_LIMIT_S.
const REPORT_FIRST_S: f64 = 120.0;
const REPORT_CHECK_S: f64 = 900.0;
const REPORT_LIMIT_S: f64 = 600.0;
/// A blocked upload (battery, clock) is not retried every pass.
const REPORT_SEND_GAP_S: f64 = 300.0;
const PLATFORM: &str = "/usr/sbin/y2-platform";

/// `x == ctx.read(boot_id)` for a value from a record (None equals a missing boot id).
fn this_boot(root: &Root, value: Option<&Value>) -> bool {
    py::eq(value, Some(&json!(root.boot_id())))
}

/// `subprocess.Popen(['/usr/sbin/y2-platform', …], DEVNULL…, start_new_session=True, close_fds=True)`.
fn start(root: &Root, args: &[&str]) -> Result<Child, Failure> {
    let mut argv = vec![PLATFORM];
    argv.extend_from_slice(args);
    process::spawn(root, &argv, Start::Service)
}

/// A started child of the service whose limit ran out: the whole tree is SIGKILLed (V1 code review B5)
/// and reaped for at most a second.
fn kill(mut child: Child) {
    command::kill_tree(child.pid());
    child.wait(1.0);
}

/// Same meaning as the kernel's backlight_is_blank(): Reborn blanks with bl_power=4
/// (FB_BLANK_POWERDOWN) and leaves brightness unchanged.
fn display_dark(root: &Root) -> bool {
    let devices = root.glob("/sys/class/backlight/*");
    let read = |path: &std::path::Path| read(path, y2p_core::root::READ_LIMIT);
    !devices.is_empty()
        && devices.iter().all(|device| {
            let power = read(&device.join("bl_power"));
            let brightness = read(&device.join("brightness"));
            !matches!(power.as_deref(), None | Some("0")) || brightness.as_deref() == Some("0")
        })
}

/// Mirror media.reconcile's own early return without mounting anything. An expired write window
/// needs a reconcile too (its protect step): without one the card stayed writable until the next
/// insertion or removal.
fn media_reconcile_needed(root: &Root) -> bool {
    let lifecycle = root.json_object("/run/y2/media-lifecycle.json");
    let current = inventory(root);
    !py::eq(lifecycle.get("inventory"), Some(&current))
        || lifecycle.get("retry").is_some_and(truthy)
        || media::window_expired(root)
}

/// A child the service runs for one of its jobs (`{'process', 'started'}`), or when it may start one
/// again (`{'retry_at'}`; 0 for none).
#[derive(Default)]
struct Run {
    process: Option<(Child, f64)>,
    retry_at: f64,
}

/// SD insertion/removal work (`media reconcile`) in its own process. It used to run inside this loop
/// with a 7 s deadline: a card whose check took longer was never mounted, and every pass started
/// another checker while the orphaned ones kept running (V1 code review B5).
fn media_reconcile(root: &Root, run: Run, now: f64) -> Run {
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

fn media_verify_needed(root: &Root) -> bool {
    let claim = root.json_object("/run/y2/media-mount.json");
    claim.get("read_only").and_then(Value::as_str) == Some("checking")
        && this_boot(root, claim.get("boot_id"))
}

/// The check of a card mounted while its check ran (media.verify) in its own process; it bounds its
/// checker itself (media.CHECK_SECONDS).
fn media_verify(root: &Root, run: Run, now: f64) -> Run {
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
fn health_settled(answer: &command::Outcome) -> bool {
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
fn daily_update_check(root: &Root, network: &Object, child: Option<Child>) -> Option<Child> {
    if let Some(mut child) = child {
        if child.running() {
            return Some(child);
        }
    }
    if network.get("state").and_then(Value::as_str) != Some("Online") {
        return None;
    }
    let last = root.json_object("/data/updates/last-check.json");
    let now = time::wall();
    for (key, period) in [("time", 24.0 * 3600.0), ("attempt", 3600.0)] {
        // Only a check that reached the server starts the next day; ≤ 8.14 also stamped `time` on a
        // failed one (report ef22a23d: no check for a day).
        if key == "time" && last.get("result").and_then(Value::as_str) != Some("checked") {
            continue;
        }
        // type(stamp) in (int, float): not a bool.
        let stamp = match last.get(key) {
            Some(Value::Number(n)) => n.as_f64(),
            _ => None,
        };
        if stamp.is_some_and(|stamp| (0.0..period).contains(&(now - stamp))) {
            return None;
        }
    }
    start(root, &["update", "auto-check"]).ok()
}

/// `report.next_upload`: when the earliest queued report may be sent.
pub(crate) enum Due {
    None,
    At(f64),
    /// Not a number (`min()` of strings, a list): comparing it with the clock is a TypeError.
    Other,
}

/// `report.next_upload(ctx)`; Err for `min()` over values Python cannot order (a TypeError).
pub(crate) fn next_upload(root: &Root) -> Result<Due, ()> {
    // A glob names each file once: the set Python builds.
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
        [_] => Ok(Due::Other),
        many if many.iter().all(|v| matches!(v, Py::Str(_))) => Ok(Due::Other),
        _ => Err(()),
    }
}

/// `error_reports`'s state: `{'next_check', 'send_after', 'process', 'started'}`.
struct Reports {
    next_check: f64,
    send_after: Option<f64>,
    process: Option<(Child, f64)>,
}

/// Start `report work` (new triggers, then the upload queue) in its own process: building a report
/// takes ≈10 s of every core and an upload waits on the network, so neither may stall this loop. Only
/// with reports on and an upload key in this build; due every REPORT_CHECK_S, or sooner when a queued
/// report may be sent and Wi-Fi is online.
fn error_reports(
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
        // `due <= time.time()` of a string: a TypeError, the daemon ends.
        Due::Other => {
            return Err(Failure::crash(
                "TypeError",
                "'<=' not supported between instances of 'str' and 'float'",
            ))
        }
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
fn radio_firmware(
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

fn cadence(dark: bool, settled: bool) -> (f64, f64) {
    (
        if dark { DARK_INTERVAL_S } else { INTERVAL_S },
        if dark && settled {
            DARK_MAINTENANCE_S
        } else {
            MAINTENANCE_S
        },
    )
}

/// radio_policy.Coexistence: the Wi-Fi transfer rate and the Bluetooth link for the radio policy
/// (`/run/y2/coexistence.json`); provisional, no codec override.
#[derive(Default)]
struct Coexistence {
    previous: Option<(f64, f64)>,
}

impl Coexistence {
    fn observe(
        &mut self,
        root: &Root,
        wifi: &Object,
        now: f64,
    ) -> Result<Map<String, Value>, Failure> {
        let stats = match wifi.get("traffic_counters") {
            Some(Value::Object(stats)) => stats.clone(),
            Some(value) if truthy(value) => {
                return Err(Failure::attribute_error(format!(
                    "'{}' object has no attribute 'get'",
                    PyValue::type_name(value)
                )))
            }
            _ => Map::new(),
        };
        let mut total = 0.0;
        for key in ["rx_bytes", "tx_bytes"] {
            match stats.get(key) {
                Some(value) if !truthy(value) => {}
                None => {}
                Some(value) => {
                    total += py::number(Some(value)).ok_or_else(|| {
                        Failure::crash(
                            "TypeError",
                            format!(
                                "unsupported operand type(s) for +=: 'int' and '{}'",
                                PyValue::type_name(value)
                            ),
                        )
                    })?
                }
            }
        }
        let mut rate = None;
        if let Some((previous_time, previous_bytes)) = self.previous {
            if now > previous_time && total >= previous_bytes {
                rate = Some((total - previous_bytes) / (now - previous_time));
            }
        }
        self.previous = Some((now, total));
        let mut value = Map::new();
        value.insert("schema".into(), json!(1));
        value.insert("boot_id".into(), json!(root.boot_id()));
        value.insert("monotonic_s".into(), json!(now));
        value.insert(
            "wifi_active".into(),
            json!(wifi.get("state").and_then(Value::as_str) == Some("Online")),
        );
        value.insert("wifi_bytes_per_second".into(), json!(rate));
        value.insert(
            "wifi_heavy_transfer".into(),
            json!(rate.is_some_and(|r| r >= 1024.0 * 1024.0)),
        );
        value.insert(
            "cpu_pressure".into(),
            json!(root.read("/proc/pressure/cpu")),
        );
        value.insert(
            "bluetooth_link".into(),
            Value::Object(root.json_object("/run/y2/bt-reconnect.json")),
        );
        value.insert("provisional".into(), json!(true));
        value.insert("codec_bitrate_override".into(), Value::Null);
        atomic_json(&root.path("/run/y2/coexistence.json"), &value, false)?;
        Ok(value)
    }
}

#[cfg(test)]
#[path = "daemon_connectivity_tests.rs"]
mod connectivity_tests;

/// `timekeeping.remember(ctx)` (every=RETAIN_S): keep the durable floor near the running clock, at
/// most once an hour, only for a clock set from the network or by hand this boot that nobody stepped
/// since: a restart without Wi-Fi then resumes about where it was rather than from the last sync or the
/// build date.
fn remember_time(root: &Root) -> Result<(), Failure> {
    let (now, monotonic, elapsed) = (time::wall(), clock::monotonic(), time::boottime());
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

// ---- signals ------------------------------------------------------------------------------------

static RUNNING: AtomicBool = AtomicBool::new(true);
static WAKE_FD: AtomicI32 = AtomicI32::new(-1);

extern "C" fn on_signal(signal: libc::c_int) {
    if signal != libc::SIGUSR1 {
        RUNNING.store(false, Ordering::SeqCst);
    }
    let fd = WAKE_FD.load(Ordering::SeqCst);
    if fd >= 0 {
        let byte = signal as u8;
        // SAFETY: write(2) is async-signal-safe; a full pipe drops the byte (warn_on_full_buffer=False).
        unsafe { libc::write(fd, (&byte as *const u8).cast(), 1) };
    }
}

fn os(error: io::Error) -> Failure {
    let (text, class) = py::os_error(&error, None);
    Failure::Os { text, class }
}

/// `service.SignalWake`: wake the same select as supplicant events; retain signals until drained.
/// SIGTERM and SIGINT end the loop, SIGUSR1 (usb.wake_service: re-read the media views now) only wakes
/// it. The previous handlers come back on drop.
struct SignalWake {
    read: OwnedFd,
    _write: OwnedFd,
    previous: Vec<(libc::c_int, Disposition)>,
}

impl SignalWake {
    fn new() -> Result<Self, Failure> {
        let mut fds = [0; 2];
        // SAFETY: pipe2 fills two descriptors this struct then owns.
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) } != 0 {
            return Err(os(io::Error::last_os_error()));
        }
        // SAFETY: both descriptors were just created and are owned once.
        let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        WAKE_FD.store(write.as_raw_fd(), Ordering::SeqCst);
        let mut previous = Vec::new();
        for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGUSR1] {
            // SAFETY: a plain handler that only touches atomics and write(2); the old action is kept.
            if let Ok(old) =
                unsafe { set_signal(signal, Handler::Call(on_signal), libc::SA_RESTART) }
            {
                previous.push((signal, old));
            }
        }
        Ok(Self {
            read,
            _write: write,
            previous,
        })
    }

    fn fd(&self) -> RawFd {
        self.read.as_raw_fd()
    }

    fn drain(&self) {
        let mut buffer = [0u8; 4096];
        // SAFETY: reading into a local buffer from our own non-blocking pipe.
        unsafe { libc::read(self.fd(), buffer.as_mut_ptr().cast(), buffer.len()) };
    }
}

impl Drop for SignalWake {
    fn drop(&mut self) {
        for (signal, old) in &self.previous {
            old.restore(*signal);
        }
        WAKE_FD.store(-1, Ordering::SeqCst);
    }
}

// ---- the loop -----------------------------------------------------------------------------------

/// What the loop keeps between passes.
struct Service {
    root: Root,
    monitor: EventMonitor,
    recovery: WifiRecovery,
    usb: UsbService,
}

impl Service {
    fn serve(&mut self, ticks: Option<u64>) -> Result<(), Failure> {
        let root = self.root.clone();
        let root = &root;
        let runtime = root.path("/run/y2");
        let wake = SignalWake::new()?;
        let mut dns_schedule = DnsProbeSchedule::new();
        let (mut next_maintenance, mut next_ntp) = (0.0, 0.0);
        let mut associating_since: Option<f64> = None;
        let mut coexistence = Coexistence::default();
        let mut next_radio_policy = 0.0;
        let mut settled = false;
        let previous = root.json_object("/run/y2/network.json");
        // isinstance(failures, int) and failures >= 0 (a bool is an int).
        let mut query_failures: Value = match previous.get("status_query_failures") {
            _ if !this_boot(root, previous.get("boot_id")) => json!(0),
            None => json!(0),
            Some(value @ Value::Bool(_)) => value.clone(),
            Some(value) if py::exact_int(Some(value)).is_some_and(|n| n >= 0) => value.clone(),
            Some(_) => json!(0),
        };
        let mut update_check: Option<Child> = None;
        let mut reports: Option<Reports> = None;
        let (mut reconcile, mut verify) = (Run::default(), Run::default());
        let (mut firmware_scan, mut firmware_ready): (Option<Child>, bool) = (None, false);
        let mut done = 0u64;
        while RUNNING.load(Ordering::SeqCst) {
            wake.drain();
            let before = clock::monotonic();
            let dark = display_dark(root);
            let (mut interval, maintenance) = cadence(dark, settled);
            self.monitor.poll();
            self.usb.tick()?;
            let mut value = wifi(root, None);
            if value.get("state").and_then(Value::as_str) == Some("Associating") {
                let since = *associating_since.get_or_insert(before);
                if before - since > 30.0 {
                    value
                        .set("state", "Failed")
                        .set("reason", "authentication_timeout");
                }
            } else {
                associating_since = None;
            }
            let epoch = root.read("/run/y2/wifi-epoch");
            if dns_schedule.due(&value, epoch.as_deref(), before) {
                let probed = probe_dns(root, &value)?;
                if probed.is_some_and(|p| is_true(p.get("ok"))) {
                    dns_schedule.confirmed(before);
                }
                value = wifi(root, Some(&value));
            }
            let observation_failed = match value.get("supplicant_observation") {
                Some(Value::Object(o)) => matches!(o.get("ok"), Some(Value::Bool(false))),
                _ => false,
            };
            if value.get("radio_present").is_some_and(truthy) && observation_failed {
                query_failures = json!(py::number(Some(&query_failures)).unwrap_or(0.0) as i64 + 1);
            }
            self.recovery.observe(&value, before, Some(&self.monitor));
            value
                .set("status_query_failures", query_failures.clone())
                .set("schema", 1)
                .set("boot_id", json!(root.boot_id()))
                .set("monotonic_s", clock::monotonic())
                .set(
                    "supplicant_events",
                    self.monitor.summary(None).into_sorted(),
                );
            atomic_json(&runtime.join("network.json"), &value, false)?;
            let activity = coexistence.observe(root, &value, before)?;
            if before >= next_radio_policy
                && value.get("association").and_then(Value::as_str) == Some("COMPLETED")
            {
                next_radio_policy = before + 30.0;
                wifi_policy(root, Some(&activity))?;
            }
            atomic_json(
                &runtime.join("time-status.json"),
                &super::time_status(root),
                false,
            )?;
            reconcile = media_reconcile(root, reconcile, before);
            verify = media_verify(root, verify, before);
            if value.get("state").and_then(Value::as_str) == Some("Online") && before >= next_ntp {
                next_ntp = before + 60.0;
                command::run(root, &["/etc/init.d/S43y2-time", "start"], 1.0);
            }
            let due = before >= next_maintenance;
            (firmware_scan, firmware_ready) =
                radio_firmware(root, firmware_scan, firmware_ready, due);
            if due {
                next_maintenance = before + maintenance;
                // Run slow fs/stat/cleanup outside this readiness process. A kernel D-state does not
                // block Wi-Fi or the power daemon.
                command::run(root, &[PLATFORM, "space", "--cleanup"], 2.0);
                // A restart without Wi-Fi resumes near this clock (#53); hourly.
                match remember_time(root) {
                    Err(Failure::Os { .. }) | Ok(()) => {}
                    Err(other) => return Err(other),
                }
                update_check = daily_update_check(root, &value, update_check);
                reports = Some(error_reports(root, &value, reports, before)?);
                settled = settled
                    || health_settled(&command::run(
                        root,
                        &[PLATFORM, "update", "health-ack"],
                        8.0,
                    ));
                let ready = root.json_object("/run/y2/application-ready.json");
                let journal = root.json_object("/data/system/platform/boot.json");
                if this_boot(root, ready.get("boot_id"))
                    && is_true(ready.get("first_frame"))
                    && matches!(
                        journal.get("last_stage").and_then(Value::as_str),
                        Some("platform_start" | "services_started")
                    )
                {
                    command::run(root, &[PLATFORM, "boot-stage", "application_ready"], 2.0);
                }
            }
            if matches!(
                value.get("state").and_then(Value::as_str),
                Some("Scanning" | "Associating" | "AcquiringIP")
            ) {
                interval = interval.min(1.0);
            }
            if ticks.is_none() {
                let left = interval - (clock::monotonic() - before);
                self.monitor.wait(left.max(0.05), Some(wake.fd()));
            }
            done += 1;
            if ticks.is_some_and(|n| done >= n) {
                break;
            }
        }
        drop(wake);
        Ok(())
    }
}

/// `y2-platform service-daemon`: prints nothing and exits 0 when stopped (SIGTERM/SIGINT); a second
/// instance is refused (BlockingIOError) before it starts anything. An error escaping the loop ends
/// it (exit 1: main.py's record, or a traceback for what Python would not catch) after the USB gadget
/// and the responder are stopped; the supervisor restarts it.
pub fn serve(root: &Root) -> Outcome {
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    let path = runtime.join("service.lock");
    let lock = y2p_core::lock::try_exclusive(&path)
        .map_err(|e| Failure::os(e, &path))?
        .ok_or_else(|| os(io::Error::from_raw_os_error(libc::EWOULDBLOCK)))?;
    let ticks = if root.is_real() {
        None
    } else {
        env::var("Y2_DAEMON_TICKS")
            .ok()
            .and_then(|t| t.parse().ok())
    };
    let mut service = Service {
        root: root.clone(),
        monitor: EventMonitor::new(root),
        recovery: WifiRecovery::new(root),
        usb: UsbService::new(root),
    };
    let mut result = service.serve(ticks);
    // finally: the USB service stops first; its error replaces the loop's.
    if let Err(error) = service.usb.stop() {
        result = Err(error);
    }
    service.monitor.close();
    drop(lock);
    match result {
        // A daemon prints nothing (main.rs would print a newline for a reply).
        Ok(()) => std::process::exit(0),
        // What Python would not catch (TypeError, AttributeError, TimeoutExpired): a traceback on
        // stderr, no stdout, exit 1.
        Err(error @ Failure::Crash { .. }) => {
            let _ = std::io::Write::write_all(&mut std::io::stderr(), error.traceback().as_bytes());
            std::process::exit(1)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
#[path = "daemon_tests.rs"]
mod daemon_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_ack_answers_settle_the_cadence() {
        let answer = |ok: bool, output: &str| command::Outcome {
            ok,
            reason: None,
            output: Some(output.into()),
        };
        assert!(health_settled(&answer(
            true,
            r#"{"state": "Acknowledged"}"#
        )));
        assert!(health_settled(&answer(true, r#"{"state": ["x"]}"#)));
        assert!(!health_settled(&answer(
            true,
            r#"{"state": "PendingHealth"}"#
        )));
        assert!(!health_settled(&answer(true, r#"{"state": null}"#)));
        assert!(!health_settled(&answer(true, r#"{"other": 1}"#)));
        assert!(!health_settled(&answer(true, "[1]")));
        assert!(!health_settled(&answer(false, r#"{"state": "Idle"}"#)));
        assert_eq!(cadence(true, true), (10.0, 300.0));
        assert_eq!(cadence(true, false), (10.0, 30.0));
        assert_eq!(cadence(false, true), (3.0, 30.0));
    }

    #[test]
    fn a_second_instance_is_refused() {
        let dir = std::env::temp_dir().join(format!("y2-service-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("run/y2")).unwrap();
        let held = y2p_core::lock::exclusive(&dir.join("run/y2/service.lock")).unwrap();
        let error = serve(&Root::new(&dir)).err().expect("refused");
        assert_eq!(
            error.message(),
            "[Errno 11] Resource temporarily unavailable"
        );
        assert!(matches!(
            error,
            Failure::Os {
                class: "BlockingIOError",
                ..
            }
        ));
        drop(held);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
#[path = "daemon_reports_tests.rs"]
mod daemon_reports_tests;

#[cfg(test)]
#[path = "daemon_wake_tests.rs"]
mod daemon_wake_tests;
