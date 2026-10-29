//! Group `observation` of the port (docs/planning/PLATFORM-RUST-PORT-PLAN.md, "Remaining port groups"):
//! `status`, `health`, `boot-evidence`, `network-check`, `collect`, `bench-storage`, `bench-library`
//! (observe.py, health.py, boot.py, boot_retained.py, network_check.py, workload.py, collect.py,
//! bench.py). The dispatcher's argparse has parsed the arguments: a route sees their canonical form
//! and takes every one of them (cli.py's own `parser.error` for `status` is printed here).
// SPDX-License-Identifier: GPL-2.0-only
mod bench;
mod bluetooth;
mod collect;
mod evidence;
pub(crate) mod health;
mod network_check;
pub(crate) mod snapshot;
mod system;
mod util;

use super::Command;
use crate::reply::{Outcome, Reply};
use serde_json::Value;
use std::collections::BTreeMap;
use y2p_core::{command, Root};

const SECTIONS: [&str; 11] = [
    "cpu",
    "memory",
    "thermal",
    "power",
    "storage",
    "wifi",
    "bluetooth",
    "system",
    "audio",
    "readiness",
    "usb",
];

/// One subparser's options: flags (store_true), single values (last wins), appended values and an
/// optional positional with its choices.
#[derive(Default)]
struct Spec<'a> {
    flags: &'a [&'a str],
    values: &'a [&'a str],
    appends: &'a [&'a str],
    positional: &'a [&'a str],
}

#[derive(Default, Debug)]
struct Parsed {
    flags: Vec<String>,
    values: BTreeMap<String, String>,
    appends: BTreeMap<String, Vec<String>>,
    positional: Option<String>,
}

impl Parsed {
    fn flag(&self, name: &str) -> bool {
        self.flags.iter().any(|f| f == name)
    }
    fn value(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }
    fn appended(&self, name: &str) -> &[String] {
        self.appends.get(name).map_or(&[], Vec::as_slice)
    }
}

/// The canonical arguments after the subcommand (argparse.rs: positionals, then options in definition
/// order, `--option value` as two tokens), or None for a form this group does not run.
fn parse(args: &[&str], spec: &Spec) -> Option<Parsed> {
    let mut parsed = Parsed::default();
    let mut index = 0;
    while index < args.len() {
        let token = args[index];
        index += 1;
        if let Some(option) = token.strip_prefix("--").filter(|_| token.len() > 2) {
            let (name, inline) = match option.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (option, None),
            };
            let name = format!("--{name}");
            if spec.flags.contains(&name.as_str()) {
                if inline.is_some() {
                    return None;
                }
                parsed.flags.push(name);
                continue;
            }
            let takes = spec.values.contains(&name.as_str());
            let appends = spec.appends.contains(&name.as_str());
            if !takes && !appends {
                return None;
            }
            let value = match inline {
                Some(value) => value,
                None => {
                    let value = *args.get(index)?;
                    index += 1;
                    value
                }
            };
            if takes {
                parsed.values.insert(name, value.to_owned());
            } else {
                parsed
                    .appends
                    .entry(name)
                    .or_default()
                    .push(value.to_owned());
            }
            continue;
        }
        if token.starts_with('-')
            || parsed.positional.is_some()
            || !spec.positional.contains(&token)
        {
            return None;
        }
        parsed.positional = Some(token.to_owned());
    }
    Some(parsed)
}

/// `type=int` in its canonical form (`str(int(x))`). Python's int has no bounds: one beyond 64 bits
/// is clamped, which every caller refuses as out of range exactly as it refuses the real value
/// (`collection_bounds_exceeded`, `invalid pid`, …).
fn integer(text: &str) -> Option<i64> {
    if !crate::regex!(r"\A-?[0-9]+\z").is_match(text) {
        return None;
    }
    Some(text.parse().unwrap_or(if text.starts_with('-') {
        i64::MIN
    } else {
        i64::MAX
    }))
}

/// `type=float` in its canonical form (`repr(float(x))`: `0.5`, `1e-05`, `inf`, `nan`).
fn decimal(text: &str) -> Option<f64> {
    text.parse().ok()
}

/// cli.py's `parser.error(message)` after parsing: main.py's usage and the message on stderr, exit 2.
fn parser_error(message: &'static str) -> Command {
    Box::new(move |_| {
        use std::io::Write;
        let _ = std::io::stderr().write_all(crate::argparse::error_text(message).as_bytes());
        std::process::exit(2)
    })
}

fn integers(values: &[String]) -> Option<Vec<i64>> {
    values.iter().map(|v| integer(v)).collect()
}

