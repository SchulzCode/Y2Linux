//! Radio lifecycle and restart bounds of the connectivity daemon's supervisor: what
//! tests/test_connectivity_supervisor.py checked of the Python platform (connectivity.py). The signal
//! wait of the whole daemon is tested on the command line (tests/connectivity.rs).
// SPDX-License-Identifier: GPL-2.0-only
use super::super::steps::Steps;
use super::*;
use crate::testing::{failed, ok, Fixture};
use y2p_core::command::Outcome;

type Calls = Vec<Vec<String>>;

/// A fixture whose root's requests the test answers per step.
fn fixture() -> (Fixture, Steps) {
    let steps = Steps::default();
    let fixture = steps.attach(Fixture::new());
    fixture.mkdir("/run/y2").mkdir("/sys/class/net");
    (fixture, steps)
}

/// Run `f` with these answers (anything else fails); the requests' argv.
fn run<T>(
    steps: &Steps,
    answers: Vec<(Vec<String>, Outcome)>,
    f: impl FnOnce() -> T,
) -> (T, Calls) {
    let (value, calls) = steps.run(answers, f);
    (value, calls.into_iter().map(|(argv, _)| argv).collect())
}

const ADD: [&str; 8] = [
    "wpa_cli",
    "-g",
    GLOBAL,
    "interface_add",
    "wlan0",
    "/data/network/wpa_supplicant.conf",
    "nl80211",
    "/run/wpa_supplicant",
];
const LIST: [&str; 4] = ["wpa_cli", "-g", GLOBAL, "interface_list"];
const AUTOSCAN_SET: [&str; 5] = ["wpa_cli", "-i", "wlan0", "autoscan", AUTOSCAN];
const ACTION: [&str; 8] = [
    "wpa_cli",
    "-i",
    "wlan0",
    "-a",
    "/usr/libexec/y2/wpa-event",
    "-B",
    "-P",
    "/run/y2/wifi-action.pid",
];

/// The answers of a supplicant that adds the interface (Python's fixture answered `OK` to all).
fn adding() -> Vec<(Vec<String>, Outcome)> {
    vec![ok(&ADD, "OK"), ok(&AUTOSCAN_SET, "OK"), ok(&ACTION, "OK")]
}

fn words(argv: &[&str]) -> Vec<String> {
    argv.iter().map(|w| w.to_string()).collect()
}

/// Where `/proc/<pid>/exe` of a helper points: its binary below the (resolved) fixture root.
fn binary(fixture: &Fixture, path: &str) -> PathBuf {
    fs::canonicalize(fixture.base())
        .unwrap()
        .join(path.trim_start_matches('/'))
}

/// Live processes standing in for the helpers, killed on drop.
struct Helpers(Vec<std::process::Child>);

