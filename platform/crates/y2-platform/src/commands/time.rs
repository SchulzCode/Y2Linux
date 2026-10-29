//! `timekeeping.py`: `time`, `time-remember`, `ntp-event` (reasonable-clock/TLS readiness; BusyBox NTP).
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::{atomic_json, lock, private_directory, Failure, Outcome, Reply};
use serde_json::{json, Map, Value};
use std::time::{SystemTime, UNIX_EPOCH};
use y2p_core::{
    clock, command,
    json::Ordered,
    py::{self, is_true, number, truthy},
    Root,
};

/// The durable floor (time-last.json) is written at most once an hour.
pub(crate) const RETAIN_S: f64 = 3600.0;
pub(crate) const LAST: &str = "/data/system/platform/time-last.json";

/// `time.time()`.
pub fn wall() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

pub(crate) fn boottime() -> f64 {
    clock::boottime_ns() as f64 / 1e9
}

pub fn plausible(epoch: f64, floor: Option<i64>) -> bool {
    let Some(floor) = floor.filter(|f| *f > 0) else {
        return false;
    };
    epoch.is_finite()
        && (floor - 86_400) as f64 <= epoch
        && epoch <= (floor + 5 * 366 * 86_400) as f64
}

/// How far the wall clock moved apart from the anchor since it was written. The wall clock keeps
/// running in suspend, CLOCK_MONOTONIC does not: after any sleep over 60 s the monotonic comparison
/// called a correct clock incoherent and every update check failed until ntpd's next callback (Beta 4:
/// "Connect to Wi-Fi" after sleep). Compare with CLOCK_BOOTTIME.
pub(crate) fn drift(anchor: &Map<String, Value>, now: f64, monotonic: f64, elapsed: f64) -> f64 {
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

pub(crate) fn floor(root: &Root) -> Option<i64> {
    root.integer("/etc/y2linux/build-epoch")
}

pub(crate) fn boot_id(root: &Root) -> Option<String> {
    root.read("/proc/sys/kernel/random/boot_id")
}

/// `anchor.get('boot_id') == ctx.read(boot_id)`: two missing values are equal too.
pub(crate) fn same_boot(anchor: &Map<String, Value>, boot: &Option<String>) -> bool {
    anchor.get("boot_id").cloned().unwrap_or(Value::Null) == json!(boot)
}

/// `y2-platform time`.
pub fn status(root: &Root) -> Outcome {
    let (now, monotonic, elapsed) = (wall(), clock::monotonic(), boottime());
    let floor = floor(root);
    let valid = plausible(now, floor);
    let anchor = root.json_object("/run/y2/time.json");
    let (stamp, wall_anchor) = (
        number(anchor.get("monotonic_s")),
        number(anchor.get("epoch")),
    );
    let coherent = same_boot(&anchor, &boot_id(root))
        && wall_anchor.is_some()
        && stamp.is_some_and(|stamp| (0.0..=86_400.0).contains(&(monotonic - stamp)))
        && drift(&anchor, now, monotonic, elapsed).abs() <= 60.0;
    let source = anchor.get("source").cloned().unwrap_or(Value::Null);
    let ready = valid && coherent && matches!(source.as_str(), Some("ntp" | "retained_rtc"));
    let value = Ordered::new()
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
        .with("rtc_retention_qualified", false);
    Reply::unsorted(&value, 0)
}

/// `y2-platform time-remember` (`remember(ctx, every=0)`): keep the durable floor at the running clock,
/// only for a clock set from the network or by hand this boot that nobody stepped since, so a restart
/// without Wi-Fi resumes about where it was. Its own errors print `{"state": "Failed", "failure": …}`.
pub fn remember(root: &Root) -> Outcome {
    match remember_inner(root) {
        Ok(Some(value)) => Reply::unsorted(&value, 0),
        Ok(None) => Reply::unsorted(&Ordered::new().with("state", "Unchanged"), 0),
        // `except (OSError, ValueError)`: anything else leaves the command.
        Err(error) if !error.caught() => Err(error),
        Err(error) => Reply::unsorted(
            &Ordered::new()
                .with("state", "Failed")
                .with("failure", error.message()),
            1,
        ),
    }
}

fn remember_inner(root: &Root) -> Result<Option<Ordered>, Failure> {
    let (now, monotonic, elapsed) = (wall(), clock::monotonic(), boottime());
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    // Never over a record that a sync or the owner wrote meanwhile.
    let _lock = lock(&runtime.join("time.lock"))?;
    let anchor = root.json_object("/run/y2/time.json");
    let source = anchor
        .get("source")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if !matches!(source.as_deref(), Some("ntp" | "manual"))
        || !same_boot(&anchor, &boot_id(root))
        || !plausible(now, floor(root))
        || drift(&anchor, now, monotonic, elapsed).abs() > 60.0
    {
        return Ok(None);
    }
    // `0 <= now - last < every` with every=0 never holds: the CLI always writes.
    let mut value = Ordered::from_map(&anchor);
    value
        .set("epoch", now)
        .set("monotonic_s", monotonic)
        .set("boottime_s", elapsed);
    let directory = root.path("/data/system/platform");
    private_directory(&directory)?;
    atomic_json(&directory.join("time-last.json"), &value, true)?;
    Ok(Some(value))
}

/// `int(text)` of a str: its value (one beyond 64 bits as the nearest 64-bit bound, which no range
/// check here tells apart), or the ValueError int() raises.
pub fn python_int(text: &str) -> Result<i64, Failure> {
    use crate::argparse::python;
    if let Some(value) = python::int(text) {
        return Ok(value.parse().unwrap_or(if value.starts_with('-') {
            i64::MIN
        } else {
            i64::MAX
        }));
    }
    // An int but for its length: CPython's digit limit (sys.get_int_max_str_digits(), 4300).
    let digits = text
        .chars()
        .filter(|c| python::decimal(*c).is_some())
        .count();
    let mut short = String::new();
    for c in text.chars() {
        if python::decimal(c).is_none() {
            short.push(c);
        } else if !short.ends_with('\u{1}') {
            short.push('\u{1}');
        }
    }
    if digits > 4300 && python::int(&short.replace('\u{1}', "1")).is_some() {
        return Err(Failure::Value(format!(
            "Exceeds the limit (4300 digits) for integer string conversion: value has {digits} digits; \
             use sys.set_int_max_str_digits() to increase the limit"
        )));
    }
    Err(Failure::Value(int_literal(&crate::argparse::python::repr(
        text,
    ))))
}

/// int()'s ValueError text for a repr: CPython formats it with `%.200R` (at most 200 characters).
fn int_literal(shown: &str) -> String {
    let shown: String = shown.chars().take(200).collect();
    format!("invalid literal for int() with base 10: {shown}")
}

/// `y2-platform ntp-event <event>` from BusyBox ntpd (`stratum` in the environment).
pub fn ntp_event(root: &Root, event: &str) -> Outcome {
    let now = wall();
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    // BusyBox explicitly permits overlapping step/stratum callbacks.
    let _lock = lock(&runtime.join("time.lock"))?;
    // `int(env.get('stratum', '16'))`: Python's int() of the text (any size), or its ValueError.
    // os.environ reads a value that is not UTF-8 with surrogate escapes (y2p_core::os_text).
    let stratum = match std::env::var_os("stratum") {
        None => 16,
        Some(raw) => python_int(&y2p_core::os_text::fsdecode_os(&raw))?,
    };
    if event == "unsync" || !(1..16).contains(&stratum) || !plausible(now, floor(root)) {
        return Reply::unsorted(
            &Ordered::new()
                .with("state", "Unavailable")
                .with("reason", "ntp_unsynchronized_or_implausible"),
            0,
        );
    }
    let mut value = Ordered::new()
        .with("schema", 1)
        .with("boot_id", json!(boot_id(root)))
        .with("epoch", now)
        .with("monotonic_s", clock::monotonic())
        .with("boottime_s", boottime())
        .with("source", "ntp")
        .with("stratum", stratum)
        .with("authenticated", false)
        .with("event", event);
    atomic_json(&runtime.join("time.json"), &value, false)?;
    // Durable floor at most once an hour, plus the first sync (also the first after a time set by
    // hand, #53). No seeds or private credentials are present in this small record.
    let previous = root.json_object(LAST);
    let previous_time = match previous.get("epoch") {
        None => Some(0.0),
        other => number(other),
    };
    if previous_time.is_none_or(|t| (now - t).abs() >= RETAIN_S)
        || previous.get("source").and_then(Value::as_str) != Some("ntp")
    {
        let directory = root.path("/data/system/platform");
        private_directory(&directory)?;
        atomic_json(&directory.join("time-last.json"), &value, true)?;
    }
    rtc_write(root, &mut value);
    Reply::unsorted(&value, 0)
}

/// Source-backed default or an owner receipt admits normal NTP -> RTC synchronization. Mutable owner
/// policy still takes precedence.
fn rtc_write(root: &Root, value: &mut Ordered) {
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
