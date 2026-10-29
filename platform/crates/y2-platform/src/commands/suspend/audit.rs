//! `y2-power-audit`: endpoint-only observation, no activity during
//! the measured sleep window. Counters are read at both ends of the window, heavy descriptions
//! outside it; the result goes to `--output` (created exclusively, compact JSON in insertion order).
//!
//! The arguments are the script's argparse parser (`--seconds`, `--settle`, `--state`, `--output`,
//! `--help`; abbreviations, `--x=y`, argparse's errors and texts at 80 columns, `prog` y2-power-audit),
//! then its `bounded windows only` refusal (exit 2).
// SPDX-License-Identifier: GPL-2.0-only
use super::{as_map, journal::plain, monotonic, process_time, receipt, traceback, wall, E};
use crate::{argparse::python, reply::Outcome};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
use y2p_core::{
    json::{dumps, Separators},
    root::{fnmatch, python_int, python_strip},
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

struct Args {
    seconds: f64,
    settle: f64,
    state: String,
    output: String,
}

const USAGE: &str = "usage: y2-power-audit [-h] [--seconds SECONDS] [--settle SETTLE] --state STATE
                      --output OUTPUT
";
const HELP: &str = "usage: y2-power-audit [-h] [--seconds SECONDS] [--settle SETTLE] --state STATE
                      --output OUTPUT

options:
  -h, --help         show this help message and exit
  --seconds SECONDS
  --settle SETTLE
  --state STATE
  --output OUTPUT
";

/// The script's parser: `-h/--help`, `--seconds` and `--settle` (float), `--state` and `--output`
/// (required), in argparse's lookup order (option string, action).
const OPTIONS: [(&str, usize); 6] = [
    ("-h", 0),
    ("--help", 0),
    ("--seconds", 1),
    ("--settle", 2),
    ("--state", 3),
    ("--output", 4),
];
const NAMES: [&str; 5] = ["-h/--help", "--seconds", "--settle", "--state", "--output"];

/// How argparse ends the script before it runs: `--help` (stdout, exit 0) or `p.error()` (exit 2).
enum Stop {
    Help,
    Error(String),
}

/// An option string match: action (None: unknown), option string, separator, explicit argument.
#[derive(Clone)]
struct Tuple {
    action: Option<usize>,
    option: String,
    sep: Option<String>,
    explicit: Option<String>,
}

fn lookup(option: &str) -> Option<usize> {
    OPTIONS.iter().find(|(o, _)| *o == option).map(|(_, a)| *a)
}

fn argument_error(action: usize, message: &str) -> Stop {
    Stop::Error(format!("argument {}: {message}", NAMES[action]))
}

/// `_parse_optional`: None for an argument, otherwise the option tuples.
fn parse_optional(arg: &str) -> Option<Vec<Tuple>> {
    let tuple = |action, option: &str, sep: Option<&str>, explicit: Option<&str>| Tuple {
        action,
        option: option.into(),
        sep: sep.map(str::to_owned),
        explicit: explicit.map(str::to_owned),
    };
    if !arg.starts_with('-') {
        return None;
    }
    if let Some(action) = lookup(arg) {
        return Some(vec![tuple(Some(action), arg, None, None)]);
    }
    let text: Vec<char> = arg.chars().collect();
    if text.len() == 1 {
        return None;
    }
    if let Some((option, explicit)) = arg.split_once('=') {
        if let Some(action) = lookup(option) {
            return Some(vec![tuple(Some(action), option, Some("="), Some(explicit))]);
        }
    }
    // _get_option_tuples (allow_abbrev).
    let (prefix, sep, explicit) = match arg.split_once('=') {
        Some((prefix, explicit)) => (prefix, Some("="), Some(explicit)),
        None => (arg, None, None),
    };
    let mut found = Vec::new();
    let short: String = text[..2].iter().collect();
    let short_explicit: String = text[2..].iter().collect();
    for (option, action) in OPTIONS {
        if text[1] != '-' && option == short {
            found.push(tuple(Some(action), option, Some(""), Some(&short_explicit)));
        } else if option.starts_with(prefix) {
            found.push(tuple(Some(action), option, sep, explicit));
        }
    }
    if !found.is_empty() {
        return Some(found);
    }
    // `-\.?\d`: the parser has no option that looks like a negative number.
    let digit = |c: Option<&char>| c.is_some_and(|&c| python::decimal(c).is_some());
    if digit(text.get(1)) || (text[1] == '.' && digit(text.get(2))) || arg.contains(' ') {
        return None;
    }
    Some(vec![tuple(None, arg, None, None)])
}

/// `p.parse_args()` of the script's parser, then its window bounds.
fn parse(args: &[String]) -> Result<Args, Stop> {
    let mut tuples = Vec::new();
    let mut pattern = Vec::new();
    let mut after_dashes = false;
    for arg in args {
        let found = if after_dashes || arg == "--" {
            None
        } else {
            parse_optional(arg)
        };
        pattern.push(match (&found, after_dashes || arg != "--") {
            (Some(_), _) => b'O',
            (None, true) => b'A',
            (None, false) => b'-',
        });
        after_dashes |= arg == "--";
        tuples.push(found);
    }
    let mut values: [Option<f64>; 2] = [Some(60.0), Some(10.0)];
    let mut texts: [Option<String>; 2] = [None, None];
    let mut seen = [false; 5];
    let mut extras: Vec<String> = Vec::new();
    // take_action: the help action ends the parse; a store converts its one string.
    let mut take = |action: usize, strings: &[String], seen: &mut [bool; 5]| -> Result<(), Stop> {
        seen[action] = true;
        match action {
            0 => Err(Stop::Help),
            1 | 2 => {
                let text = &strings[0];
                let value = python::float(text).ok_or_else(|| {
                    argument_error(
                        action,
                        &format!("invalid float value: {}", python::repr(text)),
                    )
                })?;
                values[action - 1] = Some(value);
                Ok(())
            }
            _ => {
                texts[action - 3] = Some(strings[0].clone());
                Ok(())
            }
        }
    };
    let max_option = tuples.iter().rposition(Option::is_some);
    let mut start = 0;
    while let Some(max) = max_option.filter(|&max| start <= max) {
        let mut next = start;
        while next <= max && tuples[next].is_none() {
            next += 1;
        }
        if tuples[start].is_none() {
            extras.extend(args[start..next].iter().cloned());
            start = next;
        }
        // consume_optional
        let found = tuples[start].clone().expect("option");
        if found.len() > 1 {
            let matches: Vec<&str> = found.iter().map(|t| t.option.as_str()).collect();
            return Err(Stop::Error(format!(
                "ambiguous option: {} could match {}",
                args[start],
                matches.join(", ")
            )));
        }
        let Tuple {
            mut action,
            mut option,
            mut sep,
            mut explicit,
        } = found.into_iter().next().expect("tuple");
        let mut actions: Vec<(usize, Vec<String>)> = Vec::new();
        let stop;
        loop {
            let Some(a) = action else {
                extras.push(args[start].clone());
                stop = start + 1;
                break;
            };
            // nargs 0 for the help action, one argument for the others.
            let wants = usize::from(a != 0);
            if let Some(argument) = explicit.clone() {
                if wants == 0 && !option.starts_with("--") && !argument.is_empty() {
                    if sep.as_deref().is_some_and(|s| !s.is_empty()) || argument.starts_with('-') {
                        return Err(argument_error(
                            a,
                            &format!("ignored explicit argument {}", python::repr(&argument)),
                        ));
                    }
                    actions.push((a, Vec::new()));
                    let mut rest = argument.chars();
                    let first = rest.next().expect("argument");
                    let rest = rest.as_str();
                    option = format!("-{first}");
                    if let Some(next) = lookup(&option) {
                        action = Some(next);
                        if rest.is_empty() {
                            (sep, explicit) = (None, None);
                        } else if let Some(value) = rest.strip_prefix('=') {
                            (sep, explicit) = (Some("=".into()), Some(value.into()));
                        } else {
                            (sep, explicit) = (Some(String::new()), Some(rest.into()));
                        }
                    } else {
                        extras.push(format!("-{argument}"));
                        stop = start + 1;
                        break;
                    }
                } else if wants == 1 {
                    stop = start + 1;
                    actions.push((a, vec![argument]));
                    break;
                } else {
                    return Err(argument_error(
                        a,
                        &format!("ignored explicit argument {}", python::repr(&argument)),
                    ));
                }
            } else {
                let from = start + 1;
                if wants == 1 && pattern.get(from) != Some(&b'A') {
                    return Err(argument_error(a, "expected one argument"));
                }
                stop = from + wants;
                actions.push((a, args[from..stop].to_vec()));
                break;
            }
        }
        for (a, strings) in actions {
            take(a, &strings, &mut seen)?;
        }
        start = stop;
    }
    extras.extend(args[start..].iter().cloned());
    let required: Vec<&str> = [3, 4]
        .into_iter()
        .filter(|&a| !seen[a])
        .map(|a| NAMES[a])
        .collect();
    if !required.is_empty() {
        return Err(Stop::Error(format!(
            "the following arguments are required: {}",
            required.join(", ")
        )));
    }
    if !extras.is_empty() {
        return Err(Stop::Error(format!(
            "unrecognized arguments: {}",
            extras.join(" ")
        )));
    }
    let [Some(seconds), Some(settle)] = values else {
        unreachable!("defaults")
    };
    // `not 1 <= seconds <= 7200 or not 0 <= settle <= 120` (NaN fails both).
    if !((1.0..=7200.0).contains(&seconds) && (0.0..=120.0).contains(&settle)) {
        return Err(Stop::Error("bounded windows only".into()));
    }
    let [Some(state), Some(output)] = texts else {
        unreachable!("required")
    };
    Ok(Args {
        seconds,
        settle,
        state,
        output,
    })
}

pub fn main(root: &Root, args: &[String]) -> Outcome {
    let parsed = match parse(args) {
        Ok(parsed) => parsed,
        Err(Stop::Help) => {
            let _ = std::io::stdout().write_all(HELP.as_bytes());
            super::exit(0)
        }
        Err(Stop::Error(message)) => {
            // stderr's backslashreplace: an argument that was not UTF-8 shows as \udcXX.
            let message = y2p_core::os_text::backslashreplace(&message);
            let _ = writeln!(std::io::stderr(), "{USAGE}y2-power-audit: error: {message}");
            super::exit(2)
        }
    };
    match run(root, &parsed) {
        Ok(()) => super::exit(0),
        Err(error) => traceback(&error, 1),
    }
}

/// `Path(path).read_text().strip()`: strict UTF-8, any failure is None.
fn read(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|t| python_strip(&t).to_owned())
}

