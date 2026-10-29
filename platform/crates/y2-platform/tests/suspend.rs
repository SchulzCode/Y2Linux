//! The product sleep helper as its own process (`__product-sleep <fd>`, the request's lock its
//! inherited descriptor): what a unit test cannot do in a test harness with many threads.
// SPDX-License-Identifier: GPL-2.0-only
mod common;

use common::{Fixture, BINARY};
use serde_json::json;
use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    os::unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// test_product_auto_sleep.py's player: qualified, on battery, nothing running.
fn player() -> Fixture {
    let fixture = Fixture::new();
    for (name, text) in [
        ("/proc/sys/kernel/random/boot_id", "same-boot"),
        ("/proc/sys/kernel/osrelease", "corrected-kernel"),
        ("/proc/sys/kernel/tainted", "0"),
        (
            "/etc/y2linux/versions.json",
            r#"{"build_git_commit":"corrected-source"}"#,
        ),
        ("/sys/power/pm_test", "[none] core"),
        ("/sys/power/state", "mem"),
        ("/sys/firmware/y2_pm/stage", ""),
        (
            "/sys/firmware/y2_pm/backstop_s",
            "provider=1 armed_s=0 running=0",
        ),
        ("/sys/firmware/y2_pm/state", "valid=0 stage=EXIT error=0"),
        (
            "/sys/bus/platform/devices/10006000.power-controller/state",
            "system_sleep_abi=1 broken=0 timer_wake=0 ticks32k=0",
        ),
        ("/sys/class/power_supply/y2-usb-presence/online", "0"),
        ("/sys/class/power_supply/BAT0/status", "Discharging"),
        ("/sys/class/power_supply/BAT0/voltage_now", "3900000"),
        ("/sys/module/workload/parameters/leases", ""),
        (
            "/proc/interrupts",
            "  66: 1 0 mt6323 20 Level mt6397-rtc\n  70: 2 0 mt6323 5 Level mtk-pmic-keys\n",
        ),
        ("/data/.y2data-schema", "1"),
        ("/run/y2/suspend.lock", ""),
    ] {
        fixture.put(name, text);
    }
    std::fs::create_dir_all(fixture.path("/data/updates")).unwrap();
    fixture.put(
        "/etc/y2linux/power-policy.json",
        &std::fs::read_to_string(
            common::repository()
                .join("buildroot/board/y2/production-overlay/etc/y2linux/power-policy.json"),
        )
        .unwrap(),
    );
    fixture.put_json(
        "/run/y2/power.json",
        &json!({"boot_id": "same-boot", "state": "Normal", "monotonic_ns": y2p_core::clock::monotonic_ns()}),
    );
    fixture
}

/// `__product-sleep 0` with the request's lock as its stdin; ends it after `limit` (a hang fails).
fn product_sleep(fixture: &Fixture, limit: Duration) -> (Option<i32>, String) {
    let lock = File::open(fixture.path("/run/y2/suspend.lock")).unwrap();
    let mut child = Command::new(BINARY)
        .args(["__product-sleep", "0"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("Y2_ROOT", &fixture.root)
        .stdin(lock)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let until = Instant::now() + limit;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let output = child.wait_with_output().unwrap();
            return (
                status.code(),
                String::from_utf8_lossy(&output.stdout).into(),
            );
        }
        if Instant::now() > until {
            let _ = child.kill();
            let _ = child.wait();
            panic!("the helper did not return within {limit:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn active_wakeup_count_read_is_bounded_without_periodic_polling() {
    // An active wake source: the read blocks (here a FIFO with a writer and no data).
    let fixture = player();
    let path = fixture.path("/sys/power/wakeup_count");
    let name = CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: mkfifo with a valid path.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let _holder = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&path)
        .unwrap();
    let (code, stdout) = product_sleep(&fixture, Duration::from_secs(60));
    assert_eq!(code, Some(2));
    assert!(stdout.is_empty());
    let receipt = fixture.json("/data/system/platform/suspend-last.json");
    assert_eq!(receipt["product"]["state"], "refused");
    assert_eq!(receipt["product"]["reason"], "wakeup_count_busy");
}
