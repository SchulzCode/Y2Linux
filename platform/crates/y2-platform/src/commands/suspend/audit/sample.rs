//! What `y2-power-audit` reads at each end of the window: the cheap counters (`Sample`) and the heavy
//! description outside it (`Snapshot`), with the files and threads they hold.
// SPDX-License-Identifier: GPL-2.0-only
use super::super::{
    as_map, int_literal, journal::plain, monotonic, process_time, receipt, wall, E,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};
use y2p_core::{
    root::{fnmatch, parse_int, python_strip},
    suspend_text::{split, splitlines},
    Root,
};

const SNAPSHOT: [&str; 47] = [
    "/proc/stat",
    "/proc/interrupts",
    "/proc/softirqs",
    "/proc/uptime",
    "/proc/diskstats",
    "/proc/timer_list",
    "/proc/modules",
    "/proc/version",
    "/etc/os-release",
    "/sys/module/pwrap/parameters/*",
    "/sys/devices/system/cpu/online",
    "/sys/devices/system/cpu/cpu*/cpuidle/state*/*",
    "/sys/devices/system/cpu/cpufreq/policy0/scaling_*",
    "/sys/devices/system/cpu/cpufreq/policy0/stats/*",
    "/sys/module/idle/parameters/*",
    "/sys/module/clocks/parameters/*blockers",
    "/sys/module/spm/parameters/*",
    "/sys/bus/platform/devices/10006000*/state",
    "/sys/bus/platform/devices/10006000*/dormant_preflight",
    "/sys/bus/platform/devices/*/power/runtime_status",
    "/sys/bus/platform/devices/*/power/runtime_suspended_time",
    "/sys/bus/platform/devices/*/power/runtime_active_time",
    "/sys/bus/platform/devices/*/power/control",
    "/sys/bus/i2c/devices/*/power/runtime_status",
    "/sys/bus/platform/devices/11200000.usb/status",
    "/sys/bus/platform/devices/11200000.usb/rxdiag",
    "/sys/class/power_supply/*/uevent",
    "/sys/class/power_supply/BAT0/charging_state",
    "/sys/class/backlight/*/brightness",
    "/sys/class/backlight/*/bl_power",
    "/sys/class/rfkill/*/type",
    "/sys/class/rfkill/*/soft",
    "/sys/class/net/*/operstate",
    "/sys/class/net/*/statistics/*",
    "/proc/asound/card*/pcm*/sub*/status",
    "/sys/kernel/debug/clk/clk_summary",
    "/sys/kernel/debug/wakeup_sources",
    "/sys/kernel/debug/suspend_stats",
    "/sys/kernel/debug/dri/*/state",
    "/sys/class/udc/*/state",
    "/sys/class/udc/*/current_speed",
    "/run/y2/power.json",
    "/run/y2/cpu-idle-policy.json",
    "/run/y2/wifi-power.json",
    "/run/y2/bt-reconnect.json",
    "/run/y2/coexistence.json",
    "/run/y2/usb-transfer.json",
];
const SNAPSHOT_TAIL: [&str; 2] = ["/run/y2/sleep.json", "/run/y2/codec-runtime.json"];
const COUNTERS: [&str; 15] = [
    "/proc/stat",
    "/proc/interrupts",
    "/proc/softirqs",
    "/proc/diskstats",
    "/sys/devices/system/cpu/cpufreq/policy0/stats/time_in_state",
    "/sys/devices/system/cpu/cpu*/cpuidle/state*/usage",
    "/sys/devices/system/cpu/cpu*/cpuidle/state*/time",
    "/sys/devices/system/cpu/cpu*/cpuidle/state*/name",
    "/sys/module/idle/parameters/*",
    "/sys/module/clocks/parameters/*blockers",
    "/sys/bus/platform/devices/10006000*/state",
    "/sys/kernel/debug/wakeup_sources",
    "/sys/devices/system/cpu/online",
    "/sys/class/power_supply/BAT0/voltage_now",
    "/run/y2/power.json",
];

/// `Path(path).read_text().strip()`: strict UTF-8, any failure is None.
pub(super) fn read(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|t| python_strip(&t).to_owned())
}

