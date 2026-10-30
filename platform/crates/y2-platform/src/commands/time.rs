//! `time`, `time-remember`, `ntp-event`: whether the wall clock is good enough for TLS, the anchor a
//! BusyBox NTP sync leaves, and the durable floor a restart without Wi-Fi resumes from.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::{atomic_json, lock, private_directory, Failure, Outcome, Reply};
use serde::{Deserialize, Serialize};
use serde_json::{json, Number, Value};
pub use y2p_core::clock::wall;
use y2p_core::{
    command,
    json::{sorted_object, Object, ObjectExt},
    lenient::{self, BootId},
    py, Root,
};

/// The durable floor (time-last.json) is written at most once an hour.
pub(crate) const RETAIN_S: f64 = 3600.0;
pub(crate) const LAST: &str = "/data/system/platform/time-last.json";
/// The boot's clock anchor.
pub(crate) const ANCHOR: &str = "/run/y2/time.json";

/// A clock anchor as read back (/run/y2/time.json, the durable time-last.json): what the clock rules
/// look at. The record itself is printed and carried into the floor as it was written.
#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct Recorded {
    pub(crate) boot_id: BootId,
    /// As recorded (an integer stays one when the boot sets the clock back to it).
    #[serde(deserialize_with = "lenient::or_none")]
    pub(crate) epoch: Option<Number>,
    #[serde(deserialize_with = "lenient::number")]
    monotonic_s: Option<f64>,
    #[serde(deserialize_with = "lenient::number")]
    boottime_s: Option<f64>,
    #[serde(deserialize_with = "lenient::text")]
    pub(crate) source: Option<String>,
}

impl Recorded {
    pub(crate) fn read(root: &Root, name: &str) -> Recorded {
        lenient::read(root.json_object(name))
    }

    pub(crate) fn epoch(&self) -> Option<f64> {
        self.epoch.as_ref().and_then(Number::as_f64)
    }

    /// Whether the anchor was written this boot (an anchor without a boot id matches an unreadable
    /// one).
    pub(crate) fn this_boot(&self, root: &Root) -> bool {
        self.boot_id.is(root.boot_id().as_deref())
    }

    fn source_is(&self, sources: &[&str]) -> bool {
        self.source.as_deref().is_some_and(|s| sources.contains(&s))
    }
}

