//! report.py's `build`: everything the player records about an error (kernel, platform,
//! update/recovery and Reborn logs and state, hardware identity), every byte through the scrubber,
//! packed as one tar.gz within the receiver's 2 MiB.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    clock, json_dict,
    pyjson::{self, Py, Style},
    pyre,
    scrub::{self, Scrubber, SECTION_MAX},
    secrets, tarball,
};
use crate::reply::Failure;
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::{mpsc, Mutex},
};
use y2p_core::{clock as kernel, command, Root};

/// The compressed report (the receiver takes at most 2 MiB; real ones ≈0.5 MB).
pub const BUDGET: usize = 2 * 1024 * 1024;
pub const FOLDER: &str = "Error Reports";
/// Live progress of a report the owner started (Reborn's report screen reads it, like the update's
/// /run/y2/update-progress.json); removed when the run ends.
pub const PROGRESS: &str = "/run/y2/report-progress.json";
pub const TRIGGERS: [&str; 11] = [
    "manual",
    "kernel_crash",
    "kernel_warning",
    "unclean_restart",
    "reborn_crash",
    "reborn_failure",
    "update_failure",
    "sleep_failure",
    "radio_failure",
    "display_failure",
    "storage_error",
];
/// Bound on a file without a size (a seq_file) read through for its tail.
const READ_THROUGH_MAX: usize = 16 * 1024 * 1024;
/// Line-aligned pieces of a log scrubbed in parallel.
const PART: usize = 256 * 1024;
/// The Y2's cores.
const WORKERS: usize = 4;
/// The stock LK and preloader regions (production-partitions.json): which bootloader a player carries
/// (3.2.0 vs 3.2.1, #45/#46).
const BOOTLOADERS: [(&str, &str, u64, usize); 2] = [
    ("lk", "/dev/mmcblk0", 0x1d2_0000, 0x6_0000),
    ("preloader", "/dev/mmcblk0boot0", 0, 0x4_0000),
];

/// Live progress of one report run for the owner's screen: the step (collect, scrub, pack, then
/// upload or save), the whole run's percent and an estimate of the seconds left. At most five writes
/// a second, in RAM.
pub struct Progress {
    path: Option<PathBuf>,
    destination: Option<String>,
    start: f64,
    written: Cell<f64>,
}

impl Progress {
    pub fn new(root: &Root, destination: Option<&str>) -> Progress {
        Progress {
            path: Some(root.path(PROGRESS)),
            destination: destination.map(str::to_owned),
            start: kernel::monotonic(),
            written: Cell::new(0.0),
        }
    }

    /// `_quiet`: a run nobody watches.
    pub fn quiet() -> Progress {
        Progress {
            path: None,
            destination: None,
            start: 0.0,
            written: Cell::new(0.0),
        }
    }

    pub fn shown(&self) -> bool {
        self.path.is_some()
    }

    pub fn update(&self, phase: &str, fraction: f64, force: bool, fields: &[(&str, Py)]) {
        let Some(path) = &self.path else {
            return;
        };
        let now = kernel::monotonic();
        if !force && now - self.written.get() < 0.2 {
            return;
        }
        self.written.set(now);
        let (low, high) = match phase {
            "collect" => (0.0, 12.0),
            "scrub" => (12.0, 76.0),
            "pack" => (76.0, 82.0),
            _ => (82.0, 100.0), // upload, save
        };
        let percent = low + (high - low) * fraction.clamp(0.0, 1.0);
        let elapsed = now - self.start;
        let left = if percent >= 15.0 {
            Py::Int(format!(
                "{}",
                (elapsed * (100.0 - percent) / percent).round_ties_even() as i64
            ))
        } else {
            Py::None
        };
        let rounded: f64 = format!("{percent:.1}").parse().expect("number");
        let mut record = Py::dict()
            .with("schema", Py::int(1))
            .with("phase", Py::str(phase))
            .with("percent", Py::Float(rounded))
            .with("seconds_left", left)
            .with("destination", self.destination.clone().into());
        for (key, value) in fields {
            record.set(key, value.clone());
        }
        // The screen goes without; the report does not.
        let parent = path.parent().expect("parent");
        if std::fs::create_dir_all(parent).is_ok() || parent.is_dir() {
            let _ = y2p_core::record::atomic_json(path, &record, false, 0o600);
        }
    }