impl Drop for Helpers {
    fn drop(&mut self) {
        for child in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn healthy_radio_off_uses_native_reads_and_no_commands() {
    let (fixture, steps) = fixture();
    let mut helpers = Helpers(Vec::new());
    for (name, path) in HELPERS {
        let child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .unwrap();
        let pid = child.id();
        helpers.0.push(child);
        fixture.put(&format!("/run/y2/{name}.pid"), &pid.to_string());
        fixture.mkdir(&format!("/proc/{pid}"));
        std::os::unix::fs::symlink(
            binary(&fixture, path),
            fixture.path(&format!("/proc/{pid}/exe")),
        )
        .unwrap();
    }
    let mut monitor = Supervisor::new(&fixture.root);
    let (_, calls) = run(&steps, Vec::new(), || {
        monitor.health(0.0, false).unwrap();
        monitor.interface(0.0);
        monitor.health(20.0, false).unwrap();
        monitor.health(30.0, false).unwrap();
    });
    assert!(calls.is_empty(), "{calls:?}");
    assert_eq!(monitor.pids.len(), 4);
    assert!(!monitor.attached);
}

#[test]
fn missing_helper_crash_events_cannot_bypass_restart_rate() {
    let (fixture, steps) = fixture();
    // BlueALSA's arguments can be made (else it is not started at all).
    fixture.put("/etc/y2linux/bluetooth-codecs.json", r#"{"schema": 1}"#);
    let mut monitor = Supervisor::new(&fixture.root);
    // Each helper's restart starts it once: start-stop-daemon -S, or wpa_supplicant itself.
    let restarts = |calls: &Calls| {
        calls
            .iter()
            .filter(|argv| argv[0] == "wpa_supplicant" || argv[..2] == ["start-stop-daemon", "-S"])
            .count()
    };
    let (_, calls) = run(&steps, Vec::new(), || monitor.health(0.0, false).unwrap());
    assert_eq!(restarts(&calls), 4);
    let (_, calls) = run(&steps, Vec::new(), || {
        monitor.health(1.0, true).unwrap();
        monitor.health(29.0, true).unwrap();
    });
    assert_eq!(restarts(&calls), 0);
    let (_, calls) = run(&steps, Vec::new(), || monitor.health(30.0, true).unwrap());
    assert_eq!(restarts(&calls), 4);
}

#[test]
fn stale_reused_pid_is_not_a_healthy_helper() {
    let (fixture, _) = fixture();
    let (name, path) = HELPERS[1];
    assert_eq!(name, "wpa");
    let pid = std::process::id();
    fixture.put("/run/y2/wpa.pid", &pid.to_string());
    fixture.mkdir(&format!("/proc/{pid}"));
    let exe = fixture.path(&format!("/proc/{pid}/exe"));
    std::os::unix::fs::symlink("/different-program", &exe).unwrap();
    assert_eq!(alive(&fixture.root, name, path), None);
    fs::remove_file(&exe).unwrap();
    std::os::unix::fs::symlink(binary(&fixture, path), &exe).unwrap();
    assert_eq!(alive(&fixture.root, name, path), Some(pid as i32));
}

#[test]
fn warm_restart_reuses_supplicant_interface_and_action_hook() {
    let (fixture, steps) = fixture();
    fixture.mkdir("/sys/class/net/wlan0");
    let mut monitor = Supervisor::new(&fixture.root);
    let answers = vec![
        failed(&ADD, "command_failed"),
        ok(&LIST, "wlan0"),
        ok(&AUTOSCAN_SET, "OK"),
        ok(&ACTION, ""),
    ];
    let (_, calls) = run(&steps, answers, || monitor.interface(0.0));
    assert!(monitor.attached);
    // A missing AP must back off its scans instead of rescanning every ~5 s.
    assert_eq!(
        calls[calls.len() - 2],
        words(&["wpa_cli", "-i", "wlan0", "autoscan", "exponential:2:120"])
    );
    assert_eq!(calls[calls.len() - 1], words(&ACTION));
    let (_, more) = run(&steps, Vec::new(), || monitor.interface(60.0));
    assert_eq!(calls.len() + more.len(), 4);
}

#[test]
fn netdev_removal_runs_disconnect_cleanup_once() {
    let (fixture, steps) = fixture();
    let mut monitor = Supervisor::new(&fixture.root);
    monitor.attached = true;
    let (_, calls) = run(&steps, Vec::new(), || {
        monitor.interface(0.0);
        monitor.interface(30.0);
    });
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls[0],
        words(&["/usr/libexec/y2/wpa-event", "wlan0", "DISCONNECTED"])
    );
    assert_eq!(
        calls[2][calls[2].len() - 2..],
        ["interface_remove", "wlan0"]
    );
    assert!(!monitor.attached);
}

#[test]
fn a_new_netdev_under_the_old_name_gets_a_new_supplicant_interface() {
    // F53: a removal missed between two looks left the supplicant's old interface on the new wlan0
    // (INTERFACE_DISABLED for 106 min).
    let (fixture, steps) = fixture();
    fixture.put("/sys/class/net/wlan0/ifindex", "5\n");
    let mut monitor = Supervisor::new(&fixture.root);
    run(&steps, adding(), || monitor.interface(0.0));
    assert_eq!(
        (monitor.attached, monitor.ifindex.as_deref()),
        (true, Some("5"))
    );
    let (_, calls) = run(&steps, adding(), || monitor.interface(10.0));
    assert!(calls.is_empty());
    fixture.put("/sys/class/net/wlan0/ifindex", "7\n");
    let (_, calls) = run(&steps, adding(), || monitor.interface(20.0));
    assert_eq!(
        calls[0],
        words(&["/usr/libexec/y2/wpa-event", "wlan0", "DISCONNECTED"])
    );
    assert_eq!(
        calls[2][calls[2].len() - 2..],
        ["interface_remove", "wlan0"]
    );
    assert_eq!(calls[3][3..5], ["interface_add", "wlan0"]);
    assert_eq!(
        (monitor.attached, monitor.ifindex.as_deref()),
        (true, Some("7"))
    );
}

#[test]
fn interface_left_disabled_with_radio_on_is_added_again() {
    let (fixture, steps) = fixture();
    fixture
        .put("/proc/sys/kernel/random/boot_id", "boot-one")
        .put("/sys/class/rfkill/rfkill1/type", "wlan\n")
        .put("/sys/class/rfkill/rfkill1/soft", "0\n");
    let network = |association: &str, age: f64| {
        fixture.put_json(
            "/run/y2/network.json",
            &json!({"boot_id": "boot-one", "monotonic_s": clock::monotonic() - age,
                    "association": association, "radio_present": true}),
        );
    };
    network("INTERFACE_DISABLED", 0.0);
    let mut monitor = Supervisor::new(&fixture.root);
    monitor.attached = true;
    let (_, calls) = run(&steps, Vec::new(), || {
        monitor.verify(100.0);
        monitor.verify(129.0);
    });
    assert!(calls.is_empty());
    let (_, calls) = run(&steps, Vec::new(), || monitor.verify(130.0));
    let last = calls.last().unwrap();
    assert_eq!(last[last.len() - 2..], ["interface_remove", "wlan0"]);
    assert!(!monitor.attached);
    // Backed off when the new interface is disabled again.
    monitor.attached = true;
    let (_, calls) = run(&steps, Vec::new(), || {
        monitor.verify(131.0);
        monitor.verify(161.0);
    });
    assert!(calls.is_empty());
    let (_, calls) = run(&steps, Vec::new(), || monitor.verify(190.0));
    assert_eq!(calls.len(), 3);
    // A radio switched off (sleep, the owner) or a stale record is no reason.
    monitor.attached = true;
    fixture.put("/sys/class/rfkill/rfkill1/soft", "1\n");
    let (_, calls) = run(&steps, Vec::new(), || {
        for now in [1000.0, 1100.0, 1200.0] {
            monitor.verify(now);
        }
    });
    assert!(calls.is_empty());
    fixture.put("/sys/class/rfkill/rfkill1/soft", "0\n");
    network("INTERFACE_DISABLED", 60.0);
    let (_, calls) = run(&steps, Vec::new(), || {
        for now in [1300.0, 1400.0] {
            monitor.verify(now);
        }
    });
    assert!(calls.is_empty());
    network("DISCONNECTED", 0.0);
    let (_, calls) = run(&steps, Vec::new(), || {
        for now in [1500.0, 1600.0] {
            monitor.verify(now);
        }
    });
    assert!(calls.is_empty());
}

#[test]
fn failed_interface_attach_has_bounded_backoff() {
    let (fixture, steps) = fixture();
    fixture.mkdir("/sys/class/net/wlan0");
    let mut monitor = Supervisor::new(&fixture.root);
    let answers = || {
        vec![
            failed(&ADD, "command_failed"),
            failed(&LIST, "command_failed"),
        ]
    };
    let (_, calls) = run(&steps, answers(), || {
        monitor.interface(0.0);
        monitor.interface(1.0);
    });
    assert_eq!(calls.len(), 2);
    let (_, more) = run(&steps, answers(), || monitor.interface(2.0));
    assert_eq!(calls.len() + more.len(), 4);
    run(&steps, answers(), || {
        for now in [6.0, 14.0, 30.0, 62.0, 122.0] {
            monitor.interface(now);
        }
    });
    assert_eq!(monitor.retry_delay, 60.0);
    assert_eq!(monitor.retry_at, 182.0);
}

#[test]
fn wpa_restart_preserves_hooks_and_reconnect_single_owner() {
    let (fixture, steps) = fixture();
    let mut monitor = Supervisor::new(&fixture.root);
    monitor.attached = true;
    let (_, calls) = run(&steps, Vec::new(), || {
        monitor.restart("wpa").unwrap();
        assert!(!monitor.attached);
        monitor.restart("reconnect").unwrap();
    });
    assert!(calls.contains(&words(&[
        "/usr/sbin/y2-platform",
        "wifi-event",
        "SERVICE_RESTART"
    ])));
    assert!(calls
        .last()
        .unwrap()
        .contains(&"/usr/sbin/y2-bt-reconnect".to_string()));
    assert!(!calls
        .iter()
        .any(|argv| argv.iter().any(|a| a == "connect" || a == "bluetoothctl")));
}

#[test]
fn codec_policy_failure_does_not_tear_down_other_helpers() {
    let (fixture, steps) = fixture();
    // A policy codec_runtime refuses (ValueError).
    fixture.put(
        "/data/bluetooth/codec-policy.json",
        r#"{"schema": 1, "sbc_quality": "max"}"#,
    );
    let mut monitor = Supervisor::new(&fixture.root);
    let (restarted, calls) = run(&steps, Vec::new(), || monitor.restart("bluealsa"));
    assert!(restarted.is_ok());
    assert!(calls.is_empty());
}
