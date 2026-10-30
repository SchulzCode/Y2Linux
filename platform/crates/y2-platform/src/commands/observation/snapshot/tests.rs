//! Status snapshot boundaries on Linux-shaped fixtures, no hardware (were tests/test_platform_contract.py,
//! test_platform_telemetry_regressions.py, test_cpu_final.py and test_cpu_idle_completion.py).
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use super::{idle::idle_completion, storage::space_state};
use crate::testing::{ok, Fixture};
use std::{fs, path::PathBuf, sync::Arc, time::Instant};
use y2p_core::{
    clock::Manual,
    command::{self, Outcome},
};

/// A clock standing at 1000 s (monotonic and boottime).
fn at_1000_s() -> Arc<Manual> {
    Arc::new(Manual::at(1_000_000_000_000, 1_000_000_000_000))
}

const BLKID: [&str; 7] = ["blkid", "-p", "-s", "UUID", "-o", "value", "/dev/mmcblk1p1"];

/// The fixture's root with its commands answered from `answers` (anything else `fixture`), never
/// from the process environment (connectivity's crosscheck sets Y2_FIXTURE_COMMANDS while it runs).
fn answering(fixture: &Fixture, answers: Vec<(Vec<String>, Outcome)>) -> Root {
    fixture.root.clone().with_answers(answers)
}

/// The fixture's root with every command answered `fixture`.
fn unanswered(fixture: &Fixture) -> Root {
    answering(fixture, Vec::new())
}

/// The contract fixture: a boot id, a kernel release and the installed versions.
fn contract() -> Fixture {
    let fixture = Fixture::new();
    fixture
        .put("/proc/sys/kernel/random/boot_id", "boot-a")
        .put("/proc/sys/kernel/osrelease", "fixture-kernel")
        .put_json(
            "/etc/y2linux/versions.json",
            &json!({"build_git_commit": "commit", "rootfs_version": "release"}),
        );
    fixture
}

/// A card mounted at /media/sd with mount id `generation` and the platform's claim for it.
fn mounted_sd(fixture: &Fixture, generation: i64) -> PathBuf {
    fixture.put(
        "/proc/self/mountinfo",
        &format!("{generation} 1 179:9 / /media/sd rw - vfat /dev/mmcblk1p1 rw\n"),
    );
    let device = fixture.path("/sys/devices/platform/11240000.mmc/mmcblk1/mmcblk1p1");
    fs::create_dir_all(&device).unwrap();
    fs::write(device.join("partition"), "1").unwrap();
    fs::write(device.join("dev"), "179:9").unwrap();
    fs::write(device.parent().unwrap().join("diskseq"), "17").unwrap();
    let link = fixture.path("/sys/dev/block/179:9");
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    if link.symlink_metadata().is_err() {
        std::os::unix::fs::symlink(&device, &link).unwrap();
    }
    fixture.mkdir("/media/sd");
    fixture.put_json(
        "/run/y2/media-mount.json",
        &json!({"boot_id": fixture.root.read("/proc/sys/kernel/random/boot_id"),
                "mount_id": generation, "device_id": "179:9", "uuid": "card-one",
                "source_instance": device_instance(&fixture.root, "179:9")}),
    );
    device
}

/// The SD card volume, every command answered `card-one` as the contract fixture's runner did.
fn sd(fixture: &Fixture) -> Value {
    let root = answering(fixture, vec![ok(&BLKID, "card-one")]);
    storage(&root).unwrap()["volumes"][2].clone()
}

#[test]
fn disappeared_source_never_looks_like_an_empty_healthy_card() {
    let fixture = contract();
    let device = mounted_sd(&fixture, 51);
    assert_eq!(sd(&fixture)["state"], "Ready");
    fs::remove_dir_all(&device).unwrap();
    let absent = sd(&fixture);
    assert_eq!(absent["state"], "Failed");
    assert_eq!(absent["space_state"], "Failed");
    assert!(absent["uuid"].is_null());
    assert!(fixture.path("/media/sd").is_dir());
    fixture.put("/proc/self/mountinfo", "");
    assert_eq!(sd(&fixture)["state"], "Unavailable");
}