    pub fn close(&self) {
        if let Some(path) = &self.path {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// `_large_json`: ctx.json() reads at most 128 KiB; Reborn's session (the whole queue) is larger.
pub fn large_json(root: &Root, name: &str) -> Option<Py> {
    let text = root.read_limited(name, 16 * 1024 * 1024);
    match pyjson::loads(text.as_deref().filter(|t| !t.is_empty()).unwrap_or("null")) {
        Some(Py::None) | None => None,
        other => other,
    }
}

/// `_tail`: the newest `limit` bytes, from the first line start within 4 KiB.
pub fn tail(data: &[u8], limit: usize) -> (Vec<u8>, bool) {
    if data.len() <= limit {
        return (data.to_vec(), false);
    }
    let cut = &data[data.len() - limit..];
    match cut.iter().position(|&b| b == b'\n') {
        Some(newline) if newline < 4096 => (cut[newline + 1..].to_vec(), true),
        _ => (cut.to_vec(), true),
    }
}

/// `_read_bytes`.
fn read_bytes(root: &Root, name: &str) -> Option<(Vec<u8>, bool)> {
    let mut file = File::open(root.path(name)).ok()?;
    match file.seek(SeekFrom::End(0)) {
        Err(_) => {
            // seq_file (all of /proc but /proc/sys, debugfs) refuses SEEK_END: until 8.12 every such
            // file was silently left out. Read it through and keep the newest end.
            let (mut data, mut total) = (Vec::new(), 0usize);
            let mut chunk = vec![0u8; 65536];
            while total < READ_THROUGH_MAX {
                let n = file.read(&mut chunk).ok()?;
                if n == 0 {
                    break;
                }
                total += n;
                data.extend_from_slice(&chunk[..n]);
                let keep = SECTION_MAX + 4096;
                if data.len() > keep {
                    data.drain(..data.len() - keep);
                }
            }
            Some(tail(&data, SECTION_MAX))
        }
        Ok(size) => {
            file.seek(SeekFrom::Start(size.saturating_sub((SECTION_MAX + 4096) as u64)))
                .ok()?;
            let mut data = Vec::new();
            file.read_to_end(&mut data).ok()?;
            Some(tail(&data, SECTION_MAX))
        }
    }
}

fn read(root: &Root, name: &str) -> Py {
    root.read(name).into()
}

/// `_session_projection`: every setting and technical playback fact of session.json; no queue entries.
fn session_projection(session: Option<Py>) -> Py {
    let Some(session @ Py::Dict(_)) = session else {
        return Py::None;
    };
    let model = match session.get("model") {
        Some(m @ Py::Dict(_)) => m.clone(),
        _ => Py::dict(),
    };
    let queue = match model.get("queue") {
        Some(Py::List(q)) => q.clone(),
        _ => Vec::new(),
    };
    let position = model.get("queue_position").cloned().unwrap_or(Py::None);
    let index = match &position {
        Py::Int(_) => position.as_int(),
        Py::Bool(b) => Some(i64::from(*b)),
        _ => None,
    };
    let current = index
        .filter(|&i| 0 <= i && (i as usize) < queue.len())
        .map(|i| queue[i as usize].clone());
    const TECHNICAL: [&str; 9] = [
        "codec",
        "sample_rate",
        "channels",
        "bitrate",
        "bits_per_sample",
        "duration_ms",
        "source_id",
        "online",
        "artwork",
    ];
    let mut settings = match model.get("settings") {
        Some(s @ Py::Dict(_)) => s.clone(),
        _ => Py::dict(),
    };
    if let Some(Py::Str(zone)) = settings.get("time_zone") {
        let set = if zone.is_empty() { "" } else { "set" };
        settings.set("time_zone", Py::str(set));
    }
    let current_track = match current {
        Some(track @ Py::Dict(_)) => {
            let mut out = Py::dict();
            for key in TECHNICAL {
                if let Some(value) = track.get(key) {
                    out.set(key, value.clone());
                }
            }
            out
        }
        _ => Py::None,
    };
    let mut other: Vec<String> = match &model {
        Py::Dict(items) => items
            .iter()
            .map(|(k, _)| k.clone())
            .filter(|k| !matches!(k.as_str(), "queue" | "queue_entry_ids" | "settings"))
            .collect(),
        _ => Vec::new(),
    };
    other.sort();
    let get = |key: &str| model.get(key).cloned().unwrap_or(Py::None);
    Py::dict()
        .with("schema", session.get("schema").cloned().unwrap_or(Py::None))
        .with("settings", settings)
        .with("playback", get("playback"))
        .with("position_ms", get("position_ms"))
        .with("output", get("output"))
        .with("queue_length", Py::from(queue.len()))
        .with("queue_position", position)
        .with("current_track", current_track)
        .with("other_keys", Py::List(other.into_iter().map(Py::Str).collect()))
}

/// `_library_statistics`: counts only (sqlite read-only); never a name, title or path.
fn library_statistics(root: &Root) -> Py {
    use y2p_core::reports_sqlite::{Database, Error, Value};
    let path = root.path("/data/reborn/library.db");
    if !path.exists() {
        return Py::None;
    }
    let database = match Database::open(
        &format!("file:{}?mode=ro&immutable=1", path.to_string_lossy()),
        1000,
    ) {
        Ok(database) => database,
        Err(error) => return Py::dict().with("error", Py::str(error.class())),
    };
    let size = std::fs::metadata(&path).map_or(0, |m| m.len());
    let mut result = Py::dict().with("bytes", Py::from(size));
    let value = |v: &Value| match v {
        Value::Null => Py::None,
        Value::Integer(i) => Py::int(*i),
        Value::Real(f) => Py::Float(*f),
        Value::Text(t) => Py::str(t.clone()),
        Value::Blob(_) => Py::None,
    };
    let outcome = (|| -> Result<(), Error> {
        let tables: Vec<String> = database
            .query("select name from sqlite_master where type='table'")?
            .into_iter()
            .map(|row| match row.into_iter().next() {
                Some(Value::Text(t)) => t,
                _ => String::new(),
            })
            .collect();
        result.set("tables", Py::dict());
        for table in &tables {
            if !table.is_empty() && table.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                let rows = database.query(&format!("select count(*) from \"{table}\""))?;
                let count = rows.first().and_then(|r| r.first()).map_or(Py::None, value);
                if let Some(tables) = result.get_mut("tables") {
                    tables.set(table, count);
                }
            }
        }
        let columns: Vec<String> = if tables.iter().any(|t| t == "tracks") {
            database
                .query("pragma table_info(tracks)")?
                .into_iter()
                .filter_map(|row| match row.into_iter().nth(1) {
                    Some(Value::Text(t)) => Some(t),
                    _ => None,
                })
                .collect()
        } else {
            Vec::new()
        };
        for column in [
            "codec",
            "sample_rate",
            "channels",
            "source_id",
            "bits_per_sample",
            "online",
        ] {
            if columns.iter().any(|c| c == column) {
                let rows = database.query(&format!(
                    "select \"{column}\", count(*) from tracks group by 1 order by 2 desc limit 32"
                ))?;
                let list = rows
                    .iter()
                    .map(|row| {
                        let first = row.first().map_or(Py::None, value);
                        let count = row.get(1).map_or(Py::None, value);
                        Py::List(vec![first, count])
                    })
                    .collect();
                result.set(&format!("by_{column}"), Py::List(list));
            }
        }
        let version = database.query("pragma user_version")?;
        result.set(
            "user_version",
            version.first().and_then(|r| r.first()).map_or(Py::None, value),
        );
        Ok(())
    })();
    if let Err(error) = outcome {
        result.set("error", Py::str(error.class()));
    }
    // Source IDs are SD card filesystem IDs: keep their number, not the value.
    if let Some(Py::List(rows)) = result.get("by_source_id").cloned() {
        let renamed = rows
            .into_iter()
            .enumerate()
            .map(|(i, row)| {
                let count = match row {
                    Py::List(pair) => pair.get(1).cloned().unwrap_or(Py::None),
                    _ => Py::None,
                };
                Py::List(vec![Py::Str(format!("source#{}", i + 1)), count])
            })
            .collect();
        result.set("by_source_id", Py::List(renamed));
    }
    result
}

fn first(root: &Root, pattern: &str) -> Option<String> {
    root.glob(pattern).first().map(|p| rel(root, p))
}

/// `_rel(ctx, path)`: a glob result as the player path ctx.read() expects.
pub fn rel(root: &Root, path: &std::path::Path) -> String {
    let base = root.path("/");
    let relative = path.strip_prefix(&base).unwrap_or(path);
    format!("/{}", relative.to_string_lossy().trim_start_matches('/'))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `_hardware`.
fn hardware(root: &Root) -> Py {
    let compatible = root
        .read("/proc/device-tree/compatible")
        .unwrap_or_default()
        .replace('\0', " ");
    let mut data = Py::dict()
        .with("compatible", Py::str(pyre::strip(&compatible)))
        .with("cpuinfo", read(root, "/proc/cpuinfo"))
        .with("meminfo_total", Py::None);
    let mut boot = Py::dict();
    for name in ["metadata", "lcm", "normal_boot"] {
        boot.set(name, read(root, &format!("/sys/firmware/y2_boot/{name}")));
    }
    let mut panel = Py::dict();
    for name in ["panel_kind", "panel_hint", "power_down", "stock_suspend"] {
        panel.set(name, read(root, &format!("/sys/module/panel/parameters/{name}")));
    }
    data.set("y2_boot", boot);
    data.set("panel", panel);
    data.set("storage", Py::dict());
    data.set(
        "connectivity",
        read(root, "/sys/devices/platform/18070000.connectivity/status"),
    );
    data.set("input_devices", read(root, "/proc/bus/input/devices"));
    data.set("partitions", read(root, "/proc/partitions"));
    data.set("bootloaders", Py::dict());
    for line in pyre::splitlines(&root.read("/proc/meminfo").unwrap_or_default()) {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            data.set("meminfo_total", Py::str(pyre::strip(rest)));
        }
    }
    for host in ["mmc0", "mmc1"] {
        let Some(card) = first(root, &format!("/sys/class/mmc_host/{host}/{host}:*")) else {
            continue;
        };
        let mut fields = Py::dict();
        for name in [
            "type",
            "name",
            "manfid",
            "oemid",
            "prv",
            "rev",
            "fwrev",
            "hwrev",
            "date",
            "csd",
            "ocr",
            "life_time",
            "pre_eol_info",
            "enhanced_area_size",
            "erase_size",
            "preferred_erase_size",
        ] {
            fields.set(name, read(root, &format!("{card}/{name}")));
        }
        let ext_csd = first(root, &format!("/sys/kernel/debug/{host}/{host}:*/ext_csd"));
        fields.set(
            "ext_csd",
            ext_csd.map_or(Py::None, |p| read(root, &p)),
        );
        fields.set("ios", read(root, &format!("/sys/kernel/debug/{host}/ios")));
        if let Some(storage) = data.get_mut("storage") {
            storage.set(host, fields);
        }
    }
    for (name, device, offset, length) in BOOTLOADERS {
        let result = (|| -> std::io::Result<Vec<u8>> {
            let mut file = File::open(root.path(device))?;
            file.seek(SeekFrom::Start(offset))?;
            let mut blob = Vec::new();
            file.take(length as u64).read_to_end(&mut blob)?;
            Ok(blob)
        })();
        let entry = match result {
            Ok(blob) => Py::dict()
                .with("sha256", Py::Str(hex(&Sha256::digest(&blob))))
                .with("bytes", Py::from(blob.len()))
                .with(
                    "lk_v320_prefix_sha256",
                    if name == "lk" {
                        Py::Str(hex(&Sha256::digest(&blob[..blob.len().min(242_440)])))
                    } else {
                        Py::None
                    },
                ),
            Err(error) => Py::dict().with("error", Py::Str(strerror(&error))),
        };
        if let Some(loaders) = data.get_mut("bootloaders") {
            loaders.set(name, entry);
        }
    }
    data
}

/// `error.strerror`.
pub fn strerror(error: &std::io::Error) -> String {
    match error.raw_os_error() {
        // SAFETY: strerror returns a NUL-terminated static message.
        Some(errno) => unsafe { std::ffi::CStr::from_ptr(libc::strerror(errno)) }
            .to_string_lossy()
            .into_owned(),
        None => error.to_string(),
    }
}

/// `_commands`: live state from tools; each bounded.
const COMMANDS: [(&str, &[&str], f64, usize); 13] = [
    ("dmesg", &["dmesg"], 10.0, 2 << 20),
    ("platform_health", &["y2-platform", "health"], 10.0, 1 << 20),
    ("platform_status", &["y2-platform", "status"], 10.0, 1 << 20),
    ("update_status", &["y2-platform", "update", "status"], 10.0, 256 << 10),
    ("reborn_status", &["rebornctl", "status", "--json"], 5.0, 1 << 20),
    ("reborn_health", &["rebornctl", "health", "--json"], 5.0, 1 << 20),
    ("reborn_metrics", &["rebornctl", "metrics", "--json"], 5.0, 1 << 20),
    ("reborn_audio", &["rebornctl", "audio", "--json"], 5.0, 1 << 20),
    ("reborn_snapshot", &["rebornctl", "snapshot", "--json"], 5.0, 1 << 20),
    (
        "reborn_events",
        &["rebornctl", "events", "--last", "500", "--json"],
        5.0,
        2 << 20,
    ),
    (
        "processes",
        &["ps", "-o", "pid,ppid,stat,rss,vsz,time,args"],
        5.0,
        512 << 10,
    ),
    ("mounts_space", &["df", "-k"], 5.0, 64 << 10),
    ("amixer", &["amixer", "-c", "Y2Audio", "contents"], 5.0, 256 << 10),
];

/// Files copied whole (logs keep their newest end). Globs are bounded by ctx.glob.
const FILES: [&str; 68] = [
    "/sys/fs/pstore/*",
    "/data/logs/system.log",
    "/data/logs/system.log.0",
    "/data/logs/recovery-last.log",
    "/data/logs/recovery-display.txt",
    "/data/logs/recovery-previous-boot.txt",
    "/proc/version",
    "/proc/cmdline",
    "/proc/meminfo",
    "/proc/vmstat",
    "/proc/loadavg",
    "/proc/interrupts",
    "/proc/softirqs",
    "/proc/stat",
    "/proc/pressure/*",
    "/proc/mounts",
    "/proc/sys/kernel/tainted",
    "/sys/power/suspend_stats/*",
    "/sys/kernel/debug/wakeup_sources",
    "/sys/kernel/debug/suspend_stats",
    "/sys/kernel/debug/regulator/regulator_summary",
    "/sys/kernel/debug/clk/clk_summary",
    // The CS43131 at 0x30; its I2C bus number follows probe order (0 or 1).
    "/sys/kernel/debug/regmap/*-0030/registers",
    "/sys/bus/i2c/devices/*-0030/y2_hp/xtal",
    "/sys/devices/system/cpu/cpu*/cpuidle/state*/usage",
    "/sys/devices/system/cpu/cpu*/cpuidle/state*/time",
    "/sys/devices/system/cpu/cpu*/cpuidle/state*/name",
    "/sys/devices/system/cpu/cpufreq/policy0/stats/time_in_state",
    "/sys/devices/system/cpu/online",
    "/sys/class/thermal/thermal_zone*/temp",
    "/sys/class/thermal/thermal_zone*/type",
    "/sys/class/power_supply/*/uevent",
    "/sys/class/power_supply/BAT0/charging_state",
    "/sys/fs/ext4/*/errors_count",
    "/sys/fs/ext4/*/first_error_time",
    "/sys/devices/platform/11200000.usb/status",
    "/sys/devices/platform/11200000.usb/rxdiag",
    "/data/system/platform/boot.json",
    "/data/system/platform/previous-boot-evidence.json",
    "/data/system/platform/power-journal.jsonl",
    "/data/system/platform/sleep-diagnostics.jsonl",
    "/data/system/platform/suspend-last.json",
    "/data/system/platform/product-sleep-guard.json",
    "/data/system/platform/rtc-policy.json",
    "/data/system/platform/time-last.json",
    "/data/system/platform/battery-ui.json",
    "/data/system/platform/panel.json",
    "/data/system/platform/sd-admissions.json",
    "/data/system/platform/sd-repair-last.json",
    "/data/system/platform/wifi-journal*.jsonl",
    "/data/system/platform/retained/*.tar",
    "/data/updates/*.json",
    "/data/updates/current/manifest.json",
    "/data/updates/health/*.txt",
    "/run/y2/*.json",
    "/run/y2/*.jsonl",
    "/run/y2/*.log",
    "/data/bluetooth/codec-history.json",
    "/data/bluetooth/enabled",
    "/data/network/enabled",
    "/data/network/power-save.json",
    "/data/reborn/logs/*.jsonl",
    "/data/reborn/logs/panic-last.json",
    "/data/reborn/diagnostics/*.tar.gz",
    // (padding entries keep the array length explicit; never matched)
    "",
    "",
    "",
    "",
];

/// Never read into a report, even if a glob above ever matched them.
fn never(path: &str) -> bool {
    static CELL: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    pyre::lazy(
        &CELL,
        r"^/(media|data/music|data/ssh|root/\.ssh)/|wpa_supplicant|lastfm\.json|scrobbles\.jsonl|library\.db|/bluez/|machine-id|entropy|/keys?/|\.pem\n?\z|\.key\n?\z|session\.json",
    )
    .is_match(path)
}

/// `_parts`: a file as pieces that scrub independently: logs split at line ends (no pattern spans a
/// line), JSON documents and archives whole.
fn parts(path: &str, data: &[u8]) -> Vec<Vec<u8>> {
    if path.ends_with(".json") || path.ends_with(".tar") || path.ends_with(".tar.gz") || data.len() <= PART {
        return vec![data.to_vec()];
    }
    let (mut parts, mut start) = (Vec::new(), 0);
    while start < data.len() {
        let end = data[(start + PART).min(data.len())..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(data.len(), |i| start + PART + i + 1);
        parts.push(data[start..end].to_vec());
        start = end;
    }
    parts
}

/// `_scrub_files`: each (path, data) scrubbed on the cores; `progress(fraction)` follows the bytes
/// done.
fn scrub_files(
    scrubber: &mut Scrubber,
    items: &[(String, Vec<u8>)],
    progress: impl Fn(f64),
) -> Vec<Vec<u8>> {
    let mut jobs: Vec<(usize, &str, Vec<u8>)> = Vec::new();
    for (n, (path, data)) in items.iter().enumerate() {
        for part in parts(path, data) {
            jobs.push((n, path, part));
        }
    }
    let total = jobs.iter().map(|j| j.2.len()).sum::<usize>().max(1);
    // Big pieces first so no core waits on the last large one.
    let mut order: Vec<usize> = (0..jobs.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(jobs[i].2.len()));
    let next = Mutex::new(order.into_iter());
    let mut results: Vec<Option<Vec<u8>>> = vec![None; jobs.len()];
    let (sender, receiver) = mpsc::channel();
    std::thread::scope(|scope| {
        for _ in 0..WORKERS.min(jobs.len()) {
            let sender = sender.clone();
            let mut worker = scrubber.worker();
            let (jobs, next) = (&jobs, &next);
            scope.spawn(move || loop {
                let Some(i) = next.lock().expect("queue").next() else {
                    break;
                };
                let clean = scrub::blob(&mut worker, jobs[i].1, &jobs[i].2);
                if sender.send((i, clean, std::mem::take(&mut worker.counts))).is_err() {
                    break;
                }
            });
        }
        drop(sender);
        let mut done = 0;
        for (i, clean, counts) in receiver {
            done += jobs[i].2.len();
            results[i] = Some(clean);
            for (name, count) in counts {
                *scrubber.counts.entry(name).or_default() += count;
            }
            progress(done as f64 / total as f64);
        }
    });
    let mut cleaned = vec![Vec::new(); items.len()];
    for ((owner, _, _), clean) in jobs.iter().zip(results) {
        cleaned[*owner].extend(clean.expect("every job ran"));
    }
    cleaned
}

/// `_pack`: tar.gz within BUDGET: the largest optional sections are cut first.
fn pack(index: &mut Py, members: &mut [(String, Vec<u8>)]) -> Vec<u8> {
    let mut optional: Vec<usize> = (0..members.len())
        .filter(|&i| members[i].0.starts_with("files/"))
        .collect();
    optional.sort_by_key(|&i| std::cmp::Reverse(members[i].1.len()));
    loop {
        let mut tar = tarball::Writer::new();
        let body = pyjson::dumps(index, Style::Indent).into_bytes();
        let now = || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64)
        };
        tar.add(&tarball::Info::file("report.json", body.len(), now(), 0o600), &body);
        for (name, data) in members.iter() {
            tar.add(&tarball::Info::file(name, data.len(), now(), 0o600), data);
        }
        let packed = tarball::gzip(&tar.finish(), 6, 0);
        if packed.len() <= BUDGET || optional.is_empty() {
            return packed;
        }
        let victim = optional.remove(0);
        let (name, data) = &mut members[victim];
        let half = data.len() / 2;
        data.drain(..half);
        if let Some(section) = index.get_mut("sections").and_then(|s| s.get_mut(name)) {
            section.set("cut", Py::Bool(true));
            section.set("bytes", Py::from(data.len()));
        }
        optional.push(victim);
        optional.sort_by_key(|&i| std::cmp::Reverse(members[i].1.len()));
    }
}

fn random_key() -> Result<[u8; 16], Failure> {
    let mut key = [0u8; 16];
    let mut filled = 0;
    while filled < key.len() {
        // SAFETY: the pointer and length describe the unfilled tail of `key`.
        let n = unsafe { libc::getrandom(key[filled..].as_mut_ptr().cast(), key.len() - filled, 0) };
        if n < 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(super::os_plain(error));
        }
        filled += n as usize;
    }
    Ok(key)
}

