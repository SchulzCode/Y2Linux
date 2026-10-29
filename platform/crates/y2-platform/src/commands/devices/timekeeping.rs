//! The rest of `timekeeping.py`: `time-set` (set_manual, #53), `time-bootstrap` (bootstrap) and
//! `rtc-alarm` (alarm). `time`, `time-remember` and `ntp-event` are `commands/time.rs`; its private
//! helpers this module needs (status at a given time, rtc_write, drift) are copied here.
// SPDX-License-Identifier: GPL-2.0-only
use crate::{
    commands::time::{plausible, wall},
    reply::{atomic_json, lock, private_directory, write_text, Failure, Outcome, Reply},
};
use serde_json::{json, Map, Value};
use y2p_core::{
    clock, command,
    json::{Object, ObjectExt},
    py::{self, is_true, number, truthy},
    Root,
};

fn boottime() -> f64 {
    clock::boottime_ns() as f64 / 1e9
}

fn floor(root: &Root) -> Option<i64> {
    root.integer("/etc/y2linux/build-epoch")
}

fn boot_id(root: &Root) -> Option<String> {
    root.read("/proc/sys/kernel/random/boot_id")
}

/// `timekeeping.drift` (as time.rs).
fn drift(anchor: &Map<String, Value>, now: f64, monotonic: f64, elapsed: f64) -> f64 {
    let (stamp, wall, since) = (
        number(anchor.get("monotonic_s")),
        number(anchor.get("epoch")),
        number(anchor.get("boottime_s")),
    );
    match (since, stamp, wall) {
        (Some(since), _, Some(wall)) => now - wall - (elapsed - since),
        (None, Some(stamp), Some(wall)) => now - wall - (monotonic - stamp),
        _ => f64::INFINITY,
    }
}

/// `timekeeping.status(ctx, now=epoch)`: the status `time` prints, at the owner's time.
fn status_at(root: &Root, now: i64) -> Object {
    let (monotonic, elapsed) = (clock::monotonic(), boottime());
    let floor = floor(root);
    let valid = plausible(now as f64, floor);
    let anchor = root.json_object("/run/y2/time.json");
    let (stamp, wall_anchor) = (
        number(anchor.get("monotonic_s")),
        number(anchor.get("epoch")),
    );
    let coherent = anchor.get("boot_id").cloned().unwrap_or(Value::Null) == json!(boot_id(root))
        && wall_anchor.is_some()
        && stamp.is_some_and(|stamp| (0.0..=86_400.0).contains(&(monotonic - stamp)))
        && drift(&anchor, now as f64, monotonic, elapsed).abs() <= 60.0;
    let source = anchor.get("source").cloned().unwrap_or(Value::Null);
    let ready = valid && coherent && matches!(source.as_str(), Some("ntp" | "retained_rtc"));
    Object::new()
        .with("schema", 1)
        .with("clock_sane", valid)
        .with("tls_ready", ready)
        .with(
            "reason",
            if ready {
                "reasonable_clock"
            } else {
                "clock_not_established"
            },
        )
        .with("source", source)
        .with("ntp_authenticated", false)
        .with("epoch", now)
        .with("build_epoch", json!(floor))
        .with("anchor", Value::Object(anchor))
        .with(
            "entropy_ready",
            if root.is_real() {
                json!(py::entropy_ready())
            } else {
                Value::Null
            },
        )
        .with("rtc_retention_qualified", false)
}

/// `timekeeping.rtc_write` (as time.rs): a source-backed default or an owner receipt admits normal
/// NTP -> RTC synchronization. Mutable owner policy still takes precedence.
fn rtc_write(root: &Root, value: &mut Object) {
    let rtc = match root.json("/data/system/platform/rtc-policy.json") {
        Some(Value::Object(map)) => map,
        _ => root.json_object("/etc/y2linux/rtc-policy.json"),
    };
    let referenced = rtc.get("qualification_reference").is_some_and(truthy)
        || rtc.get("source").is_some_and(truthy);
    if is_true(rtc.get("write_enabled")) && referenced {
        let ok = command::run(root, &["/bin/busybox", "hwclock", "-w", "-u"], 3.0).ok;
        value.set("rtc_write", ok);
    }
}

