//! What a measurement writes and reads, and the passive counters it is compared by (CPU ticks,
//! interrupts, the MMC controllers' error counters).
// SPDX-License-Identifier: GPL-2.0-only
use super::{fd::borrowed, Error, Result};
use crate::commands::observation::{
    snapshot::storage_mode,
    util::{self, read, split_ws},
};
use crate::regex;
use serde_json::{json, Map, Value};
use std::{
    collections::BTreeSet,
    io,
    os::fd::{BorrowedFd, RawFd},
};
use y2p_core::Root;

/// What a measurement does to its scratch file and reads around each step: the kernel's calls
/// (`Kernel`), or a test's that brings the faults a fixture cannot cause (a failing write, aliased
/// addresses, refused cache advice, controller counters that move).
pub(super) trait Io {
    fn pwrite(&self, fd: BorrowedFd, data: &[u8], offset: u64) -> io::Result<usize> {
        y2p_core::sys::pwrite(fd, data, offset)
    }

    fn pread(&self, fd: BorrowedFd, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
        y2p_core::sys::pread(fd, buffer, offset)
    }

    /// `posix_fadvise(fd, 0, 0, POSIX_FADV_DONTNEED)`: 0 or the errno.
    fn drop_cache(&self, fd: RawFd) -> i32 {
        // SAFETY: an open descriptor; the whole file.
        unsafe { libc::posix_fadvise(fd, 0, 0, libc::POSIX_FADV_DONTNEED) }
    }

    /// The passive counters a measurement is compared by.
    fn counters(&self, root: &Root) -> Value {
        io_snapshot(root)
    }
}

/// The kernel's calls and counters.
pub(super) struct Kernel;

impl Io for Kernel {}

/// Write all of `data` at `offset` (pwrite again after a short write).
pub(super) fn write_all(io: &dyn Io, fd: RawFd, data: &[u8], offset: usize) -> Result<()> {
    let mut written = 0;
    while written < data.len() {
        let n = io.pwrite(borrowed(fd), &data[written..], (offset + written) as u64)?;
        if n == 0 {
            return Err(Error::raised(libc::EIO, "short_write"));
        }
        written += n;
    }
    Ok(())
}

/// One pread of up to `size` bytes at `offset`.
pub(super) fn pread(io: &dyn Io, fd: RawFd, size: usize, offset: usize) -> Result<Vec<u8>> {
    let mut buffer = vec![0u8; size];
    let n = io.pread(borrowed(fd), &mut buffer, offset as u64)?;
    buffer.truncate(n);
    Ok(buffer)
}

