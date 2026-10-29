//! Which children one `service-daemon` tick starts (tests/test_platform_update.py,
//! test_radio_firmware.py: the patched Popen's argv): below a fixture root each is recorded as a
//! fixture call (timeout 0) and refused, never started. The decisions around them (a running child
//! kept, a hung one stopped, the back-off) are unit tests in src/commands/service/daemon_tests.rs.
// SPDX-License-Identifier: GPL-2.0-only
mod common;

use common::{Fixture, Run};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

const PLATFORM: &str = "/usr/sbin/y2-platform";

fn fixture() -> Fixture {
    let fixture = Fixture::new();
    fixture.put("/proc/sys/kernel/random/boot_id", "boot-a\n");
    fixture
}

fn tick(fixture: &Fixture) -> Run {
    let run = fixture.run_as(None, &["service-daemon"], &[("Y2_DAEMON_TICKS", "1")]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    run
}

/// Whether the tick started `y2-platform <args>`.
fn started(run: &Run, args: &[&str]) -> bool {
    let mut argv = vec![PLATFORM.to_string()];
    argv.extend(args.iter().map(|a| a.to_string()));
    run.calls
        .iter()
        .any(|c| c["timeout"] == json!(0) && c["argv"] == json!(argv))
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

#[test]
fn media_reconcile_runs_in_the_background_and_stops_a_hung_check() {
    // Needed (no lifecycle yet): `media reconcile` in its own process.
    let fixture = fixture();
    assert!(started(&tick(&fixture), &["media", "reconcile"]));
    // The lifecycle names this inventory (no card): nothing to start.
    let fixture = self::fixture();
    fixture.put(
        "/run/y2/media-lifecycle.json",
        r#"{"inventory": [], "retry": false}"#,
    );
    assert!(!started(&tick(&fixture), &["media", "reconcile"]));
}

#[test]
fn media_verify_runs_one_background_check_while_a_card_is_checking() {
    let fixture = fixture();
    assert!(!started(&tick(&fixture), &["media", "verify"]));
    fixture.put(
        "/run/y2/media-mount.json",
        r#"{"read_only": "checking", "boot_id": "boot-a"}"#,
    );
    assert!(started(&tick(&fixture), &["media", "verify"]));
}

/// Wi-Fi online: the supplicant associated, an address and a default route on wlan0, the DNS probe
/// answering (tests/platform_cases/service.py `wifi_online`, `WLAN`).
fn online(fixture: &mut Fixture) {
    for (name, text) in [
        ("/sys/class/net/wlan0/ifindex", "3\n"),
        ("/sys/class/net/wlan0/statistics/rx_bytes", "1000\n"),
        ("/sys/class/net/wlan0/statistics/tx_bytes", "500\n"),
        ("/data/network/enabled", "1\n"),
        ("/run/y2/wifi-epoch", "1\n"),
    ] {
        fixture.put(name, text);
    }
    fixture
        .answer(
            &["wpa_cli", "-i", "wlan0", "status"],
            true,
            "wpa_state=COMPLETED\nbssid=00:11:22:33:44:55\nid=0\n",
        )
        .answer(
            &["ip", "-j", "-4", "addr", "show", "dev", "wlan0"],
            true,
            r#"[{"addr_info": [{"local": "192.168.1.40", "scope": "global"}]}]"#,
        )
        .answer(
            &["ip", "-j", "-4", "route", "show", "default", "dev", "wlan0"],
            true,
            r#"[{"dst": "default", "gateway": "192.168.1.1"}]"#,
        )
        .answer(
            &["wpa_cli", "-i", "wlan0", "signal_poll"],
            true,
            "RSSI=-52\nLINKSPEED=72\n",
        )
        .answer(&["getaddrinfo", "pool.ntp.org"], true, "2")
        .answer(
            &["/usr/sbin/iw", "dev", "wlan0", "get", "power_save"],
            true,
            "Power save: off",
        );
}

#[test]
fn service_spawns_the_check_only_when_due() {
    let auto_check = ["update", "auto-check"];
    // Not online: no check.
    assert!(!started(&tick(&fixture()), &auto_check));
    let mut fixture = fixture();
    online(&mut fixture);
    let run = tick(&fixture);
    assert_eq!(
        fixture.json("/run/y2/network.json")["state"],
        json!("Online")
    );
    assert!(started(&run, &auto_check));
    // An attempt within the hour, a check that reached the server within the day: not due.
    fixture.put_json(
        "/data/updates/last-check.json",
        &json!({"attempt": now() - 60.0}),
    );
    assert!(!started(&tick(&fixture), &auto_check));
    fixture.put_json(
        "/data/updates/last-check.json",
        &json!({"attempt": now() - 7200.0, "time": now() - 7200.0, "result": "checked"}),
    );
    assert!(!started(&tick(&fixture), &auto_check));
    // Report ef22a23d: ≤ 8.14 stamped `time` on a failed check too; that one waits an hour like any
    // failed attempt, not a day.
    fixture.put_json(
        "/data/updates/last-check.json",
        &json!({"attempt": now() - 7200.0, "time": now() - 7200.0, "result": "failed",
                "failure": "[Errno 11] Resource temporarily unavailable"}),
    );
    assert!(started(&tick(&fixture), &auto_check));
}

#[test]
fn removed_firmware_is_noticed_at_the_next_maintenance_pass() {
    let scan = ["firmware", "scan"];
    // No firmware: the first (maintenance) pass looks for the owner's copy.
    let fixture = fixture();
    assert!(started(&tick(&fixture), &scan));
    // Every file there (present() looks at sizes only): nothing to do.
    for (name, size) in [
        ("WIFI_RAM_CODE_MT6582", 160368),
        ("WMT_SOC.cfg", 80),
        ("mt6572_82_patch_e1_0_hdr.bin", 33120),
        ("mt6572_82_patch_e1_1_hdr.bin", 17476),
        ("modem_1_2g_n.img", 2424376),
        ("WIFI.defaults", 512),
        ("BT.defaults", 64),
    ] {
        fixture.put(&format!("/data/firmware/mt6582/{name}"), &"x".repeat(size));
    }
    assert!(!started(&tick(&fixture), &scan));
    // Firmware in place and a copy waiting: the maintenance pass scans to delete it.
    fixture.put("/media/sd/Firmware/system.img", "x");
    assert!(started(&tick(&fixture), &scan));
}
