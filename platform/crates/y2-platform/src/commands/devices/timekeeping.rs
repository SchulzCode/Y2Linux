//! The owner's and the boot's clock commands: `time-set` (#53), `time-bootstrap` and `rtc-alarm`.
//! `time`, `time-remember` and `ntp-event` are `commands/time.rs`, whose clock rules (plausibility,
//! drift, build floor) these share.
// SPDX-License-Identifier: GPL-2.0-only
use crate::{
    commands::time::{floor, plausible, rtc_write, same_boot, status_at, wall},
    reply::{atomic_json, lock, private_directory, write_text, Failure, Outcome, Reply},
};
use serde::Serialize;
use serde_json::{json, Value};
use y2p_core::{
    command,
    json::{Object, ObjectExt},
    py::number,
    Root,
};

/// CLOCK_BOOTTIME in seconds: it keeps counting while the player sleeps.
fn boottime(root: &Root) -> f64 {
    root.clock().boottime_ns() as f64 / 1e9
}

/// /run/y2/time.json (and the durable time-last.json): the boot's clock anchor, printed in this key
/// order; `time-set` adds what it did after the anchor was written.
#[derive(Serialize)]
struct Anchor {
    schema: u8,
    boot_id: Option<String>,
    /// The wall clock the anchor holds: the owner's integer, the running clock (a float) or the
    /// retained value as it was recorded.
    epoch: Value,
    monotonic_s: f64,
    boottime_s: f64,
    source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    rtc_write: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ntp_restart: Option<bool>,
}

impl Anchor {
    fn now(root: &Root, epoch: Value, source: &'static str) -> Anchor {
        Anchor {
            schema: 1,
            boot_id: root.boot_id(),
            epoch,
            monotonic_s: root.clock().monotonic(),
            boottime_s: boottime(root),
            source,
            rtc_write: None,
            ntp_restart: None,
        }
    }
}

/// `y2-platform time-set <epoch>`: the owner's own date and time (#53), for a player without Wi-Fi.
/// Within the build-date range only; it never makes the clock TLS ready (an owner's guess is not an
/// authenticated time), and the next network time replaces it. Remembered for the next boot, which
/// starts from it instead of the build date. Its refusals and storage errors print
/// `{"state": "Failed", "failure": …}` (exit 1); Reborn shows the failure.
pub fn set_manual(root: &Root, epoch: i64) -> Outcome {
    match set_manual_inner(root, epoch) {
        Ok(value) => Reply::unsorted(&value, 0),
        Err(error) => Reply::unsorted(
            &Object::new()
                .with("state", "Failed")
                .with("failure", error.message()),
            1,
        ),
    }
}

fn set_manual_inner(root: &Root, epoch: i64) -> Result<Anchor, Failure> {
    if !plausible(epoch as f64, floor(root)) {
        return Err(Failure::value("date_out_of_range"));
    }
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    let _lock = lock(&runtime.join("time.lock"))?;
    // Network time this boot: the clock is right, and the running time server would keep a hand-set
    // time for minutes while its periodic callback calls it network time (8.22 test build: 2 h off,
    // "ntp").
    let anchor = root.json_object("/run/y2/time.json");
    if anchor.get("source").and_then(Value::as_str) == Some("ntp")
        && same_boot(&anchor, &root.boot_id())
    {
        return Err(Failure::value("network_time_active"));
    }
    let set = format!("@{epoch}");
    if !command::run(root, &["/bin/busybox", "date", "-u", "-s", &set], 3.0).ok {
        return Err(Failure::value("date_not_set"));
    }
    let mut value = Anchor::now(root, json!(epoch), "manual");
    atomic_json(&runtime.join("time.json"), &value, false)?;
    // Readers of the status (scrobbling) see the untrusted clock at once.
    atomic_json(
        &runtime.join("time-status.json"),
        &status_at(root, epoch as f64, json!(epoch)),
        false,
    )?;
    let directory = root.path("/data/system/platform");
    private_directory(&directory)?;
    atomic_json(&directory.join("time-last.json"), &value, true)?;
    value.rtc_write = rtc_write(root);
    // A running, not yet synchronised ntpd took a 2 h jump for network time at its first sync and
    // kept it for minutes (8.22 test build); a fresh one steps at once. Without a reachable server
    // the owner's time stays, which is what it is for.
    if root
        .read("/run/y2/ntp.pid")
        .is_some_and(|pid| !pid.is_empty())
    {
        command::run(root, &["/etc/init.d/S43y2-time", "stop"], 5.0);
        value.ntp_restart = Some(command::run(root, &["/etc/init.d/S43y2-time", "start"], 5.0).ok);
    }
    Ok(value)
}

