//! The storage benchmark on a fixture's /data (from tests/test_platform_storage_tools.py). Python's
//! tests passed a `guard` instead of a mount; the port has none, so /data is a fixture mount that
//! the real identity checks accept, measured on the host's temporary filesystem.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::{ok, scripted, Fixture};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    fs,
    io::Write,
    os::unix::fs::symlink,
};

thread_local! {
    static WRITE_FAULT: Cell<Option<i32>> = const { Cell::new(None) };
    static ALIASED_SIZE: Cell<Option<usize>> = const { Cell::new(None) };
    static ADVICE: Cell<Option<i32>> = const { Cell::new(None) };
    static SNAPSHOTS: RefCell<VecDeque<Value>> = const { RefCell::new(VecDeque::new()) };
}

/// Every write fails with this errno.
pub(super) fn write_fault() -> Option<i32> {
    WRITE_FAULT.with(Cell::get)
}

/// A read of ALIASED_SIZE bytes reads offset 0 (address aliasing).
pub(super) fn read_offset(size: usize, offset: usize) -> usize {
    if ALIASED_SIZE.with(Cell::get) == Some(size) {
        0
    } else {
        offset
    }
}

/// posix_fadvise answers this errno.
pub(super) fn advice(error: i32) -> i32 {
    ADVICE.with(Cell::get).unwrap_or(error)
}

/// The next queued io_snapshot, while there are any.
pub(super) fn snapshot() -> Option<Value> {
    SNAPSHOTS.with(|s| s.borrow_mut().pop_front())
}

const DATA_UUID: &str = "79324c69-6e75-4801-8000-000000000102";

/// Y2DATA mounted at /data on the eMMC (p6, disk sequence 1).
fn data() -> Fixture {
    let fixture = Fixture::new();
    fixture.mkdir("/data");
    fixture.put("/proc/sys/kernel/random/boot_id", "boot-one");
    fixture.put(
        "/proc/self/mountinfo",
        "20 1 179:6 / /data rw,nosuid - ext4 /dev/mmcblk0p6 rw",
    );
    let part = "/sys/devices/platform/11230000.mmc/block/mmcblk0/mmcblk0p6";
    fixture.put(&format!("{part}/partition"), "6");
    fixture.put(&format!("{part}/dev"), "179:6");
    fixture.put(
        "/sys/devices/platform/11230000.mmc/block/mmcblk0/diskseq",
        "1",
    );
    fixture.mkdir("/sys/dev/block");
    symlink(fixture.path(part), fixture.path("/sys/dev/block/179:6")).unwrap();
    fixture
}

fn blkid() -> Vec<(Vec<String>, y2p_core::command::Outcome)> {
    vec![ok(
        &["blkid", "-p", "-s", "UUID", "-o", "value", "/dev/mmcblk0p6"],
        DATA_UUID,
    )]
}

/// `storage_benchmark(ctx, size_mib=1, operations=…)` as its JSON answer.
fn benchmark(fixture: &Fixture, operations: i64, seconds: i64) -> Value {
    let (reply, _) = scripted(blkid(), || {
        storage_benchmark(&fixture.root, "/data", 1, operations, seconds)
    });
    serde_json::from_str(&reply.unwrap().stdout).unwrap()
}

fn scratch_is_empty(fixture: &Fixture) -> bool {
    fs::read_dir(fixture.path("/data/.y2-bench"))
        .unwrap()
        .next()
        .is_none()
}

fn sd_crc(events: u64) -> Value {
    json!({"cpu_ticks": null, "irqs": null, "controllers": {"sd": {"crc_events": events}}})
}