/// CLOCK_BOOTTIME in seconds as `root` reads it.
fn boottime_of(root: &Root) -> f64 {
    root.clock().boottime_ns() as f64 / 1e9
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
pub(crate) fn drift(anchor: &Recorded, now: f64, monotonic: f64, elapsed: f64) -> f64 {
    match (anchor.boottime_s, anchor.monotonic_s, anchor.epoch()) {
        (Some(since), _, Some(wall)) => now - wall - (elapsed - since),
        (None, Some(stamp), Some(wall)) => now - wall - (monotonic - stamp),
        _ => f64::INFINITY,
    }
}

pub(crate) fn floor(root: &Root) -> Option<i64> {
    root.integer("/etc/y2linux/build-epoch")
}

/// `y2-platform time`.
pub fn status(root: &Root) -> Outcome {
    let now = wall();
    Reply::unsorted(&status_at(root, now, json!(now)), 0)
}

/// What `time` prints, as a record for the readers in-process (the system section, the service's
/// readiness and update checks), without printing and parsing it.
pub(crate) fn record(root: &Root) -> Status {
    let now = wall();
    status_at(root, now, json!(now))
}

/// Whether the wall clock is good enough for TLS now (`time`'s `tls_ready`).
pub(crate) fn tls_ready(root: &Root) -> bool {
    let now = wall();
    status_at(root, now, json!(now)).tls_ready
}

/// The clock status at wall time `now`: TLS ready only for a plausible clock whose anchor of this
/// boot came from the network or a retained RTC and still agrees. `epoch` is `now` as it is printed
/// (the wall clock's float for `time`, the owner's integer for `time-set`).
pub(crate) fn status_at(root: &Root, now: f64, epoch: Value) -> Status {
    let (monotonic, elapsed) = (root.clock().monotonic(), boottime_of(root));
    let floor = floor(root);
    let valid = plausible(now, floor);
    let shown = root.json_object(ANCHOR);
    let anchor: Recorded = lenient::read(shown.clone());
    let coherent = anchor.this_boot(root)
        && anchor.epoch().is_some()
        && anchor
            .monotonic_s
            .is_some_and(|stamp| (0.0..=86_400.0).contains(&(monotonic - stamp)))
        && drift(&anchor, now, monotonic, elapsed).abs() <= 60.0;
    let ready = valid && coherent && anchor.source_is(&["ntp", "retained_rtc"]);
    Status {
        schema: 1,
        clock_sane: valid,
        tls_ready: ready,
        reason: if ready {
            "reasonable_clock"
        } else {
            "clock_not_established"
        },
        source: anchor.source,
        ntp_authenticated: false,
        epoch,
        build_epoch: floor,
        anchor: sorted_object(&shown),
        // A fixture root has no random pool of its own.
        entropy_ready: root.is_real().then(py::entropy_ready),
        rtc_retention_qualified: false,
    }
}

/// `y2-platform time`: whether the wall clock is good enough for TLS, and why.
#[derive(Serialize, Default, Clone)]
pub(crate) struct Status {
    schema: u8,
    clock_sane: bool,
    pub(crate) tls_ready: bool,
    reason: &'static str,
    /// The anchor's source (`ntp`, `retained_rtc`, `manual`, …).
    source: Option<String>,
    ntp_authenticated: bool,
    epoch: Value,
    build_epoch: Option<i64>,
    anchor: Object,
    pub(crate) entropy_ready: Option<bool>,
    rtc_retention_qualified: bool,
}

#[cfg(test)]
impl Status {
    /// A clock status with only these readings (tests).
    pub(crate) fn ready(tls_ready: bool, entropy_ready: bool) -> Status {
        Status {
            tls_ready,
            entropy_ready: Some(entropy_ready),
            ..Status::default()
        }
    }
}

/// `/run/y2/time.json` (and the durable floor) after an NTP sync; `ntp-event` prints it with the RTC
/// write's outcome when one ran.
#[derive(Serialize)]
struct Sync<'a> {
    schema: u8,
    boot_id: Option<String>,
    epoch: f64,
    monotonic_s: f64,
    boottime_s: f64,
    source: &'static str,
    stratum: i64,
    authenticated: bool,
    event: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    rtc_write: Option<bool>,
}

/// `y2-platform time-remember` (a shutdown step): keep the durable floor at the running clock,
/// only for a clock set from the network or by hand this boot that nobody stepped since, so a restart
/// without Wi-Fi resumes about where it was. Its own errors print `{"state": "Failed", "failure": …}`.
pub fn remember(root: &Root) -> Outcome {
    match remember_inner(root) {
        Ok(Some(value)) => Reply::unsorted(&value, 0),
        Ok(None) => Reply::unsorted(&Object::new().with("state", "Unchanged"), 0),
        Err(error) => Reply::unsorted(
            &Object::new()
                .with("state", "Failed")
                .with("failure", error.message()),
            1,
        ),
    }
}

fn remember_inner(root: &Root) -> Result<Option<Object>, Failure> {
    remember_floor(root, false)
}

/// Keep the durable floor (`time-last.json`) at the running clock while it is a good NTP or owner
/// time of this boot: the anchor plus `epoch`, `monotonic_s` and `boottime_s`. With `retain` a floor
/// of the same source written less than `RETAIN_S` ago is kept (the service daemon's hourly pass,
/// #53); `time-remember` (a shutdown step) always writes. The record written, if any.
pub(crate) fn remember_floor(root: &Root, retain: bool) -> Result<Option<Object>, Failure> {
    let (now, monotonic, elapsed) = (wall(), root.clock().monotonic(), boottime_of(root));
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    // Never over a record that a sync or the owner wrote meanwhile.
    let _lock = lock(&runtime.join("time.lock"))?;
    let written = root.json_object(ANCHOR);
    let anchor: Recorded = lenient::read(written.clone());
    if !anchor.source_is(&["ntp", "manual"])
        || !anchor.this_boot(root)
        || !plausible(now, floor(root))
        || drift(&anchor, now, monotonic, elapsed).abs() > 60.0
    {
        return Ok(None);
    }
    if retain {
        let previous = Recorded::read(root, LAST);
        if previous
            .epoch()
            .is_some_and(|last| (0.0..RETAIN_S).contains(&(now - last)))
            && previous.source == anchor.source
        {
            return Ok(None);
        }
    }
    let mut value = sorted_object(&written);
    value
        .set("epoch", now)
        .set("monotonic_s", monotonic)
        .set("boottime_s", elapsed);
    let directory = root.path("/data/system/platform");
    private_directory(&directory)?;
    atomic_json(&directory.join("time-last.json"), &value, true)?;
    Ok(Some(value))
}