/// Latency percentiles (nearest rank), throughput and rate of a measurement's samples (ns).
pub(super) fn distribution(
    samples: &[u64],
    bytes: u64,
    duration: Option<u64>,
) -> Map<String, Value> {
    let mut result = Map::new();
    if samples.is_empty() {
        for (k, v) in [
            ("samples", json!(0)),
            ("p50_ms", Value::Null),
            ("p95_ms", Value::Null),
            ("p99_ms", Value::Null),
            ("max_ms", Value::Null),
            ("MB_per_second", Value::Null),
            ("operations_per_second", Value::Null),
        ] {
            result.insert(k.into(), v);
        }
        return result;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let maximum = sorted.last().copied().unwrap_or_default();
    let elapsed = duration.unwrap_or_else(|| sorted.iter().sum());
    let percentile = |p: f64| {
        let index = ((sorted.len() as f64 * p).ceil() as i64 - 1).max(0) as usize;
        sorted[index] as f64 / 1e6
    };
    result.insert("samples".into(), json!(sorted.len()));
    result.insert("p50_ms".into(), json!(percentile(0.5)));
    result.insert("p95_ms".into(), json!(percentile(0.95)));
    result.insert("p99_ms".into(), json!(percentile(0.99)));
    result.insert("max_ms".into(), json!(maximum as f64 / 1e6));
    let rate = |numerator: f64| {
        if elapsed != 0 {
            json!(numerator / elapsed as f64)
        } else {
            Value::Null
        }
    };
    result.insert("MB_per_second".into(), rate((bytes * 1000) as f64));
    result.insert(
        "operations_per_second".into(),
        rate(sorted.len() as f64 * 1e9),
    );
    result.insert("bytes".into(), json!(bytes));
    result.insert("elapsed_ms".into(), json!(elapsed as f64 / 1e6));
    result
}

/// The passive counters (CPU ticks, interrupt totals, the MMC controllers' modes and error counts);
/// never resume MMC just to observe an idle gate.
pub(super) fn io_snapshot(root: &Root) -> Value {
    let stat = root.read("/proc/stat").unwrap_or_default();
    let ticks = regex!(r"(?m)^cpu\s+([0-9 ]+)$").captures(&stat).map(|c| {
        split_ws(&c[1])
            .iter()
            .filter_map(|v| v.parse::<i64>().ok())
            .take(8)
            .collect::<Vec<_>>()
    });
    let mut irqs = Map::new();
    for row in y2p_core::text::interrupt_rows(&root.read("/proc/interrupts").unwrap_or_default()) {
        if !row.counts.is_empty() {
            let total: i64 = row
                .counts
                .iter()
                .map(|c| c.parse::<i64>().unwrap_or(0))
                .sum();
            irqs.insert(row.key.to_owned(), json!(total));
        }
    }
    let mut controllers = Map::new();
    for path in root.glob("/sys/bus/platform/devices/*.mmc/y2_storage") {
        controllers.insert(
            util::parent_name(&path),
            storage_mode(read(&path).as_deref()),
        );
    }
    json!({"cpu_ticks": ticks, "irqs": if irqs.is_empty() { Value::Null } else { Value::Object(irqs) },
           "controllers": controllers})
}

/// What changed between two counter readings: an unavailable or reset counter is an explicit null,
/// not a zero-error pass.
pub(super) fn io_delta(before: &Value, after: &Value) -> Value {
    let mut busy = Value::Null;
    if let (Some(a), Some(b)) = (
        before["cpu_ticks"].as_array(),
        after["cpu_ticks"].as_array(),
    ) {
        let (a, b): (Vec<i64>, Vec<i64>) = (
            a.iter().filter_map(Value::as_i64).collect(),
            b.iter().filter_map(Value::as_i64).collect(),
        );
        if !a.is_empty()
            && !b.is_empty()
            && a.len() == b.len()
            && a.iter().zip(&b).all(|(x, y)| y >= x)
        {
            let delta: Vec<i64> = a.iter().zip(&b).map(|(x, y)| y - x).collect();
            let total: i64 = delta.iter().sum();
            if total != 0 && delta.len() >= 5 {
                busy = json!((100 * (total - delta[3] - delta[4])) as f64 / total as f64);
            }
        }
    }
    let irq = match (before["irqs"].as_object(), after["irqs"].as_object()) {
        (Some(a), Some(b)) => Value::Object(
            a.iter()
                .map(|(k, v)| {
                    let (v, now) = (v.as_i64().unwrap_or(0), b.get(k).and_then(Value::as_i64));
                    (
                        k.clone(),
                        now.filter(|n| *n >= v)
                            .map_or(Value::Null, |n| json!(n - v)),
                    )
                })
                .collect(),
        ),
        _ => Value::Null,
    };
    let counters = [
        "crc_events",
        "timeout_events",
        "controller_events",
        "tuning_events",
        "voltage_events",
        "verify_events",
        "fallbacks",
        "resets",
    ];
    let empty = Map::new();
    let names: BTreeSet<&String> = before["controllers"]
        .as_object()
        .unwrap_or(&empty)
        .keys()
        .chain(after["controllers"].as_object().unwrap_or(&empty).keys())
        .collect();
    let mut controllers = Map::new();
    for name in names {
        let pick = |side: &Value| match &side["controllers"][name.as_str()] {
            Value::Object(map) => map.clone(),
            _ => Map::new(),
        };
        let (a, b) = (pick(before), pick(after));
        let delta: Map<String, Value> = counters
            .iter()
            .map(|k| {
                let value = match (
                    util::is_int(a.get(*k)).filter(|_| a[*k].is_number()),
                    util::is_int(b.get(*k)).filter(|_| b[*k].is_number()),
                ) {
                    (Some(x), Some(y)) if y >= x => json!((y - x) as i64),
                    _ => Value::Null,
                };
                (k.to_string(), value)
            })
            .collect();
        controllers.insert(
            name.clone(),
            json!({"before": a, "after": b, "delta": delta}),
        );
    }
    json!({"cpu_busy_percent": busy, "irq_delta": irq, "controllers": controllers})
}

pub(super) fn controller_fault(system: &Value) -> bool {
    system["controllers"].as_object().is_some_and(|c| {
        c.values().any(|c| {
            c["delta"]
                .as_object()
                .is_some_and(|d| d.values().any(|v| v.as_i64().is_some_and(|v| v > 0)))
        })
    })
}