/// `y2-platform time-set <epoch>`: the owner's own date and time (#53), for a player without Wi-Fi.
/// Within the build-date range only; it never makes the clock TLS ready (an owner's guess is not an
/// authenticated time), and the next network time replaces it. Remembered for the next boot, which
/// starts from it instead of the build date. Its errors print `{"state": "Failed", "failure": …}`.
pub fn set_manual(root: &Root, epoch: i64) -> Outcome {
    match set_manual_inner(root, epoch) {
        Ok(value) => Reply::unsorted(&value, 0),
        // `except (ValueError, OSError)`: anything else leaves the command.
        Err(error) if !error.caught() => Err(error),
        Err(error) => Reply::unsorted(
            &Object::new()
                .with("state", "Failed")
                .with("failure", error.message()),
            1,
        ),
    }
}

fn set_manual_inner(root: &Root, epoch: i64) -> Result<Object, Failure> {
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
        && anchor.get("boot_id").cloned().unwrap_or(Value::Null) == json!(boot_id(root))
    {
        return Err(Failure::value("network_time_active"));
    }
    let set = format!("@{epoch}");
    if !command::run(root, &["/bin/busybox", "date", "-u", "-s", &set], 3.0).ok {
        return Err(Failure::value("date_not_set"));
    }
    let mut value = Object::new()
        .with("schema", 1)
        .with("boot_id", json!(boot_id(root)))
        .with("epoch", epoch)
        .with("monotonic_s", clock::monotonic())
        .with("boottime_s", boottime())
        .with("source", "manual");
    atomic_json(&runtime.join("time.json"), &value, false)?;
    // Readers of the status (scrobbling) see the untrusted clock at once.
    atomic_json(
        &runtime.join("time-status.json"),
        &status_at(root, epoch),
        false,
    )?;
    let directory = root.path("/data/system/platform");
    private_directory(&directory)?;
    atomic_json(&directory.join("time-last.json"), &value, true)?;
    rtc_write(root, &mut value);
    // A running, not yet synchronised ntpd took a 2 h jump for network time at its first sync and
    // kept it for minutes (8.22 test build); a fresh one steps at once. Without a reachable server
    // the owner's time stays, which is what it is for.
    if root
        .read("/run/y2/ntp.pid")
        .is_some_and(|pid| !pid.is_empty())
    {
        command::run(root, &["/etc/init.d/S43y2-time", "stop"], 5.0);
        let ok = command::run(root, &["/etc/init.d/S43y2-time", "start"], 5.0).ok;
        value.set("ntp_restart", ok);
    }
    Ok(value)
}

/// `y2-platform time-bootstrap` (S03y2-time, before the power service): the boot's first clock anchor.
/// Takes no time.lock and reads time-last.json without a data_ready check, as Python.
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
        // max(floor or 0, retained if plausible_retained else 0): the first of equals wins.
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
            // str(int(target)): a float is truncated toward zero.
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
    let value = Object::new()
        .with("schema", 1)
        .with("boot_id", json!(boot_id(root)))
        .with("epoch", now)
        .with("monotonic_s", clock::monotonic())
        .with("boottime_s", boottime())
        .with("source", source);
    atomic_json(&runtime.join("time.json"), &value, false)?;
    Reply::unsorted(&value, 0)
}

/// `y2-platform rtc-alarm <seconds>`: kernel RTC alarm plumbing; arming an alarm does not enter
/// suspend. The target uses the RTC's clock (since_epoch), not the system wall clock.
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
    let target = if seconds != 0 {
        now.saturating_add(seconds)
    } else {
        0
    };
    if target != 0 {
        write_text(&path, &format!("{target}\n"))?;
    }
    let observed = root.integer("/sys/class/rtc/rtc0/wakealarm");
    let valid = observed == Some(target) || (target == 0 && observed.is_none());
    if !valid {
        write_text(&path, "0\n")?;
        return Err(Failure::value("rtc_alarm_readback_mismatch"));
    }
    let value = Object::new()
        .with("schema", 1)
        .with("boot_id", json!(boot_id(root)))
        .with(
            "alarm_epoch",
            if target != 0 {
                json!(target)
            } else {
                Value::Null
            },
        )
        .with("state", if target != 0 { "Armed" } else { "Disabled" })
        .with("qualified", false)
        .with("reason", "same_boot_alarm_wake_PHYSICAL_PENDING");
    atomic_json(&runtime.join("rtc-alarm.json"), &value, false)?;
    Reply::unsorted(&value, 0)
}

#[cfg(test)]
#[path = "timekeeping_tests.rs"]
mod timekeeping_tests;
