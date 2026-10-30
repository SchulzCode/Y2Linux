//! `y2-power-audit`: endpoint-only observation, no activity during
//! the measured sleep window. Counters are read at both ends of the window, heavy descriptions
//! outside it; the result goes to `--output` (created exclusively, compact JSON in insertion order).
//!
//! The arguments (`args`) are the script's argparse parser, then its `bounded windows only` refusal
//! (exit 2); `sample` reads each end, `delta` measures the window.
// SPDX-License-Identifier: GPL-2.0-only
mod args;
mod delta;
mod sample;

use super::monotonic;
use super::{fail, process_time, E};
use crate::reply::Outcome;
use args::{parse, Args, Stop, HELP, USAGE};
use delta::Summary;
use sample::{counters, snapshot, Sample, Snapshot};
use serde::Serialize;
use std::{fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt, path::Path};
use y2p_core::{
    json::{dumps, Separators},
    Root,
};

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
        Err(error) => fail("y2-power-audit", &error),
    }
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
    let summary = delta::delta(&a, &b, duration)?;
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
    out.write_all(text.as_bytes())?;
    Ok(())
}

#[cfg(test)]
#[path = "audit_tests.rs"]
mod audit_tests;