/// The Rust implementation of these exact arguments, if this group has one.
pub fn route(args: &[&str]) -> Option<Command> {
    let (name, rest) = args.split_first()?;
    match *name {
        "status" => {
            let parsed = parse(
                rest,
                &Spec {
                    flags: &["--json", "--pss", "--reborn"],
                    values: &["--interval"],
                    appends: &["--pid"],
                    positional: &SECTIONS,
                },
            )?;
            let interval = match parsed.value("--interval") {
                Some(v) => decimal(v)?,
                None => 0.0,
            };
            let pids = integers(parsed.appended("--pid"))?;
            // `not 0 <= interval <= 10 or 0 < interval < 0.1` (NaN too) `or len(pid) > 32`.
            if !(0.0..=10.0).contains(&interval)
                || (interval > 0.0 && interval < 0.1)
                || pids.len() > 32
            {
                return Some(parser_error(
                    "interval must be zero or 0.1–10 seconds; at most 32 PIDs",
                ));
            }
            let options = StatusOptions {
                section: parsed.positional.clone(),
                pids,
                pss: parsed.flag("--pss"),
                reborn: parsed.flag("--reborn"),
                interval,
            };
            Some(Box::new(move |root| crashing(status(root, &options))))
        }
        "health" => {
            let parsed = parse(
                rest,
                &Spec {
                    flags: &["--json", "--full"],
                    ..Spec::default()
                },
            )?;
            let full = parsed.flag("--full");
            Some(Box::new(move |root| crashing(health::run(root, full))))
        }
        "boot-evidence" if rest.is_empty() => Some(Box::new(evidence::run)),
        "network-check" => {
            let parsed = parse(
                rest,
                &Spec {
                    flags: &["--throughput"],
                    values: &["--peer", "--seconds", "--interface"],
                    ..Spec::default()
                },
            )?;
            let peer = parsed.value("--peer")?.to_owned();
            let seconds = match parsed.value("--seconds") {
                Some(v) => integer(v)?,
                None => 10,
            };
            let interface = parsed.value("--interface").unwrap_or("wlan0").to_owned();
            if !["wlan0", "usb0"].contains(&interface.as_str()) {
                return None;
            }
            let throughput = parsed.flag("--throughput");
            Some(Box::new(move |root| {
                network_check::run(root, &peer, seconds, throughput, &interface)
            }))
        }
        "collect" => {
            let parsed = parse(
                rest,
                &Spec {
                    flags: &["--pss", "--reborn"],
                    values: &["--seconds", "--interval", "--warmup", "--workload"],
                    appends: &["--pid"],
                    ..Spec::default()
                },
            )?;
            let number = |name: &str, default: i64| match parsed.value(name) {
                Some(v) => integer(v),
                None => Some(default),
            };
            let options = collect::Options {
                seconds: number("--seconds", 60)?,
                interval: number("--interval", 5)?,
                warmup: number("--warmup", 0)?,
                workload: parsed.value("--workload").unwrap_or("idle").to_owned(),
                pids: integers(parsed.appended("--pid"))?,
                pss: parsed.flag("--pss"),
                reborn: parsed.flag("--reborn"),
            };
            Some(Box::new(move |root| crashing(collect::run(root, &options))))
        }
        "bench-storage" => {
            let parsed = parse(
                rest,
                &Spec {
                    values: &["--volume", "--size-mib", "--operations", "--seconds"],
                    ..Spec::default()
                },
            )?;
            let volume = volume(parsed.value("--volume"))?;
            let number = |name: &str, default: i64| match parsed.value(name) {
                Some(v) => integer(v),
                None => Some(default),
            };
            let (size, operations, seconds) = (
                number("--size-mib", 16)?,
                number("--operations", 64)?,
                number("--seconds", 300)?,
            );
            Some(Box::new(move |root| {
                bench::storage_benchmark(root, &volume, size, operations, seconds)
            }))
        }
        "bench-library" => {
            let parsed = parse(
                rest,
                &Spec {
                    flags: &["--scan"],
                    values: &["--volume", "--tracks"],
                    ..Spec::default()
                },
            )?;
            let volume = volume(parsed.value("--volume"))?;
            let tracks = match parsed.value("--tracks") {
                Some(v) => integer(v).filter(|t| [1000, 10000, 20000].contains(t))?,
                None => 1000,
            };
            let scan = parsed.flag("--scan");
            Some(Box::new(move |root| {
                bench::library_benchmark(root, &volume, tracks, scan)
            }))
        }
        _ => None,
    }
}

/// End the command as Python's uncaught exception `error` does (a traceback on stderr, nothing more
/// on stdout, exit 1) from where it is raised, after what the command already did (bench scratch
/// files, streamed `collect` lines).
pub fn crash(error: &crate::reply::Failure) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().write_all(error.traceback().as_bytes());
    std::process::exit(1)
}

