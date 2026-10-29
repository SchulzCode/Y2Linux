//! `bench.py`: scratch-only storage measurements, descriptor-pinned across media removal
//! (`bench-storage`, `bench-library`).
// SPDX-License-Identifier: GPL-2.0-only
mod device;
mod fd;
mod measure;
mod scratch;

use super::util;
use crate::reply::{Failure, Outcome, Reply};
use device::{Io, Kernel};
use fd::fstatvfs;
use measure::Bench;
use scratch::{observe_storage, Scratch};
use serde_json::{json, Map, Value};
use std::io;
use y2p_core::{
    observation_support::{run_passing_fd, PyRandom},
    py, Root,
};

/// An exception the benchmarks catch: its class, `str()` and `errno`.
#[derive(Debug)]
struct Error {
    class: String,
    text: String,
    errno: Option<i32>,
}

impl Error {
    fn value(text: &str) -> Error {
        Error {
            class: "ValueError".into(),
            text: text.into(),
            errno: None,
        }
    }

    /// `OSError(errno, text)`.
    fn raised(errno: i32, text: &str) -> Error {
        Error {
            class: py::os_error_class(errno).into(),
            text: format!("[Errno {errno}] {text}"),
            errno: Some(errno),
        }
    }

    /// An OSError from a call (with the name Python passed it, if any).
    fn os(error: io::Error, name: Option<&str>) -> Error {
        let (text, class) = py::os_error(&error, name);
        Error {
            class: class.into(),
            text,
            errno: error.raw_os_error(),
        }
    }
}

type Result<T> = std::result::Result<T, Error>;

/// `y2-platform bench-storage`.
pub fn storage_benchmark(
    root: &Root,
    volume: &str,
    size_mib: i64,
    operations: i64,
    seconds: i64,
) -> Outcome {
    storage_benchmark_with(root, &Kernel, volume, size_mib, operations, seconds)
}

/// `storage_benchmark` with its scratch I/O and counters from `io`.
fn storage_benchmark_with(
    root: &Root,
    io: &dyn Io,
    volume: &str,
    size_mib: i64,
    operations: i64,
    seconds: i64,
) -> Outcome {
    if !(1..=128).contains(&size_mib)
        || !(1..=1024).contains(&operations)
        || !(1..=1800).contains(&seconds)
    {
        return Err(Failure::value("benchmark_bounds_exceeded"));
    }
    let size = size_mib as usize * 1024 * 1024;
    let operations = operations as usize;
    let deadline = root.clock().monotonic() + seconds as f64;
    let mut parameters = Map::new();
    parameters.insert("volume".into(), json!(volume));
    parameters.insert("size_mib".into(), json!(size_mib));
    parameters.insert("operations".into(), json!(operations));
    parameters.insert("deadline_seconds".into(), json!(seconds));
    parameters.insert("io".into(), json!("buffered"));
    parameters.insert("cold_read".into(), json!("not_guaranteed"));
    parameters.insert("seed".into(), json!(6582));
    let mut record = util::record(root, "storage", parameters);
    record.insert(
        "units".into(),
        json!({"latency": "ms", "throughput": "MB/s (decimal)", "rate": "operations/s"}),
    );
    let mut result = Map::new();
    result.insert("schema".into(), json!("org.y2linux.benchmark/v1"));
    result.insert("measurements".into(), json!([]));
    result.insert("volume".into(), Value::Null);
    result.insert("cache_advice".into(), json!([]));
    result.insert("integrity".into(), json!([]));
    let mut rng = PyRandom::new(6582);
    // A repeatable nonzero 1-MiB pattern prepared outside timed I/O.
    let block = rng.randbytes(1024 * 1024);
    let mut bench = Bench {
        root,
        io,
        deadline,
        result,
    };
    let outcome = (|| -> Result<()> {
        let mut scratch = Scratch::new(
            root,
            volume,
            (size + 3 * 1024 * 1024 + operations * 4096) as u64,
        )?;
        let initial = &scratch.initial;
        let mut described = Map::new();
        described.insert("path".into(), json!(volume));
        described.insert(
            "generation".into(),
            json!([initial.0, initial.1, initial.2, initial.3]),
        );
        described.insert("uuid".into(), scratch.initial_uuid.clone());
        described.insert("observation".into(), observe_storage(root)?);
        bench
            .result
            .insert("volume".into(), Value::Object(described));
        let ran = bench.run(&mut scratch, size, operations, &mut rng, &block);
        scratch.close();
        ran
    })();
    let mut result = bench.result;
    match outcome {
        Ok(()) => {
            record.insert("result".into(), json!("OK"));
        }
        Err(error) => {
            record.insert("result".into(), json!("FAILED"));
            record.insert(
                "failure".into(),
                json!({"type": error.class, "reason": error.text, "errno": error.errno}),
            );
        }
    }
    let failed = record["result"] == "FAILED";
    result.insert("record".into(), Value::Object(record));
    Reply::sorted(&result, u8::from(failed))
}