/// `build(ctx, trigger, detail, topic, install_id, now, progress, triggers)`: the complete report
/// (tar.gz bytes) and its index.
pub fn build(
    root: &Root,
    triggers: &[String],
    detail: &str,
    topic: Option<&str>,
    install_id: Py,
    now: clock::Instant,
    progress: &Progress,
) -> Result<(Vec<u8>, Py), Failure> {
    if triggers.is_empty() || !triggers.iter().all(|t| TRIGGERS.contains(&t.as_str())) {
        return Err(Failure::value("unknown_trigger"));
    }
    progress.update("collect", 0.0, true, &[]);
    let known = secrets::secrets(root);
    let mut scrubber = Scrubber::new(&known, random_key()?);
    let versions = json_dict(root, "/etc/y2linux/versions.json");
    let release = json_dict(root, "/etc/y2linux/release.json");
    let uptime = root
        .read("/proc/uptime")
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "0".into());
    let uptime = uptime
        .split(pyre::isspace)
        .find(|s| !s.is_empty())
        .unwrap_or("0")
        .to_owned();
    let mut index = Py::dict()
        .with("schema", Py::int(1))
        .with("report_id", Py::Str(super::state::uuid4()?))
        .with("install_id", install_id)
        .with("trigger", Py::str(triggers[0].clone()))
        .with(
            "triggers",
            Py::List(triggers.iter().cloned().map(Py::Str).collect()),
        )
        .with("detail", Py::Str(scrubber.text(detail)))
        .with("topic", topic.into())
        .with("created_utc", Py::Str(clock::isoformat(now)))
        .with(
            "utc_offset_minutes",
            Py::int(clock::utc_offset_minutes(clock::Instant::now())),
        )
        .with("uptime_seconds", Py::Str(uptime))
        .with("boot_id", super::boot_id(root).into())
        .with(
            "release",
            if release.truthy() { release } else { versions.clone() },
        )
        .with("versions", versions)
        .with("build_id", root.read("/etc/y2linux/build-id").into())
        .with("kernel", root.read("/proc/sys/kernel/osrelease").into())
        .with("sections", Py::dict())
        .with("scrubbed", Py::dict());
    let mut members: Vec<(String, Vec<u8>)> = Vec::new();
    let mut add = |index: &mut Py, name: &str, data: Vec<u8>, source: Py, cut: bool| {
        if let Some(sections) = index.get_mut("sections") {
            sections.set(
                name,
                Py::dict()
                    .with("bytes", Py::from(data.len()))
                    .with("source", source)
                    .with("cut", Py::Bool(cut)),
            );
        }
        members.push((name.to_owned(), data));
    };
    let hardware = scrubber.value(&hardware(root), None);
    add(
        &mut index,
        "hardware.json",
        pyjson::dumps(&hardware, Style::Indent).into_bytes(),
        Py::str("sysfs"),
        false,
    );
    let session = session_projection(large_json(root, "/data/reborn/state/session.json"));
    let session = scrubber.value(&session, None);
    add(
        &mut index,
        "reborn/session.json",
        pyjson::dumps(&session, Style::Indent).into_bytes(),
        Py::str("session.json (projected)"),
        false,
    );
    add(
        &mut index,
        "reborn/library.json",
        pyjson::dumps(&library_statistics(root), Style::Indent).into_bytes(),
        Py::str("library.db (counts)"),
        false,
    );
    // The live commands (≈4 s, mostly waiting) run while the files scrub.
    let runner = {
        let root = root.clone();
        std::thread::spawn(move || {
            COMMANDS
                .iter()
                .map(|(name, argv, timeout, limit)| {
                    (*name, command::run_with(&root, argv, *timeout, *limit, true))
                })
                .collect::<Vec<_>>()
        })
    };
    let mut seen = std::collections::HashSet::new();
    let mut files: Vec<(String, Vec<u8>, bool)> = Vec::new();
    progress.update("collect", 0.3, false, &[]);
    let patterns: Vec<&str> = FILES.iter().copied().filter(|p| !p.is_empty()).collect();
    for (n, pattern) in patterns.iter().enumerate() {
        progress.update(
            "collect",
            0.3 + 0.7 * n as f64 / patterns.len() as f64,
            false,
            &[],
        );
        let paths = if pattern.contains('*') {
            root.glob(pattern)
        } else {
            vec![root.path(pattern)]
        };
        for path in paths {
            let relative = rel(root, &path);
            if seen.contains(&relative) || never(&relative) || !path.is_file() {
                continue;
            }
            seen.insert(relative.clone());
            if let Some((data, cut)) = read_bytes(root, &relative) {
                files.push((relative, data, cut));
            }
        }
    }
    progress.update("scrub", 0.0, true, &[]);
    let items: Vec<(String, Vec<u8>)> = files.iter().map(|(r, d, _)| (r.clone(), d.clone())).collect();
    let cleaned = scrub_files(&mut scrubber, &items, |fraction| {
        progress.update("scrub", fraction, false, &[])
    });
    progress.update("pack", 0.0, true, &[]);
    let commands = runner.join().expect("command runner");
    for (name, result) in commands {
        let output = result.output.clone().unwrap_or_default();
        let (data, cut) = tail(output.as_bytes(), SECTION_MAX);
        let clean = match pyjson::loads_bytes(&data) {
            Some(value) => pyjson::dumps(&scrubber.value(&value, None), Style::Indent).into_bytes(),
            None => scrubber.text(&String::from_utf8_lossy(&data)).into_bytes(),
        };
        let source = if result.ok {
            "command".to_owned()
        } else {
            format!(
                "command ({})",
                result.reason.as_deref().unwrap_or("None")
            )
        };
        add(&mut index, &format!("commands/{name}.txt"), clean, Py::Str(source), cut);
    }
    for ((relative, _, cut), clean) in files.iter().zip(cleaned) {
        // Scrubbed archives are re-packed uncompressed (the report itself is gzip).
        let name = relative.strip_suffix(".gz").filter(|_| relative.ends_with(".tar.gz")).unwrap_or(relative);
        add(&mut index, &format!("files{name}"), clean, Py::str(relative.clone()), *cut);
    }
    let counts = scrubber
        .counts
        .iter()
        .map(|(k, v)| (k.clone(), Py::from(*v)))
        .collect();
    index.set("scrubbed", Py::Dict(counts));
    let packed = pack(&mut index, &mut members);
    progress.update("pack", 1.0, true, &[]);
    Ok((packed, index))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tails_and_parts_follow_python() {
        let data = b"aaaa\nbbbb\ncccc";
        assert_eq!(tail(data, 100), (data.to_vec(), false));
        assert_eq!(tail(data, 7), (b"cccc".to_vec(), true));
        let log: Vec<u8> = (0..PART * 2).map(|i| if i % 100 == 99 { b'\n' } else { b'x' }).collect();
        let pieces = parts("/data/logs/system.log", &log);
        assert_eq!(pieces.concat(), log);
        assert!(pieces.iter().all(|p| p.ends_with(b"\n")));
        assert_eq!(parts("/run/y2/x.json", &log).len(), 1);
        assert!(never("/data/reborn/state/session.json") && never("/x/y.pem") && !never("/run/y2/power.json"));
    }
}