/// `outcome`: a helper of another group already ends as Python's crash (`Failure::Crash`) where
/// Python would, after the snapshot asked for its subprocesses.
pub fn crashing<T>(outcome: Result<T, crate::reply::Failure>) -> Result<T, crate::reply::Failure> {
    outcome
}

fn volume(value: Option<&str>) -> Option<String> {
    let volume = value.unwrap_or("/data");
    ["/data", "/media/sd", "/media/internal"]
        .contains(&volume)
        .then(|| volume.to_owned())
}

struct StatusOptions {
    section: Option<String>,
    pids: Vec<i64>,
    pss: bool,
    reborn: bool,
    interval: f64,
}

/// `power.process_identity(ctx, name, expected)`: the pid in /run/reborn/<name>.pid when its
/// executable is `expected`, with its start time.
pub fn process_identity(root: &Root, name: &str, expected: &str) -> Option<(i64, i64)> {
    let pid = root
        .integer(&format!("/run/reborn/{name}.pid"))
        .filter(|p| *p > 1)?;
    let exe = util::realpath(&root.path(&format!("/proc/{pid}/exe")));
    if exe != util::realpath(&root.path(expected)) {
        return None;
    }
    let stat = root.read(&format!("/proc/{pid}/stat"))?;
    let (_, after) = stat.rsplit_once(") ")?;
    let start = util::int(util::split_ws(after).get(19)?)?;
    Some((pid, start))
}

/// `collect.metrics`: Reborn's bounded metrics, or None.
pub fn reborn_metrics(root: &Root) -> Value {
    let answer = command::run(root, &["/usr/bin/rebornctl", "metrics", "--json"], 1.0);
    match answer
        .output
        .filter(|_| answer.ok)
        .and_then(|o| serde_json::from_str::<Value>(&o).ok())
    {
        Some(value @ Value::Object(_)) => value,
        _ => Value::Null,
    }
}

/// `y2-platform status [section] [--json] [--pid N]… [--pss] [--reborn] [--interval S]`.
fn status(root: &Root, options: &StatusOptions) -> Outcome {
    let mut pids = options.pids.clone();
    if options.reborn {
        if let Some((pid, _)) = process_identity(root, "process", "/usr/bin/reborn") {
            pids.push(pid);
        }
        let mut seen = Vec::new();
        pids.retain(|p| {
            let first = !seen.contains(p);
            seen.push(*p);
            first
        });
        pids.truncate(32);
    }
    let mut result = snapshot::snapshot(root, &pids, options.pss, options.interval)?;
    if options.reborn {
        result.insert("reborn_metrics".into(), reborn_metrics(root));
    }
    if let Some(section) = &options.section {
        let part = if section == "usb" {
            result["system"]["usb"].clone()
        } else {
            result[section.as_str()].clone()
        };
        let mut reduced = serde_json::Map::new();
        reduced.insert("schema".into(), result["schema"].clone());
        reduced.insert("record".into(), result["record"].clone());
        reduced.insert(section.clone(), part);
        result = reduced;
    }
    util::sortable(&Value::Object(result.clone()))?;
    Reply::sorted(&result, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argument_forms_follow_argparse() {
        let spec = Spec {
            flags: &["--pss"],
            values: &["--interval"],
            appends: &["--pid"],
            positional: &SECTIONS,
        };
        let parsed = parse(&["wifi", "--pid", "5", "--pid=-3", "--pss"], &spec).unwrap();
        assert_eq!(parsed.positional.as_deref(), Some("wifi"));
        assert_eq!(parsed.appended("--pid"), ["5", "-3"]);
        assert!(parsed.flag("--pss"));
        assert_eq!(
            parse(&["--pid", "-x"], &spec).unwrap().appended("--pid"),
            ["-x"]
        );
        assert!(parse(&["--pi", "5"], &spec).is_none());
        assert!(parse(&["wifi", "cpu"], &spec).is_none());
        assert!(parse(&["--pss=1"], &spec).is_none());
        assert!(parse(&["--"], &spec).is_none());
        assert_eq!(decimal("0.5"), Some(0.5));
        assert_eq!(decimal("1e-05"), Some(1e-5));
        assert!(decimal("nan").is_some_and(f64::is_nan));
        assert_eq!(decimal("inf"), Some(f64::INFINITY));
        assert_eq!(integer("-7"), Some(-7));
        assert_eq!(integer("99999999999999999999"), Some(i64::MAX));
        assert_eq!(integer("-99999999999999999999"), Some(i64::MIN));
        // cli.py's parser.error is a route of its own (exit 2).
        assert!(route(&["status", "--interval", "0.05"]).is_some());
        assert!(route(&["status", "--interval", "11"]).is_some());
        assert!(route(&["status", "--reborn", "--pss"]).is_some());
        assert!(route(&["bench-library", "--tracks", "500"]).is_none());
        assert!(route(&["boot-evidence", "x"]).is_none());
    }
}
