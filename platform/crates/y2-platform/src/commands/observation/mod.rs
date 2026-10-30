//! The observation commands: `status`, `health`, `boot-evidence`, `network-check`, `collect`,
//! `bench-storage`, `bench-library`. They read the player and change nothing outside their own
//! records (boot evidence, benchmark scratch files). cli.rs has parsed the arguments: a route sees
//! their canonical form and takes every one of them; `status` refuses its out-of-range interval and
//! PID count here, as an argument error.
// SPDX-License-Identifier: GPL-2.0-only
mod bench;
mod bluetooth;
mod collect;
pub mod evidence;
pub(crate) mod health;
mod network_check;
pub(crate) mod snapshot;
mod system;
pub(crate) mod util;

use super::Command;
use crate::reply::{Outcome, Reply};
use serde_json::Value;
use y2p_core::{command, Root};

/// An integer argument in its canonical form (`-?digits`). One beyond 64 bits is clamped, which every
/// caller refuses as out of range like the real value (`collection_bounds_exceeded`, `invalid pid`,
/// …).
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

/// A decimal argument in its canonical form (`0.5`, `1e-05`, `inf`, `nan`).
fn decimal(text: &str) -> Option<f64> {
    text.parse().ok()
}

/// A refusal after parsing, as an argument error: the usage and `message` on stderr, exit 2.
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
            let parsed = crate::cli::given(name, rest)?;
            let interval = match parsed.value("--interval") {
                Some(v) => decimal(v)?,
                None => 0.0,
            };
            let pids = integers(&parsed.appended("--pid"))?;
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
            Some(Box::new(move |root| status(root, &options)))
        }
        "health" => {
            let parsed = crate::cli::given(name, rest)?;
            let full = parsed.flag("--full");
            Some(Box::new(move |root| health::run(root, full)))
        }
        "boot-evidence" if rest.is_empty() => Some(Box::new(evidence::run)),
        "network-check" => {
            let parsed = crate::cli::given(name, rest)?;
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
            let parsed = crate::cli::given(name, rest)?;
            let number = |name: &str, default: i64| match parsed.value(name) {
                Some(v) => integer(v),
                None => Some(default),
            };
            let options = collect::Options {
                seconds: number("--seconds", 60)?,
                interval: number("--interval", 5)?,
                warmup: number("--warmup", 0)?,
                workload: parsed.value("--workload").unwrap_or("idle").to_owned(),
                pids: integers(&parsed.appended("--pid"))?,
                pss: parsed.flag("--pss"),
                reborn: parsed.flag("--reborn"),
            };
            Some(Box::new(move |root| collect::run(root, &options)))
        }
        "bench-storage" => {
            let parsed = crate::cli::given(name, rest)?;
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
            let parsed = crate::cli::given(name, rest)?;
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

/// The pid in /run/reborn/<name>.pid while its executable is still `expected` (not a reused pid),
/// with its start time.
pub fn process_identity(root: &Root, name: &str, expected: &str) -> Option<(i64, i64)> {
    y2p_core::power_identity::process_identity(root, name, expected).map(|p| (p.pid, p.start_ticks))
}

/// Reborn's control socket (rebornctl's default).
pub const REBORN_CONTROL: &str = "/run/reborn/control.sock";

/// One request `{"op": op}` to Reborn's control socket with rebornctl's bounds (`timeout` seconds in
/// all, the answer at most what `command::run` keeps of rebornctl's output plus the reply's
/// envelope): the result of an `ok` reply, as `rebornctl <op> --json` prints it; None for no answer,
/// a refusal or an answer too long or not JSON.
pub fn reborn_control(path: &std::path::Path, op: &str, timeout: f64) -> Option<Value> {
    use std::io::{Read, Write};
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs_f64(timeout);
    let left = || {
        Some(deadline.saturating_duration_since(std::time::Instant::now())).filter(|d| !d.is_zero())
    };
    let mut stream = std::os::unix::net::UnixStream::connect(path).ok()?;
    stream.set_write_timeout(left()).ok()?;
    let request =
        serde_json::json!({"version": 1, "id": 1, "command": {"op": op}}).to_string() + "\n";
    stream.write_all(request.as_bytes()).ok()?;
    let mut reply = Vec::new();
    let mut block = [0u8; 4096];
    while !reply.contains(&b'\n') {
        stream.set_read_timeout(left()).ok()?;
        match stream.read(&mut block) {
            Ok(0) => break,
            Ok(n) => reply.extend_from_slice(&block[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        }
        if reply.len() > command::LIMIT + 1024 {
            return None;
        }
    }
    let mut reply: Value = serde_json::from_slice(&reply).ok()?;
    (reply.get("ok") == Some(&Value::Bool(true))).then(|| reply["result"].take())
}

/// Reborn's own bounded metrics (the object `rebornctl metrics --json` prints), or null. On the
/// player they come straight from Reborn's control socket, whose control thread answers `metrics`
/// from its counters: no rebornctl process and no wake of Reborn's UI loop. Below a fixture root the
/// same command as before is asked (the goldens record it).
pub fn reborn_metrics(root: &Root) -> Value {
    let metrics = if root.is_real() {
        reborn_control(&root.path(REBORN_CONTROL), "metrics", 1.0)
    } else {
        let answer = command::run(root, &["/usr/bin/rebornctl", "metrics", "--json"], 1.0);
        answer
            .output
            .filter(|_| answer.ok)
            .and_then(|o| serde_json::from_str::<Value>(&o).ok())
    };
    match metrics {
        Some(value @ Value::Object(_)) => value,
        _ => Value::Null,
    }
}

/// `y2-platform status [section] [--json] [--pid N]… [--pss] [--reborn] [--interval S]`.
fn status(root: &Root, options: &StatusOptions) -> Outcome {
    let mut pids = options.pids.clone();
    if options.reborn {
        // Reborn's own process too, each pid once (the first mention keeps its place).
        if let Some((pid, _)) = process_identity(root, "process", "/usr/bin/reborn") {
            pids.push(pid);
        }
        let mut unique = Vec::with_capacity(pids.len());
        for pid in pids {
            if !unique.contains(&pid) {
                unique.push(pid);
            }
        }
        pids = unique;
        pids.truncate(32);
    }
    let mut snapshot = snapshot::snapshot(root, &pids, options.pss, options.interval)?;
    if options.reborn {
        snapshot.reborn_metrics = Some(reborn_metrics(root));
    }
    let mut result = snapshot.into_map()?;
    if let Some(section) = &options.section {
        // One section (`usb` is the system section's), with the schema and the record.
        let part = if section == "usb" {
            result
                .get_mut("system")
                .and_then(|system| system.get_mut("usb"))
                .map(Value::take)
        } else {
            result.shift_remove(section)
        };
        let mut reduced = serde_json::Map::new();
        for key in ["schema", "record"] {
            reduced.insert(key.into(), result.shift_remove(key).unwrap_or(Value::Null));
        }
        reduced.insert(section.clone(), part.unwrap_or(Value::Null));
        result = reduced;
    }
    Reply::sorted(&result, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_arguments_are_read_by_the_commands_table() {
        let parsed =
            crate::cli::given("status", &["wifi", "--pss", "--pid", "5", "--pid", "-3"]).unwrap();
        assert_eq!(parsed.positional.as_deref(), Some("wifi"));
        assert_eq!(parsed.appended("--pid"), ["5", "-3"]);
        assert!(parsed.flag("--pss") && !parsed.flag("--reborn"));
        assert!(crate::cli::given("status", &["--pi", "5"]).is_none());
        assert!(crate::cli::given("status", &["wifi", "cpu"]).is_none());
        assert!(crate::cli::given("status", &["--pid"]).is_none());
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