/// Python's `json.loads` error text for output that is not JSON (the messages a truncated or empty
/// answer gives; others are worded from serde's position).
fn json_error(text: &str, error: &serde_json::Error) -> String {
    let column = error.column();
    let line = error.line();
    let offset: usize = text
        .split('\n')
        .take(line.saturating_sub(1))
        .map(|l| l.len() + 1)
        .sum::<usize>()
        + column.saturating_sub(1);
    if error.classify() == serde_json::error::Category::Syntax
        && error.to_string().starts_with("trailing characters")
    {
        format!("Extra data: line {line} column {column} (char {offset})")
    } else if text.trim().is_empty() {
        "Expecting value: line 1 column 1 (char 0)".into()
    } else {
        format!("Expecting value: line {line} column {column} (char {offset})")
    }
}

/// `y2-platform bench-library`.
pub fn library_benchmark(root: &Root, volume: &str, tracks: i64, scan: bool) -> Outcome {
    let mut parameters = Map::new();
    parameters.insert("volume".into(), json!(volume));
    parameters.insert("tracks".into(), json!(tracks));
    parameters.insert("scan".into(), json!(scan));
    let mut record = util::record(root, "reborn-library", parameters);
    record.insert(
        "units".into(),
        json!({"latency": "ms", "memory": "KiB", "size": "bytes"}),
    );
    let mut measurements = Value::Null;
    let outcome = (|| -> Result<Value> {
        let mut scratch = Scratch::new(root, volume, (tracks as u64) * 8192 + 32 * 1024 * 1024)?;
        let vfs = fstatvfs(scratch.fd())?;
        if scan && vfs.files != 0 && (vfs.favail as i64) < tracks + 2048 {
            return Err(Error::value("insufficient_scratch_inode_reserve"));
        }
        scratch.child_outputs = true;
        let fd = scratch.fd().to_string();
        let count = tracks.to_string();
        let mut argv = vec![
            "/usr/bin/reborn-bench",
            "--scratch-fd",
            &fd,
            "--tracks",
            &count,
        ];
        if scan {
            argv.push("--scan");
        }
        let answer = run_passing_fd(root, &argv, 1800.0, 131_072, scratch.fd());
        scratch.ensure()?;
        if !answer.ok {
            return Err(Error {
                class: "RuntimeError".into(),
                text: answer.reason.unwrap_or_else(|| "None".into()),
                errno: None,
            });
        }
        let output = answer.output.unwrap_or_default();
        serde_json::from_str::<Value>(&output).map_err(|e| Error::value(&json_error(&output, &e)))
    })();
    match outcome {
        Ok(value) => {
            measurements = value;
            record.insert("result".into(), json!("OK"));
        }
        Err(error) => {
            record.insert("result".into(), json!("FAILED"));
            record.insert("failure".into(), json!(error.text));
        }
    }
    let failed = record["result"] == "FAILED";
    let result = json!({"schema": "org.y2linux.benchmark/v1", "record": record, "measurements": measurements});
    Reply::sorted(&result, u8::from(failed))
}

#[cfg(test)]
mod storage_tests;

#[cfg(test)]
mod tests {
    use super::device::distribution;
    use super::*;

    #[test]
    fn distributions_follow_bench() {
        let d = distribution(&[3_000_000, 1_000_000, 2_000_000], 300, Some(6_000_000));
        assert_eq!(d["p50_ms"], 2.0);
        assert_eq!(d["p95_ms"], 3.0);
        assert_eq!(d["MB_per_second"], 0.05);
        assert_eq!(d["operations_per_second"], 500.0);
        assert_eq!(distribution(&[], 0, None).len(), 7);
    }

    #[test]
    fn json_errors_read_as_python() {
        let check =
            |text: &str| json_error(text, &serde_json::from_str::<Value>(text).unwrap_err());
        assert_eq!(check(""), "Expecting value: line 1 column 1 (char 0)");
        assert_eq!(check("{\"a\":1} x"), "Extra data: line 1 column 9 (char 8)");
    }
}
