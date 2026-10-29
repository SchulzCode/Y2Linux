//! Command lines of the observation group on fixture roots (were tests/test_platform_telemetry_regressions.py
//! and tests/test_platform_measurement.py).
// SPDX-License-Identifier: GPL-2.0-only
mod common;

use common::Fixture;
use serde_json::{json, Value};
use std::{fs, thread, time::Duration};

#[test]
fn health_cli_dispatches_quick_and_full_checks_and_preserves_failure() {
    let fixture = Fixture::new();
    fixture.put("/sys/fs/ext4/test/errors_count", "1\n");
    for full in [&[][..], &["--full"][..]] {
        let args: Vec<&str> = ["--json"].iter().chain(full).copied().collect();
        let platform: Vec<&str> = ["health"].iter().chain(&args).copied().collect();
        // `y2-platform health …` and the y2-health link.
        for run in [
            fixture.run(&platform),
            fixture.run_as(Some("y2-health"), &args, &[]),
        ] {
            assert_eq!(run.code, Some(1), "{full:?}: {}", run.stderr);
            let result = run.json();
            assert_eq!(result["schema"], "org.y2linux.health/v1");
            assert_eq!(result["state"], "FAILED");
            assert!(result["record"]["failure"]
                .as_array()
                .unwrap()
                .contains(&json!("filesystem_errors")));
            assert_eq!(
                result["record"]["parameters"]["mode"],
                if full.is_empty() { "quick" } else { "full" }
            );
            let checks = result["checks"].as_array().unwrap();
            assert_eq!(
                checks.iter().any(|c| c["name"] == "alsa_query"),
                !full.is_empty()
            );
        }
    }
}

#[test]
fn network_cli_still_dispatches_to_network_check() {
    // Python replaced network_check.check and compared its arguments (peer, 10 s, no throughput,
    // wlan0); here the check runs and records them (the radio is off: wifi_not_online).
    let fixture = Fixture::new();
    let run = fixture.run(&["network-check", "--peer", "192.0.2.1"]);
    assert_eq!(run.code, Some(1), "{}", run.stderr);
    let result = run.json();
    assert_eq!(result["schema"], "org.y2linux.benchmark/v1");
    assert_eq!(result["record"]["workload"], "network-qualification");
    assert_eq!(
        result["record"]["parameters"],
        json!({"peer": "192.0.2.1", "seconds": 10, "throughput_requested": false,
               "interface": "wlan0"})
    );
    assert_eq!(result["record"]["failure"], "wifi_not_online");
}

/// `/proc/<pid>/stat` with `start` as field 22 (starttime).
fn stat(start: u64) -> String {
    format!("123 (reborn) S{} {start} 0 0\n", " 0".repeat(18))
}

#[test]
fn growth_window_and_pid_reuse() {
    let fixture = Fixture::new();
    fixture
        .put("/proc/123/status", "Name:\treborn\nVmRSS:\t100 kB\n")
        .put("/proc/123/stat", &stat(10));
    // Samples at 0, 1 and 2 s: PID 123 is a new process (start ticks 20) from the third on.
    let path = fixture.path("/proc/123/stat");
    let reuse = thread::spawn(move || {
        thread::sleep(Duration::from_millis(1500));
        fs::write(path, stat(20)).unwrap();
    });
    let run = fixture.run(&[
        "collect",
        "--seconds",
        "3",
        "--interval",
        "1",
        "--warmup",
        "0",
        "--pid",
        "123",
    ]);
    reuse.join().unwrap();
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    let lines: Vec<Value> = run
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[0]["record"]["workload"], "idle");
    let summary = &lines[3];
    assert_eq!(summary["samples"], 3);
    let growth = summary["growth"].as_object().unwrap();
    assert_eq!(growth.len(), 2, "{growth:?}");
    assert_eq!(
        growth
            .keys()
            .map(|k| k.rsplit(':').next().unwrap())
            .collect::<Vec<_>>(),
        ["10", "20"]
    );
}