#[test]
fn real_scratch_readback_latency_and_no_live_data_changes() {
    let fixture = data();
    fixture.put("/data/music/track.flac", "valuable music");
    fixture.put("/data/reborn/state/settings.json", "valuable state");
    let result = benchmark(&fixture, 2, 10);
    assert_eq!(
        result["record"]["result"], "OK",
        "{}",
        result["record"]["failure"]
    );
    let measurements = result["measurements"].as_array().unwrap();
    assert_eq!(measurements.len(), 24);
    let integrity = result["integrity"].as_array().unwrap();
    assert_eq!(integrity.len(), 5);
    assert!(integrity
        .iter()
        .all(|r| r["expected_sha256"] == r["readback_sha256"]));
    assert!(measurements
        .iter()
        .all(|m| m["samples"].as_u64() > Some(0) && m["p99_ms"].as_f64() >= m["p50_ms"].as_f64()));
    assert_eq!(fixture.read("/data/music/track.flac"), "valuable music");
    assert_eq!(
        fixture.read("/data/reborn/state/settings.json"),
        "valuable state"
    );
    assert!(scratch_is_empty(&fixture));
}

#[test]
fn source_loss_during_operation_leaves_replacement_untouched() {
    let fixture = data();
    let (failure, _) = scripted(blkid(), || {
        let mut scratch = Scratch::new(&fixture.root, "/data", 4096).unwrap();
        scratch
            .open("payload")
            .unwrap()
            .write_all(b"old media")
            .unwrap();
        fs::rename(fixture.path("/data"), fixture.path("/detached")).unwrap();
        fixture.mkdir("/data");
        fixture.put("/data/payload", "new card");
        // Another mount of another card (Python's guard returned a new epoch).
        fixture.put(
            "/proc/self/mountinfo",
            "21 1 179:6 / /data rw,nosuid - ext4 /dev/mmcblk0p6 rw",
        );
        scratch.open("replacement").err()
    });
    assert!(failure.unwrap().text.contains("generation"));
    assert_eq!(fixture.read("/data/payload"), "new card");
    assert!(!fixture.exists("/data/.y2-bench"));
}

#[test]
fn symlink_scratch_and_arbitrary_volumes_refused() {
    let fixture = data();
    fixture.mkdir("/state");
    symlink(fixture.path("/state"), fixture.path("/data/.y2-bench")).unwrap();
    let (refused, _) = scripted(blkid(), || {
        let linked = Scratch::new(&fixture.root, "/data", 4096).err().unwrap();
        let music = Scratch::new(&fixture.root, "/data/music", 4096)
            .err()
            .unwrap();
        (linked, music)
    });
    assert!(refused.0.errno.is_some(), "{:?}", refused.0); // an OSError
    assert_eq!(
        (refused.1.class.as_str(), refused.1.text.as_str()),
        ("ValueError", "scratch_volume_not_allowed")
    );
    assert!(fs::read_dir(fixture.path("/state"))
        .unwrap()
        .next()
        .is_none());
}

#[test]
fn io_error_is_reported_with_completed_samples_not_a_pass() {
    let fixture = data();
    WRITE_FAULT.with(|f| f.set(Some(libc::ENOSPC)));
    let result = benchmark(&fixture, 1, 300);
    WRITE_FAULT.with(|f| f.set(None));
    assert_eq!(result["record"]["result"], "FAILED");
    assert_eq!(result["record"]["failure"]["errno"], libc::ENOSPC);
    assert_eq!(result["measurements"], json!([]));
    assert!(scratch_is_empty(&fixture));
}

#[test]
fn percentiles_and_units() {
    let result = distribution(
        &[1_000_000, 4_000_000, 2_000_000, 3_000_000],
        1_000_000,
        Some(10_000_000),
    );
    assert_eq!(result["p50_ms"], 2.0);
    assert_eq!(result["p95_ms"], 4.0);
    assert_eq!(result["MB_per_second"], 100.0);
    assert_eq!(result["operations_per_second"], 400.0);
}