/// `glob.glob(pattern)` below the root: hidden names only match a pattern that starts with a dot;
/// the listing order, not sorted. Device paths (without the root).
fn glob(root: &Root, pattern: &str) -> Vec<String> {
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
type Files = Vec<(String, Option<String>)>;

fn files(root: &Root, patterns: &[&str]) -> Files {
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

fn get<'a>(files: &'a Files, key: &str) -> Option<&'a Option<String>> {
    files.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// One thread at one end of the window (`/proc/<pid>/task/<tid>/stat`, `status`, `wchan`).
#[derive(Serialize)]
struct Thread {
    name: String,
    state: String,
    utime: i64,
    stime: i64,
    #[serde(rename = "context_switches", serialize_with = "as_map")]
    switches: Vec<(String, i64)>,
    wchan: Option<String>,
}

/// Threads by `pid:tid:starttime`.
type Threads = Vec<(String, Thread)>;

fn int(text: &str) -> Result<i64, E> {
    python_int(text).ok_or_else(|| {
        E::value(format!(
            "invalid literal for int() with base 10: {}",
            y2p_core::py::repr(text)
        ))
    })
}

fn processes(root: &Root) -> Result<Threads, E> {
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
enum Application {
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

fn application(root: &Root) -> Application {
    let status = match receipt::application_rpc(root, "status", false) {
        Ok(status) => status,
        Err(error) => {
            return Application::Unavailable {
                unavailable: error.class(),
            }
        }
    };
    let empty = Value::Object(serde_json::Map::new());
    let section = |key: &str| status.get(key).unwrap_or(&empty);
    // A section that is not an object has no fields: the whole status is unavailable.
    let field = |key: &str, inner: &str| -> Option<Value> {
        let Value::Object(map) = section(key) else {
            return None;
        };
        Some(plain(map.get(inner).cloned().unwrap_or(Value::Null)))
    };
    let graphics = ["backend", "frames", "context_losses"]
        .into_iter()
        .map(|k| field("graphics", k).map(|v| (k, v)))
        .collect::<Option<Vec<_>>>();
    match (
        field("playback", "state"),
        field("library", "scanning"),
        field("audio", "state"),
        graphics,
    ) {
        (Some(playback_state), Some(scanning), Some(audio_state), Some(graphics)) => {
            Application::Status {
                screen_off: plain(status.get("screen_off").cloned().unwrap_or(Value::Null)),
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

fn release() -> String {
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
struct Snapshot {
    wall_time: f64,
    monotonic: f64,
    boot_id: Option<String>,
    identity: Option<String>,
    kernel: String,
    application: Application,
    #[serde(serialize_with = "as_map")]
    files: Files,
    #[serde(serialize_with = "as_map")]
    threads: Threads,
    collection_seconds: f64,
}

fn snapshot(root: &Root) -> Result<Snapshot, E> {
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
struct Sample {
    boot_id: Option<String>,
    #[serde(serialize_with = "as_map")]
    threads: Threads,
    #[serde(serialize_with = "as_map")]
    files: Files,
    monotonic: f64,
    #[serde(rename = "collection_seconds")]
    collection: f64,
    #[serde(rename = "cpu_seconds")]
    cpu: f64,
}

fn counters(root: &Root) -> Result<Sample, E> {
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

fn file<'a>(sample: &'a Sample, key: &str) -> &'a str {
    get(&sample.files, key)
        .and_then(|v| v.as_deref())
        .unwrap_or_default()
}

type Irqs = Vec<(String, (Vec<(String, i64)>, String))>;

fn interrupts(sample: &Sample) -> Result<Irqs, E> {
    let text = file(sample, "/proc/interrupts");
    let lines = splitlines(text);
    let cpus: Vec<&str> = lines.first().map(|l| split(l)).unwrap_or_default();
    let mut rows: Irqs = Vec::new();
    for line in lines.iter().skip(1) {
        let Some((key, tail)) = line.split_once(':') else {
            continue;
        };
        let mut parts = split(tail);
        let mut counts = Vec::new();
        while parts
            .first()
            .is_some_and(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        {
            counts.push(int(parts.remove(0))?);
        }
        let columns = if counts.len() == cpus.len() {
            cpus.iter().map(|c| (*c).to_owned()).zip(counts).collect()
        } else {
            Vec::new()
        };
        let key = python_strip(key).to_owned();
        let value = (columns, parts.join(" "));
        match rows.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = value,
            None => rows.push((key, value)),
        }
    }
    Ok(rows)
}

/// A row's CPU column names as a set.
fn column_keys(list: &[(String, i64)]) -> Vec<&String> {
    let mut keys: Vec<&String> = list.iter().map(|(c, _)| c).collect();
    keys.sort();
    keys.dedup();
    keys
}

/// One thread's CPU and scheduling over the window.
#[derive(Clone, Serialize)]
struct Offender {
    thread: String,
    name: String,
    ticks: i64,
    cpu_percent_one_core: f64,
    context_switches_per_second: f64,
}

/// One IRQ line's count over the window. Offline CPU columns disappear, even though their earlier
/// IRQ totals remain relevant: a column change or a reset makes the delta unknown, and the
/// common-CPU evidence is kept apart.
#[derive(Serialize)]
struct Irq {
    #[serde(skip)]
    rank: i64,
    irq: String,
    name: String,
    delta: Option<i64>,
    per_second: Option<f64>,
    counter_discontinuity: bool,
    cpu_columns_changed: bool,
    #[serde(serialize_with = "as_map")]
    common_cpu_deltas: Vec<(String, i64)>,
    common_cpu_lower_bound_per_second: Option<f64>,
}

/// One CPU idle state's residency over the window.
#[derive(Serialize)]
struct Idle {
    path: String,
    name: Option<String>,
    #[serde(rename = "raw_time_delta_us")]
    raw: i64,
    time_us: Option<i64>,
    #[serde(rename = "percent_wall_time")]
    percent: Option<f64>,
    entries: Option<i64>,
    #[serde(rename = "counter_discontinuity")]
    discontinuity: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    residency_invalid_reason: Option<&'static str>,
}

/// One OPP's share of the recorded residency (keys sorted, as the record has always had them).
#[derive(Serialize)]
struct Frequency {
    khz: i64,
    percent_recorded_residency: Option<f64>,
    ticks: i64,
}

#[derive(Serialize)]
struct Rate {
    delta: i64,
    per_second: f64,
}

#[derive(Serialize)]
struct Wakeup {
    name: String,
    #[serde(serialize_with = "as_map")]
    deltas: Vec<(String, i64)>,
}

/// What the window measured.
#[derive(Serialize)]
struct Summary {
    top_cpu_threads: Vec<Offender>,
    top_context_switch_threads: Vec<Offender>,
    irqs: Vec<Irq>,
    cpuidle: Vec<Idle>,
    cpu0_idle_exits_per_second: Option<f64>,
    frequency_residency: Vec<Frequency>,
    frequency_reason: Option<&'static str>,
    #[serde(serialize_with = "as_map")]
    scheduler: Vec<(String, Rate)>,
    wakeup_sources: Vec<Wakeup>,
    wakeups_per_second: Option<f64>,
    wakeups_reason: &'static str,
}

fn delta(a: &Sample, b: &Sample, duration: f64) -> Result<Summary, E> {
    if duration <= 0.0 || a.boot_id != b.boot_id {
        return Err(E::value("counter_window_crosses_boot_or_has_no_duration"));
    }
    // SAFETY: sysconf has no preconditions.
    let clock = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
    let mut offenders = Vec::new();
    for (key, old) in &a.threads {
        let Some(new) = b.threads.iter().find(|(k, _)| k == key).map(|(_, t)| t) else {
            continue;
        };
        let ticks = new.utime + new.stime - old.utime - old.stime;
        let switches = |t: &Thread| t.switches.iter().map(|(_, v)| v).sum::<i64>();
        offenders.push(Offender {
            thread: key.clone(),
            name: new.name.clone(),
            ticks,
            cpu_percent_one_core: (100 * ticks) as f64 / clock / duration,
            context_switches_per_second: (switches(new) - switches(old)) as f64 / duration,
        });
    }
    let (ia, ib) = (interrupts(a)?, interrupts(b)?);
    let mut irqs: Vec<Irq> = Vec::new();
    for (key, (counts, name)) in &ib {
        let old: &[(String, i64)] = ia
            .iter()
            .find(|(k, _)| k == key)
            .map_or(&[], |(_, (c, _))| c.as_slice());
        let common: Vec<(String, i64)> = counts
            .iter()
            .filter_map(|(cpu, n)| {
                old.iter()
                    .find(|(c, _)| c == cpu)
                    .map(|(_, o)| (cpu.clone(), n - o))
            })
            .collect();
        let columns_changed = column_keys(old) != column_keys(counts);
        let reset = common.iter().any(|(_, v)| *v < 0);
        let valid = !common.is_empty() && !columns_changed && !reset;
        let sum: i64 = common.iter().map(|(_, v)| v).sum();
        let delta = valid.then_some(sum);
        // Subtracting column sums can silently undercount even when the apparent delta is
        // positive: the lower bound counts only the CPUs present at both ends.
        let lower = (!common.is_empty() && !reset).then_some(sum);
        irqs.push(Irq {
            rank: delta.unwrap_or(0),
            irq: key.clone(),
            name: name.clone(),
            delta,
            per_second: delta.map(|v| v as f64 / duration),
            counter_discontinuity: !valid,
            cpu_columns_changed: columns_changed,
            common_cpu_deltas: common,
            common_cpu_lower_bound_per_second: lower.map(|v| v as f64 / duration),
        });
    }
    let mut idle: Vec<Idle> = Vec::new();
    for (path, value) in &a.files {
        if !(path.contains("/cpuidle/") && path.ends_with("/time")) {
            continue;
        }
        let (Some(start), Some(Some(end))) = (value, get(&b.files, path)) else {
            continue;
        };
        let parent = path.rsplit_once('/').map_or("", |(p, _)| p).to_owned();
        let usage = format!("{parent}/usage");
        let time_us = int(end)? - int(start)?;
        let usage_of = |s: &Sample| -> Result<i64, E> {
            match get(&s.files, &usage) {
                Some(Some(text)) => int(text),
                Some(None) => Err(E::TypeError("int() argument must be a string".into())),
                None => Err(E::Crash(format!(
                    "KeyError: {}",
                    y2p_core::py::repr(&usage)
                ))),
            }
        };
        let entries = usage_of(b)? - usage_of(a)?;
        let discontinuity = time_us < 0 || entries < 0;
        idle.push(Idle {
            name: get(&a.files, &format!("{parent}/name")).cloned().flatten(),
            path: parent,
            raw: time_us,
            time_us: (!discontinuity).then_some(time_us),
            percent: (!discontinuity).then(|| time_us as f64 / duration / 10000.0),
            entries: (!discontinuity).then_some(entries),
            discontinuity,
            residency_invalid_reason: None,
        });
    }
    // An idle interval is charged when that CPU exits it. An untouched secondary CPU can therefore
    // charge time that began before the first endpoint read. Reject impossible per-CPU totals
    // instead of publishing/clamping >100%.
    let cpu_of = |row: &Idle| {
        row.path
            .split("/cpuidle/")
            .next()
            .unwrap_or_default()
            .to_owned()
    };
    let mut cpus: Vec<String> = Vec::new();
    for row in &idle {
        let cpu = cpu_of(row);
        if !cpus.contains(&cpu) {
            cpus.push(cpu);
        }
    }
    for cpu in cpus {
        let rows: Vec<usize> = (0..idle.len())
            .filter(|i| idle[*i].path.split("/cpuidle/").next() == Some(cpu.as_str()))
            .collect();
        if rows.iter().all(|i| idle[*i].time_us.is_some())
            && rows
                .iter()
                .map(|i| idle[*i].time_us.unwrap_or(0))
                .sum::<i64>() as f64
                > duration * 1e6
        {
            for i in rows {
                let row = &mut idle[i];
                row.time_us = None;
                row.percent = None;
                row.residency_invalid_reason = Some("idle_accounting_crosses_counter_window");
            }
        }
    }
    let opps = |s: &Sample| -> Result<Vec<(i64, i64)>, E> {
        let mut map: Vec<(i64, i64)> = Vec::new();
        for line in splitlines(file(
            s,
            "/sys/devices/system/cpu/cpufreq/policy0/stats/time_in_state",
        )) {
            let parts = split(line);
            let [k, v] = parts.as_slice() else {
                return Err(E::value("not enough values to unpack"));
            };
            let (k, v) = (int(k)?, int(v)?);
            match map.iter_mut().find(|(key, _)| *key == k) {
                Some(entry) => entry.1 = v,
                None => map.push((k, v)),
            }
        }
        Ok(map)
    };
    let (fa, fb) = (opps(a)?, opps(b)?);
    let mut fd: BTreeMap<i64, i64> = BTreeMap::new();
    for (key, value) in &fb {
        if let Some((_, old)) = fa.iter().find(|(k, _)| k == key) {
            if value >= old {
                fd.insert(*key, value - old);
            }
        }
    }
    let total: i64 = fd.values().sum();
    let frequency: Vec<Frequency> = fd
        .iter()
        .map(|(&khz, &ticks)| Frequency {
            khz,
            percent_recorded_residency: (total != 0).then(|| (100 * ticks) as f64 / total as f64),
            ticks,
        })
        .collect();
    let proc_stat = |s: &Sample| -> Result<Vec<(String, i64)>, E> {
        let mut map: Vec<(String, i64)> = Vec::new();
        for line in splitlines(file(s, "/proc/stat")) {
            let parts = split(line);
            if let Some(&first) = parts.first() {
                if ["ctxt", "processes", "intr"].contains(&first) {
                    let value = int(parts.get(1).ok_or_else(|| E::Crash("IndexError".into()))?)?;
                    match map.iter_mut().find(|(k, _)| k == first) {
                        Some(entry) => entry.1 = value,
                        None => map.push((first.to_owned(), value)),
                    }
                }
            }
        }
        Ok(map)
    };
    let (sa, sb) = (proc_stat(a)?, proc_stat(b)?);
    let scheduler: Vec<(String, Rate)> = sa
        .iter()
        .filter_map(|(key, old)| {
            let new = sb.iter().find(|(k, _)| k == key)?.1;
            (new >= *old).then(|| {
                let rate = Rate {
                    delta: new - old,
                    per_second: (new - old) as f64 / duration,
                };
                (key.clone(), rate)
            })
        })
        .collect();
    let sources = |s: &Sample| -> Vec<(String, Vec<(String, String)>)> {
        let rows = splitlines(file(s, "/sys/kernel/debug/wakeup_sources"));
        let Some(first) = rows.first() else {
            return Vec::new();
        };
        let header = split(first);
        let mut result: Vec<(String, Vec<(String, String)>)> = Vec::new();
        for row in &rows[1..] {
            let parts = split(row);
            if let Some(&name) = parts.first() {
                let fields = header
                    .iter()
                    .skip(1)
                    .zip(parts.iter().skip(1))
                    .map(|(h, p)| ((*h).to_owned(), (*p).to_owned()))
                    .collect();
                match result.iter_mut().find(|(k, _)| k == name) {
                    Some(entry) => entry.1 = fields,
                    None => result.push((name.to_owned(), fields)),
                }
            }
        }
        result
    };
    let (wa, wb) = (sources(a), sources(b));
    let mut wakes = Vec::new();
    for (name, old) in &wa {
        let Some((_, new)) = wb.iter().find(|(k, _)| k == name) else {
            continue;
        };
        let mut deltas = Vec::new();
        for key in ["event_count", "wakeup_count", "active_count"] {
            let lookup = |list: &[(String, String)]| {
                list.iter()
                    .rev()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.clone())
            };
            if let (Some(o), Some(n)) = (lookup(old), lookup(new)) {
                deltas.push((key.to_owned(), int(&n)? - int(&o)?));
            }
        }
        wakes.push(Wakeup {
            name: name.clone(),
            deltas,
        });
    }
    let cpu0: Vec<&Idle> = idle.iter().filter(|r| r.path.contains("/cpu0/")).collect();
    let exits = (!cpu0.is_empty() && cpu0.iter().all(|r| r.entries.is_some()))
        .then(|| cpu0.iter().map(|r| r.entries.unwrap_or(0)).sum::<i64>());
    let mut by_ticks: Vec<&Offender> = offenders.iter().collect();
    by_ticks.sort_by_key(|x| std::cmp::Reverse(x.ticks));
    let mut by_rate: Vec<&Offender> = offenders.iter().collect();
    by_rate.sort_by(|x, y| {
        y.context_switches_per_second
            .partial_cmp(&x.context_switches_per_second)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    irqs.sort_by_key(|x| std::cmp::Reverse(x.rank));
    let frequency_reason = frequency
        .is_empty()
        .then_some("Kernel does not expose OPP residency counters.");
    Ok(Summary {
        top_cpu_threads: by_ticks.into_iter().take(25).cloned().collect(),
        top_context_switch_threads: by_rate.into_iter().take(25).cloned().collect(),
        irqs,
        cpuidle: idle,
        cpu0_idle_exits_per_second: exits.map(|e| e as f64 / duration),
        frequency_residency: frequency,
        frequency_reason,
        scheduler,
        wakeup_sources: wakes,
        wakeups_per_second: None,
        wakeups_reason:
            "Context switches and IRQs are proxies; they are not independent unique CPU wakes.",
    })
}

/// The audit's record (`--output`, compact JSON in this order).
#[derive(Serialize)]
struct Audit<'a> {
    schema: &'static str,
    state: &'a str,
    duration_seconds: f64,
    undisturbed_sleep_seconds: f64,
    same_boot: bool,
    start: Sample,
    end: Sample,
    state_start: Snapshot,
    state_end: Snapshot,
    summary: Summary,
    observer: Observer,
}

/// What the observer itself cost.
#[derive(Serialize)]
struct Observer {
    pid: u32,
    total_cpu_seconds: f64,
    counter_cpu_seconds: f64,
    counter_collection_seconds: f64,
    max_rss_kib: i64,
    note: &'static str,
}

fn sleep(seconds: f64) {
    std::thread::sleep(std::time::Duration::from_secs_f64(seconds));
}

fn run(root: &Root, args: &Args) -> Result<(), E> {
    let cpu_start = process_time();
    let state_start = snapshot(root)?;
    sleep(args.settle);
    let a = counters(root)?;
    let measured_start = monotonic();
    sleep(args.seconds);
    let measured_end = monotonic();
    let b = counters(root)?;
    // Heavy device/clock/process descriptions run outside the counter interval.
    let state_end = snapshot(root)?;
    let duration = b.monotonic - a.monotonic;
    let summary = delta(&a, &b, duration)?;
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: getrusage into a zeroed rusage.
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    let observer = Observer {
        pid: std::process::id(),
        total_cpu_seconds: process_time() - cpu_start,
        counter_cpu_seconds: a.cpu + b.cpu,
        counter_collection_seconds: a.collection + b.collection,
        max_rss_kib: y2p_core::wide(usage.ru_maxrss),
        note: "Endpoint counter skew is bounded by collection time; descriptions are outside the window.",
    };
    let result = Audit {
        schema: "org.y2linux.power-audit/v2",
        state: &args.state,
        duration_seconds: duration,
        undisturbed_sleep_seconds: measured_end - measured_start,
        same_boot: a.boot_id == b.boot_id,
        start: a,
        end: b,
        state_start,
        state_end,
        summary,
        observer,
    };
    // os.fsencode: an --output given as bytes that are not UTF-8 names those bytes.
    let encoded = y2p_core::os_text::fsencode(&args.output);
    let path = Path::new(&encoded);
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| E::os(&e, Some(path)))?;
    let text = dumps(&result, Separators::Compact).map_err(|e| E::value(e.to_string()))? + "\n";
    out.write_all(text.as_bytes())
        .map_err(|e| E::os(&e, None))?;
    Ok(())
}

#[cfg(test)]
#[path = "audit_tests.rs"]
mod audit_tests;