/// `glob.glob(pattern)` below the root: hidden names only match a pattern that starts with a dot;
/// the listing order, not sorted. Device paths (without the root).
pub(super) fn glob(root: &Root, pattern: &str) -> Vec<String> {
    let base = root.path("/");
    let mut found: Vec<PathBuf> = vec![base.clone()];
    for component in pattern.split('/').filter(|c| !c.is_empty()) {
        let magic = component.contains(['*', '?', '[']);
        let mut next = Vec::new();
        for directory in &found {
            if !magic {
                let path = directory.join(component);
                if path.symlink_metadata().is_ok() {
                    next.push(path);
                }
                continue;
            }
            let Ok(entries) = fs::read_dir(directory) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                if name.starts_with('.') && !component.starts_with('.') {
                    continue;
                }
                if fnmatch(component, name) {
                    next.push(entry.path());
                }
            }
        }
        found = next;
    }
    found
        .iter()
        .filter_map(|p| p.strip_prefix(&base).ok())
        .map(|p| format!("/{}", p.display()))
        .collect()
}

/// Each file read (its device path) with its text, `None` when it could not be read.
pub(super) type Files = Vec<(String, Option<String>)>;

pub(super) fn files(root: &Root, patterns: &[&str]) -> Files {
    let mut result: Files = Vec::new();
    for pattern in patterns {
        let mut paths = glob(root, pattern);
        paths.sort();
        for path in paths {
            if root.path(&path).is_file() {
                let value = read(&root.path(&path));
                match result.iter_mut().find(|(k, _)| *k == path) {
                    Some(entry) => entry.1 = value,
                    None => result.push((path, value)),
                }
            }
        }
    }
    result
}

pub(super) fn get<'a>(files: &'a Files, key: &str) -> Option<&'a Option<String>> {
    files.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// One thread at one end of the window (`/proc/<pid>/task/<tid>/stat`, `status`, `wchan`).
#[derive(Serialize)]
pub(super) struct Thread {
    pub(super) name: String,
    pub(super) state: String,
    pub(super) utime: i64,
    pub(super) stime: i64,
    #[serde(rename = "context_switches", serialize_with = "as_map")]
    pub(super) switches: Vec<(String, i64)>,
    pub(super) wchan: Option<String>,
}

/// Threads by `pid:tid:starttime`.
pub(super) type Threads = Vec<(String, Thread)>;

pub(super) fn int(text: &str) -> Result<i64, E> {
    parse_int(text).ok_or_else(|| int_literal(text))
}

pub(super) fn processes(root: &Root) -> Result<Threads, E> {
    let mut result: Threads = Vec::new();
    for path in glob(root, "/proc/[0-9]*/task/[0-9]*/stat") {
        let Some(raw) = read(&root.path(&path)) else {
            continue;
        };
        let rest = raw.split_once('(').map_or("", |(_, r)| r);
        let (name, tail) = rest.rsplit_once(')').unwrap_or(("", rest));
        let parts = split(tail);
        if parts.len() < 20 {
            continue;
        }
        let sibling = |n: &str| root.path(&path).with_file_name(n);
        let status = read(&sibling("status")).unwrap_or_default();
        let mut switches: Vec<(String, i64)> = Vec::new();
        for line in splitlines(&status) {
            if line.contains("ctxt_switches:") {
                let mut pieces = line.split(':');
                let key = pieces.next().unwrap_or_default().to_owned();
                let value = int(pieces.next().unwrap_or_default())?;
                match switches.iter_mut().find(|(k, _)| *k == key) {
                    Some(entry) => entry.1 = value,
                    None => switches.push((key, value)),
                }
            }
        }
        let segments: Vec<&str> = path.split('/').collect();
        let key = format!("{}:{}:{}", segments[2], segments[4], parts[19]);
        let thread = Thread {
            name: name.to_owned(),
            state: parts[0].to_owned(),
            utime: int(parts[11])?,
            stime: int(parts[12])?,
            switches,
            wchan: read(&sibling("wchan")),
        };
        match result.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = thread,
            None => result.push((key, thread)),
        }
    }
    Ok(result)
}