#[test]
fn retried_controller_fault_is_not_a_clean_benchmark_pass() {
    let fixture = data();
    SNAPSHOTS.with(|s| *s.borrow_mut() = VecDeque::from([sd_crc(0), sd_crc(1)]));
    let result = benchmark(&fixture, 1, 300);
    SNAPSHOTS.with(|s| s.borrow_mut().clear());
    assert_eq!(result["record"]["result"], "FAILED");
    assert!(result["record"]["failure"]["reason"]
        .as_str()
        .unwrap()
        .contains("controller_fault_during_benchmark"));
    let measurements = result["measurements"].as_array().unwrap();
    assert_eq!(measurements.len(), 1);
    assert_eq!(
        measurements[0]["system"]["controllers"]["sd"]["delta"]["crc_events"],
        1
    );
}

#[test]
fn offset_specific_sha256_detects_address_aliasing() {
    let fixture = data();
    ALIASED_SIZE.with(|a| a.set(Some(4099)));
    let result = benchmark(&fixture, 1, 300);
    ALIASED_SIZE.with(|a| a.set(None));
    assert_eq!(result["record"]["result"], "FAILED");
    assert!(result["record"]["failure"]["reason"]
        .as_str()
        .unwrap()
        .contains("sha256_readback_mismatch"));
    assert!(scratch_is_empty(&fixture));
}

#[test]
fn retried_fault_during_sha256_is_not_a_clean_integrity_pass() {
    let fixture = data();
    let mut queue: VecDeque<Value> = std::iter::repeat_n(sd_crc(0), 49).collect();
    queue.push_back(sd_crc(1));
    SNAPSHOTS.with(|s| *s.borrow_mut() = queue);
    let result = benchmark(&fixture, 1, 300);
    SNAPSHOTS.with(|s| s.borrow_mut().clear());
    assert_eq!(result["integrity"].as_array().unwrap().len(), 5);
    assert_eq!(result["record"]["result"], "FAILED");
    assert_eq!(
        result["integrity_system"]["controllers"]["sd"]["delta"]["crc_events"],
        1
    );
}

#[test]
fn benchmark_reports_cpu_irq_and_storage_error_deltas() {
    let first = json!({"cpu_ticks": [1, 0, 1, 10, 2, 0, 0, 0], "irqs": {"39": 5},
                       "controllers": {"sd": {"level": "SDR104", "crc_events": 0}}});
    let last = json!({"cpu_ticks": [2, 0, 2, 12, 2, 0, 0, 0], "irqs": {"39": 8},
                      "controllers": {"sd": {"level": "DDR50", "crc_events": 1}}});
    let delta = io_delta(&first, &last);
    assert_eq!(delta["cpu_busy_percent"], 50.0);
    assert_eq!(delta["irq_delta"], json!({"39": 3}));
    assert_eq!(delta["controllers"]["sd"]["delta"]["crc_events"], 1);
    assert!(delta["controllers"]["sd"]["delta"]["timeout_events"].is_null());
}

#[test]
fn missing_cache_advice_preserves_readback_without_cold_read_claim() {
    let fixture = data();
    ADVICE.with(|a| a.set(Some(libc::EINVAL)));
    let result = benchmark(&fixture, 1, 300);
    ADVICE.with(|a| a.set(None));
    assert_eq!(result["record"]["result"], "OK");
    let advice = result["cache_advice"].as_array().unwrap();
    assert_eq!(advice.len(), 13);
    assert!(advice
        .iter()
        .all(|a| a["state"] == "unavailable" && a["errno"] == libc::EINVAL));
    assert_eq!(
        result["record"]["parameters"]["cold_read"],
        "not_guaranteed"
    );
    assert!(result["measurements"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["operation"] == "random_read_1048576"));
    assert!(scratch_is_empty(&fixture));
}

#[test]
fn cache_advice_io_fault_is_not_downgraded_to_unavailable() {
    let fixture = data();
    ADVICE.with(|a| a.set(Some(libc::EIO)));
    let result = benchmark(&fixture, 1, 300);
    ADVICE.with(|a| a.set(None));
    assert_eq!(result["record"]["result"], "FAILED");
    assert_eq!(result["record"]["failure"]["errno"], libc::EIO);
    assert!(!result["measurements"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["operation"].as_str().unwrap().contains("read_")));
}