#[test]
fn reinsert_and_new_boot_change_generation_even_at_same_path() {
    let fixture = contract();
    mounted_sd(&fixture, 51);
    let first = sd(&fixture)["generation"].clone();
    mounted_sd(&fixture, 52);
    let second = sd(&fixture)["generation"].clone();
    fixture.put("/proc/sys/kernel/random/boot_id", "boot-b");
    let third = sd(&fixture)["generation"].clone();
    assert!(first.is_string());
    assert!(first != second && second != third && first != third);
}

#[test]
fn unknown_sensors_are_null_not_zero_and_die_temperature_is_separate() {
    let fixture = contract();
    fixture
        .put("/sys/class/thermal/thermal_zone0/type", "cpu-thermal")
        .put("/sys/class/thermal/thermal_zone0/temp", "46800")
        .put("/sys/class/power_supply/battery/type", "Battery")
        .put("/sys/class/power_supply/battery/voltage_now", "3900000")
        .put(
            "/sys/class/power_supply/battery/constant_charge_current",
            "450000",
        );
    let result = snapshot(&unanswered(&fixture), &[], false, 0.0).unwrap();
    assert_eq!(
        result["thermal"]["zones"][0]["temperature_millicelsius"],
        46800
    );
    assert!(result["power"]["pack_temperature"].is_null());
    assert!(result["power"]["measured_current_ua"].is_null());
    assert!(result["power"]["soc_percent"].is_null());
    assert_eq!(
        result["power"]["supplies"][0]["constant_charge_current"],
        "450000"
    );
    assert_eq!(result["record"]["evidence_level"], "IMPLEMENTED");
    // json.dumps(allow_nan=False): every number is finite.
    y2p_core::json::dumps_sorted(&result, y2p_core::json::Separators::Spaced).unwrap();
}

#[test]
fn cpu_delta_rejects_reset_and_missing_cores() {
    let a = json!({"cpu0": [10, 0, 10, 80, 0, 0, 0, 0]});
    let b = json!({"cpu0": [20, 0, 20, 160, 0, 0, 0, 0], "cpu1": [1, 1, 1, 1, 1, 1, 1, 1]});
    assert_eq!(utilization(Some(&a), Some(&b)), json!({"cpu0": 20.0}));
    assert!(utilization(Some(&b), Some(&a)).is_null());
    assert!(utilization(Some(&a), Some(&a)).is_null());
}