/// What Reborn says of itself, outside the counter window; no media names, paths or peer IDs.
#[derive(Serialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)] // two values per run
pub(super) enum Application {
    /// The status request failed (its error class), or its sections were not objects.
    Unavailable { unavailable: &'static str },
    Status {
        screen_off: Value,
        playback_state: Value,
        scanning: Value,
        audio_state: Value,
        #[serde(serialize_with = "as_map")]
        graphics: Vec<(&'static str, Value)>,
    },
}

/// Reborn's `status` as the audit reads it: four sections (a missing one is empty) and whether its
/// screen is off, each value as Reborn gave it.
#[derive(Deserialize, Default)]
#[serde(default)]
struct RebornStatus {
    screen_off: Value,
    playback: Section,
    library: Section,
    audio: Section,
    graphics: Section,
}

/// A status section: its fields, or `None` when it is no object.
#[derive(Debug)]
struct Section(Option<serde_json::Map<String, Value>>);

impl Default for Section {
    fn default() -> Section {
        Section(Some(serde_json::Map::new()))
    }
}

impl<'de> Deserialize<'de> for Section {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Section, D::Error> {
        Ok(Section(match Value::deserialize(deserializer)? {
            Value::Object(fields) => Some(fields),
            _ => None,
        }))
    }
}

impl Section {
    fn field(&self, key: &str) -> Option<Value> {
        let fields = self.0.as_ref()?;
        Some(plain(fields.get(key).cloned().unwrap_or(Value::Null)))
    }
}

pub(super) fn application(root: &Root) -> Application {
    let status: RebornStatus = match receipt::application_rpc(root, "status", false) {
        Ok(status) => status,
        Err(error) => {
            return Application::Unavailable {
                unavailable: error.class(),
            }
        }
    };
    // A section that is not an object has no fields: the whole status is unavailable.
    let graphics = ["backend", "frames", "context_losses"]
        .into_iter()
        .map(|k| status.graphics.field(k).map(|v| (k, v)))
        .collect::<Option<Vec<_>>>();
    match (
        status.playback.field("state"),
        status.library.field("scanning"),
        status.audio.field("state"),
        graphics,
    ) {
        (Some(playback_state), Some(scanning), Some(audio_state), Some(graphics)) => {
            Application::Status {
                screen_off: plain(status.screen_off),
                playback_state,
                scanning,
                audio_state,
                graphics,
            }
        }
        _ => Application::Unavailable {
            unavailable: "AttributeError",
        },
    }
}

pub(super) fn release() -> String {
    // SAFETY: uname into a zeroed utsname.
    let mut name: libc::utsname = unsafe { std::mem::zeroed() };
    unsafe { libc::uname(&mut name) };
    // SAFETY: uname NUL-terminates the release.
    unsafe { std::ffi::CStr::from_ptr(name.release.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

/// The heavy description at each end, outside the counter window (`state_start`, `state_end`).
#[derive(Serialize)]
pub(super) struct Snapshot {
    pub(super) wall_time: f64,
    pub(super) monotonic: f64,
    pub(super) boot_id: Option<String>,
    pub(super) identity: Option<String>,
    pub(super) kernel: String,
    pub(super) application: Application,
    #[serde(serialize_with = "as_map")]
    pub(super) files: Files,
    #[serde(serialize_with = "as_map")]
    pub(super) threads: Threads,
    pub(super) collection_seconds: f64,
}

pub(super) fn snapshot(root: &Root) -> Result<Snapshot, E> {
    let start = monotonic();
    let wall_time = wall();
    let boot_id = read(&root.path("/proc/sys/kernel/random/boot_id"));
    let identity = read(&root.path("/etc/y2linux/versions.json"));
    let kernel = release();
    let application = application(root);
    let patterns: Vec<&str> = SNAPSHOT.iter().chain(&SNAPSHOT_TAIL).copied().collect();
    let files = files(root, &patterns);
    let threads = processes(root)?;
    Ok(Snapshot {
        wall_time,
        monotonic: start,
        boot_id,
        identity,
        kernel,
        application,
        files,
        threads,
        collection_seconds: monotonic() - start,
    })
}

/// The cheap counters at one end of the window (`start`, `end`).
#[derive(Serialize)]
pub(super) struct Sample {
    pub(super) boot_id: Option<String>,
    #[serde(serialize_with = "as_map")]
    pub(super) threads: Threads,
    #[serde(serialize_with = "as_map")]
    pub(super) files: Files,
    pub(super) monotonic: f64,
    #[serde(rename = "collection_seconds")]
    pub(super) collection: f64,
    #[serde(rename = "cpu_seconds")]
    pub(super) cpu: f64,
}

pub(super) fn counters(root: &Root) -> Result<Sample, E> {
    let start = monotonic();
    let cpu_start = process_time();
    let boot_id = read(&root.path("/proc/sys/kernel/random/boot_id"));
    let threads = processes(root)?;
    let files = files(root, &COUNTERS);
    let now = monotonic();
    Ok(Sample {
        boot_id,
        threads,
        files,
        monotonic: now,
        collection: now - start,
        cpu: process_time() - cpu_start,
    })
}

pub(super) fn file<'a>(sample: &'a Sample, key: &str) -> &'a str {
    get(&sample.files, key)
        .and_then(|v| v.as_deref())
        .unwrap_or_default()
}