/// `y2-platform time-bootstrap` (S03y2-time, before the power service): the boot's first clock anchor.
/// It takes no time.lock and reads time-last.json without checking /data: nothing else runs yet, and
/// a missing or unverified /data only loses the retained floor.
pub fn bootstrap(root: &Root) -> Outcome {
    let mut now = json!(wall());
    let floor = floor(root);
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    let previous = root.json_object("/data/system/platform/time-last.json");
    let retained = previous.get("epoch");
    let retained_number = number(retained);
    let plausible_retained = retained_number.is_some_and(|r| plausible(r, floor));
    let rtc_date = root
        .read("/sys/class/rtc/rtc0/date")
        .is_some_and(|d| !d.is_empty());
    // The owner's own date and time (#53) carries over a restart as itself: a lower bound like the
    // build date, never TLS readiness.
    let manual = previous.get("source").and_then(Value::as_str) == Some("manual");
    let now_f = now.as_f64().unwrap_or_default();
    // Kernel hctosys supplied wall time. Only a plausible RTC following an earlier network clock
    // establishes retained-clock readiness; a build date alone is merely a lower bound and cannot
    // authorize TLS downloads.
    let source = if plausible(now_f, floor)
        && plausible_retained
        && retained_number.is_some_and(|r| now_f >= r - 60.0)
        && rtc_date
    {
        if manual {
            "manual"
        } else {
            "retained_rtc"
        }
    } else {
        let mut source = "build_floor";
        // The later of the build floor and a plausible retained time (the floor on a tie).
        let first = floor.unwrap_or(0);
        let (second, second_f) = match (plausible_retained, retained, retained_number) {
            (true, Some(value), Some(r)) => (value.clone(), r),
            _ => (json!(0), 0.0),
        };
        let (target, target_f) = if second_f > first as f64 {
            (second, second_f)
        } else {
            (json!(first), first as f64)
        };
        if !plausible(now_f, floor) && target_f != 0.0 {
            // `date` takes whole seconds: a fractional retained time is truncated toward zero.
            let set = format!("@{}", target_f.trunc() as i64);
            if command::run(root, &["/bin/busybox", "date", "-u", "-s", &set], 1.0).ok {
                now = target;
                if manual && retained_number == Some(target_f) {
                    source = "manual";
                }
            }
        }
        source
    };
    let value = Anchor::now(root, now, source);
    atomic_json(&runtime.join("time.json"), &value, false)?;
    Reply::unsorted(&value, 0)
}

/// What `rtc-alarm` prints and records (/run/y2/rtc-alarm.json), in this key order.
#[derive(Serialize)]
struct Alarm {
    schema: u8,
    boot_id: Option<String>,
    alarm_epoch: Option<i64>,
    state: &'static str,
    qualified: bool,
    reason: &'static str,
}

/// `y2-platform rtc-alarm <seconds>`: kernel RTC alarm plumbing; arming an alarm does not enter
/// suspend. The target uses the RTC's clock (since_epoch), not the system wall clock; 0 disables.
pub fn alarm(root: &Root, seconds: i64) -> Outcome {
    if !(0..=604_800).contains(&seconds) {
        return Err(Failure::value("rtc_alarm_seconds_out_of_range"));
    }
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    let _lock = lock(&runtime.join("rtc-alarm.lock"))?;
    let now = root
        .integer("/sys/class/rtc/rtc0/since_epoch")
        .filter(|n| *n > 0)
        .ok_or_else(|| Failure::value("rtc_time_unavailable"))?;
    let path = root.path("/sys/class/rtc/rtc0/wakealarm");
    // Linux requires disabling an existing alarm before replacement.
    write_text(&path, "0\n")?;
    let target = (seconds != 0).then(|| now.saturating_add(seconds));
    if let Some(target) = target {
        write_text(&path, &format!("{target}\n"))?;
    }
    // A disabled alarm reads back empty (or 0 on some drivers).
    let observed = root.integer("/sys/class/rtc/rtc0/wakealarm");
    if observed != Some(target.unwrap_or(0)) && !(target.is_none() && observed.is_none()) {
        write_text(&path, "0\n")?;
        return Err(Failure::value("rtc_alarm_readback_mismatch"));
    }
    let value = Alarm {
        schema: 1,
        boot_id: root.boot_id(),
        alarm_epoch: target,
        state: if target.is_some() {
            "Armed"
        } else {
            "Disabled"
        },
        qualified: false,
        reason: "same_boot_alarm_wake_PHYSICAL_PENDING",
    };
    atomic_json(&runtime.join("rtc-alarm.json"), &value, false)?;
    Reply::unsorted(&value, 0)
}

#[cfg(test)]
#[path = "timekeeping_tests.rs"]
mod timekeeping_tests;