#[test]
fn wifi_authentication_is_insufficient_and_dns_epoch_expires() {
    let fixture = contract().with_clock(at_1000_s());
    fixture.mkdir("/sys/class/net/wlan0");
    let mut address: Vec<Value> = Vec::new();
    // The contract fixture's runner: the supplicant status, the address and route queries, '' else.
    let wifi = |address: &[Value]| {
        let answers = vec![
            ok(
                &["wpa_cli", "-i", "wlan0", "status"],
                "wpa_state=COMPLETED\nbssid=peer1",
            ),
            ok(
                &["ip", "-j", "-4", "addr", "show", "dev", "wlan0"],
                &json!([{"addr_info": address}]).to_string(),
            ),
            ok(
                &["ip", "-j", "-4", "route", "show", "default", "dev", "wlan0"],
                r#"[{"dst":"default","dev":"wlan0"}]"#,
            ),
            ok(&["wpa_cli", "-i", "wlan0", "signal_poll"], ""),
        ];
        super::wifi(&answering(&fixture, answers)).unwrap()
    };
    assert_eq!(wifi(&address)["state"], "AcquiringIP");
    fixture.put(
        "/run/y2/dhcp.json",
        r#"{"boot_id":"boot-a","error":"dhcp_timeout"}"#,
    );
    assert_eq!(wifi(&address)["reason"], "dhcp_timeout");
    address.push(json!({"scope": "global", "local": "192.0.2.7"}));
    assert_eq!(wifi(&address)["reason"], "dns_unavailable");
    fixture.put("/run/y2/wifi-epoch", "lease-one");
    let mut record = json!({"ok": true, "boot_id": "boot-a", "epoch": "lease-one",
                            "address": "192.0.2.7", "bssid": "peer1",
                            "monotonic_s": 1000.0});
    fixture.put_json("/run/y2/dns.json", &record);
    assert_eq!(wifi(&address)["state"], "Online");
    record["monotonic_s"] = json!(1000.0 - 421.0);
    fixture.put_json("/run/y2/dns.json", &record);
    assert_eq!(wifi(&address)["dns_ready"], false);
    record["monotonic_s"] = json!(1000.0);
    record["bssid"] = json!("peer2");
    fixture.put_json("/run/y2/dns.json", &record);
    assert_eq!(wifi(&address)["dns_ready"], false);
    fixture.put("/run/y2/dns.json", r#"{"monotonic_s":null}"#);
    assert_eq!(wifi(&address)["dns_ready"], false);
}

#[test]
fn space_reserve_and_read_only() {
    let mib = 1024 * 1024;
    assert_eq!(
        space_state(200 * mib, 800 * mib, false, false),
        y2p_core::volume::SpaceState::Normal
    );
    assert_eq!(
        space_state(90 * mib, 800 * mib, false, false),
        y2p_core::volume::SpaceState::LowSpace
    );
    assert_eq!(
        space_state(39 * mib, 800 * mib, false, false),
        y2p_core::volume::SpaceState::CriticalSpace
    );
    assert_eq!(
        space_state(200 * mib, 800 * mib, true, false),
        y2p_core::volume::SpaceState::ReadOnlyRisk
    );
    assert_eq!(
        space_state(200 * mib, 800 * mib, false, true),
        y2p_core::volume::SpaceState::Failed
    );
}

// ---- Telemetry regressions: the owner-observed charging-time read hang -------------------------

#[test]
fn supplicant_timeout_is_observation_failure_not_disconnect_evidence() {
    let fixture = Fixture::new().with_clock(at_1000_s());
    fixture.mkdir("/sys/class/net/wlan0");
    let status = ["wpa_cli", "-i", "wlan0", "status"];
    let answers = vec![
        crate::testing::failed(&status, "command_timeout"),
        ok(&["ip", "-j", "-4", "addr", "show", "dev", "wlan0"], "[]"),
        ok(
            &["ip", "-j", "-4", "route", "show", "default", "dev", "wlan0"],
            "[]",
        ),
    ];
    let root = answering(&fixture, answers);
    let result = wifi(&root).unwrap();
    let calls = root.calls();
    assert_eq!(result["reason"], "supplicant_unavailable");
    let observation = &result["supplicant_observation"];
    assert_eq!(observation["ok"], false);
    assert_eq!(observation["reason"], "command_timeout");
    // The clock stands still: the answer took no time.
    assert_eq!(observation["duration_ns"], 0);
    assert!(result["events"].is_null());
    // The accepted Wi-Fi policy allows measured >2 s STATUS replies. A timeout still means
    // unavailable observation, never disconnect.
    assert_eq!(calls[0], (status.map(String::from).to_vec(), 3.0));
    fs::remove_dir(fixture.path("/sys/class/net/wlan0")).unwrap();
    let root = unanswered(&fixture);
    let result = wifi(&root).unwrap();
    let calls = root.calls();
    assert_eq!(
        result["supplicant_observation"],
        json!({"ok": null, "reason": "radio_off", "duration_ns": null})
    );
    assert!(!calls.iter().any(|(argv, _)| argv[0] == "wpa_cli"));
}

/// The wakeup counter read as the real bounded runner does it (`/bin/cat` of the fixture's file, its
/// 0.1 s deadline and 64-byte limit), handed to `cpu` as its answer.
fn wakeup_answer(fixture: &Fixture) -> (Vec<String>, Outcome) {
    let argv = [
        "/bin/cat".to_owned(),
        fixture
            .path("/sys/power/wakeup_count")
            .to_string_lossy()
            .into_owned(),
    ];
    let words: Vec<&str> = argv.iter().map(String::as_str).collect();
    let outcome = command::run_with(&Root::new("/"), &words, 0.1, 64, false);
    (argv.to_vec(), outcome)
}

#[test]
fn active_wakeup_read_returns_without_leaving_a_reader() {
    let fixture = Fixture::new();
    let counter = fixture.path("/sys/power/wakeup_count");
    fs::create_dir_all(counter.parent().unwrap()).unwrap();
    // With no writer, opening this FIFO blocks like the active-wakeup sysfs read. Exercise a real
    // child, deadline, termination and reap.
    let name = std::ffi::CString::new(counter.to_string_lossy().as_bytes()).unwrap();
    // SAFETY: a valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let start = Instant::now();
    let answer = wakeup_answer(&fixture);
    let root = answering(&fixture, vec![answer.clone()]);
    let result = cpu(&root).unwrap();
    let calls = root.calls();
    assert!(start.elapsed().as_secs_f64() < 2.0);
    assert!(result["wakeup_count"].is_null());
    assert_eq!(result["wakeup_count_reason"], "command_timeout");
    let reads: Vec<_> = calls.iter().filter(|(argv, _)| *argv == answer.0).collect();
    assert_eq!(reads, vec![&(answer.0.clone(), 0.1)]);
    // No reader of the FIFO is left behind (killed and reaped).
    let path = counter.to_string_lossy().into_owned();
    let left = fs::read_dir("/proc").unwrap().flatten().any(|entry| {
        fs::read(entry.path().join("cmdline"))
            .is_ok_and(|c| String::from_utf8_lossy(&c).contains(&path))
    });
    assert!(!left);
}

#[test]
fn wakeup_zero_is_observed_but_invalid_or_missing_data_is_unknown() {
    let fixture = Fixture::new();
    for (text, expected) in [
        ("0\n", Some(0)),
        ("42\n", Some(42)),
        ("-1\n", None),
        ("bad\n", None),
    ] {
        fixture.put("/sys/power/wakeup_count", text);
        let answer = wakeup_answer(&fixture);
        let result = cpu(&answering(&fixture, vec![answer])).unwrap();
        assert_eq!(result["wakeup_count"], json!(expected), "{text}");
        assert_eq!(
            result["wakeup_count_reason"],
            if expected.is_some() {
                Value::Null
            } else {
                json!("counter_unavailable")
            }
        );
    }
    fixture.remove("/sys/power/wakeup_count");
    let answer = wakeup_answer(&fixture);
    let result = cpu(&answering(&fixture, vec![answer])).unwrap();
    assert!(result["wakeup_count"].is_null());
    assert_eq!(result["wakeup_count_reason"], "command_failed");
}

// ---- CPU Final and idle completion diagnostics -------------------------------------------------

#[test]
fn broadcast_and_dormant_abort_observations_are_explicit() {
    let fixture = Fixture::new();
    assert!(cpu(&unanswered(&fixture)).unwrap()["timer"]["broadcast_clockevent"].is_null());
    fixture
        .put(
            "/sys/devices/system/clockevents/broadcast/current_device",
            "y2-gpt4-broadcast",
        )
        .put("/sys/module/idle/parameters/dormant_aborts", "7")
        .put("/sys/module/idle/parameters/last_error", "-62");
    let result = cpu(&unanswered(&fixture)).unwrap();
    assert_eq!(result["timer"]["broadcast_clockevent"], "y2-gpt4-broadcast");
    assert_eq!(result["idle_diagnostics"]["dormant_aborts"], "7");
    assert_eq!(result["idle_diagnostics"]["last_error"], "-62");
    assert!(result["timer"]["highres_active"].is_null());
}

#[test]
fn diagnostics_do_not_fabricate_absent_states_or_mask_owners() {
    let fixture = Fixture::new();
    let result = serde_json::to_value(idle_completion(&unanswered(&fixture), &[])).unwrap();
    assert_eq!(result["C1"]["registered"], false);
    assert!(result["C1"]["entries"].is_null());
    assert!(result["C2"]["blocker_owners"].is_null());
    assert!(result["C3"]["unused_clock_handoff"].is_null());
    assert!(result["C3"]["uart_clock_handoff"].is_null());
    assert_eq!(result["C3"]["uart_sleep"], json!({}));
    for (path, value) in [
        ("/sys/module/clocks/parameters/slow_blockers", ((1 << 11) | (1 << 12)).to_string()),
        ("/sys/bus/platform/devices/18070000.connectivity/power/runtime_status", "active".into()),
        ("/sys/bus/platform/devices/11230000.mmc/power/runtime_status", "suspended".into()),
        (
            "/sys/devices/platform/10006000.power-controller/dormant_preflight",
            "peri_blockers=0x400 infra_blockers=0x80 disp0=0x400 disp1=0x8 prerequisite_topology=1 prerequisite_frequency=0 unmet=frequency,".into(),
        ),
        (
            "/sys/module/clocks/parameters/unused_handoff",
            "UART1 sampled=1 lcr=3 ier=0 lsr=0x60 dma=4 reason=quiet retained=0\nSPI0 sampled=1 command=0 status1=1 reason=quiet retained=0".into(),
        ),
        (
            "/sys/devices/platform/10006000.power-controller/state",
            "dormant_entries=0 uart_sleep_attempts=1 uart_sleep_successes=0 uart_sleep_ack=0 uart_sleep_timeouts=1 uart_sleep_rejection=UART_BUSY uart_power_before=0x15820 uart_power_after=0x15820 uart_r13_ack=0".into(),
        ),
        (
            "/sys/module/clocks/parameters/uart_sleep_handoff",
            "uart_sleep_owner_ready=1 uart1_deferred=0x20000 uart1_gate_before=0 uart1_gate_after=0".into(),
        ),
    ] {
        fixture.put(path, &value);
    }
    let row = cpu::IdleState {
        cpu: "cpu0".into(),
        state: "state0".into(),
        name: Some("WFI".into()),
        time_us: Some(1000),
        usage: Some(10),
        exit_latency_us: None,
        target_residency_us: None,
        rejected: Some(0),
        disabled: Some(0),
    };
    let result = serde_json::to_value(idle_completion(&unanswered(&fixture), &[row])).unwrap();
    assert_eq!(result["C1"]["enabled"], true);
    assert_eq!(result["C1"]["residency_us"], 1000);
    let owners = |label: &str, key: &str| -> Vec<Value> {
        result[label]["blocker_owners"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x[key].clone())
            .collect()
    };
    assert_eq!(owners("C2", "clock"), [json!("APDMA"), json!("MSDC0")]);
    assert_eq!(
        owners("C2", "runtime_status"),
        [json!("active"), json!("suspended")]
    );
    assert_eq!(result["C3"]["unmet"], json!(["frequency"]));
    assert!(result["C3"]["unused_clock_handoff"]
        .as_str()
        .unwrap()
        .contains("dma=4 reason=quiet"));
    assert_eq!(
        owners("C3", "clock"),
        ["USB0", "L2C_SRAM", "MDP_WROT", "DPI_ENGINE"].map(|c| json!(c))
    );
    assert_eq!(
        result["C3"]["uart_sleep"]["uart_sleep_rejection"],
        "UART_BUSY"
    );
    assert_eq!(result["C3"]["uart_sleep"]["uart_sleep_ack"], "0");
    assert!(result["C3"]["uart_clock_handoff"]
        .as_str()
        .unwrap()
        .contains("uart1_gate_after=0"));
}