/// `y2-platform ntp-event <event>` from BusyBox ntpd (`stratum` in the environment).
pub fn ntp_event(root: &Root, event: &str) -> Outcome {
    let now = wall();
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    // BusyBox explicitly permits overlapping step/stratum callbacks.
    let _lock = lock(&runtime.join("time.lock"))?;
    // ntpd's `stratum` (16, unsynchronised, when absent) as an integer of any size, or the refusal
    // naming the text; a value that is not UTF-8 reads with surrogate escapes (y2p_core::os_text).
    let stratum = match std::env::var_os("stratum") {
        None => 16,
        Some(raw) => {
            let text = y2p_core::os_text::fsdecode_os(&raw);
            y2p_core::root::parse_int(&text)
                .ok_or_else(|| Failure::value(format!("invalid stratum: {text:?}")))?
        }
    };
    if event == "unsync" || !(1..16).contains(&stratum) || !plausible(now, floor(root)) {
        return Reply::unsorted(
            &Object::new()
                .with("state", "Unavailable")
                .with("reason", "ntp_unsynchronized_or_implausible"),
            0,
        );
    }
    let mut value = Sync {
        schema: 1,
        boot_id: root.boot_id(),
        epoch: now,
        monotonic_s: root.clock().monotonic(),
        boottime_s: boottime_of(root),
        source: "ntp",
        stratum,
        authenticated: false,
        event,
        rtc_write: None,
    };
    atomic_json(&runtime.join("time.json"), &value, false)?;
    // Durable floor at most once an hour, plus the first sync (also the first after a time set by
    // hand, #53). No seeds or private credentials are present in this small record.
    // A floor without a number (none yet, too) is replaced.
    let previous = Recorded::read(root, LAST);
    if previous.epoch().is_none_or(|t| (now - t).abs() >= RETAIN_S)
        || previous.source.as_deref() != Some("ntp")
    {
        let directory = root.path("/data/system/platform");
        private_directory(&directory)?;
        atomic_json(&directory.join("time-last.json"), &value, true)?;
    }
    value.rtc_write = rtc_write(root);
    Reply::unsorted(&value, 0)
}

/// Source-backed default or an owner receipt admits normal NTP -> RTC synchronization. Mutable owner
/// policy still takes precedence.
pub(crate) fn rtc_write(root: &Root) -> Option<bool> {
    let rtc: RtcPolicy = lenient::read(match root.json("/data/system/platform/rtc-policy.json") {
        Some(Value::Object(map)) => map,
        _ => root.json_object("/etc/y2linux/rtc-policy.json"),
    });
    (rtc.write_enabled && (rtc.qualification_reference || rtc.source))
        .then(|| command::run(root, &["/bin/busybox", "hwclock", "-w", "-u"], 3.0).ok)
}

/// rtc-policy.json (the owner's in /data, else the shipped one): whether NTP time goes to the RTC.
#[derive(Deserialize, Default)]
#[serde(default)]
struct RtcPolicy {
    #[serde(deserialize_with = "lenient::is_true")]
    write_enabled: bool,
    /// Either names what qualified the RTC (any non-empty value).
    #[serde(deserialize_with = "lenient::truthy")]
    qualification_reference: bool,
    #[serde(deserialize_with = "lenient::truthy")]
    source: bool,
}
