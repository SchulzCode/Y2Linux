//! Wi-Fi readiness and the RSSI cache (`observe.wifi`): what tests/test_platform_network_time.py
//! checked of the Python platform.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::{ok, scripted, Fixture};

const STATUS: [&str; 4] = ["wpa_cli", "-i", "wlan0", "status"];
const SIGNAL: [&str; 4] = ["wpa_cli", "-i", "wlan0", "signal_poll"];

/// wlan0 associated with `peer` at 192.0.2.2 with a default route, lease epoch `current`.
fn associated() -> Fixture {
    let fixture = Fixture::new();
    fixture
        .put("/proc/sys/kernel/random/boot_id", "boot-one")
        .put("/sys/class/net/wlan0/ifindex", "4")
        .put("/run/y2/wifi-epoch", "current");
    fixture
}

/// `wifi(root, cache)` with wpa_cli and ip answering for `peer`, and every request (argv, timeout).
fn look(
    fixture: &Fixture,
    peer: &str,
    cache: Option<&Ordered>,
) -> (Ordered, Vec<(Vec<String>, f64)>) {
    let answers = vec![
        ok(&STATUS, &format!("wpa_state=COMPLETED\nbssid={peer}")),
        ok(
            &["ip", "-j", "-4", "addr", "show", "dev", "wlan0"],
            r#"[{"addr_info":[{"local":"192.0.2.2","scope":"global"}]}]"#,
        ),
        ok(
            &["ip", "-j", "-4", "route", "show", "default", "dev", "wlan0"],
            r#"[{"dst":"default"}]"#,
        ),
        ok(&SIGNAL, "RSSI=-50"),
    ];
    scripted(answers, || wifi(&fixture.root, cache))
}

fn polled(calls: &[(Vec<String>, f64)]) -> bool {
    calls.iter().any(|(argv, _)| argv[..] == SIGNAL)
}

fn plain(value: &Ordered) -> Value {
    serde_json::to_value(value).unwrap()
}

#[test]
fn dns_readiness_rejects_old_lease_same_address_and_peer() {
    let fixture = associated();
    let mut dns = json!({"ok": true, "boot_id": "boot-one", "monotonic_s": clock::monotonic(),
                         "address": "192.0.2.2", "bssid": "one", "epoch": "old"});
    fixture.put_json("/run/y2/dns.json", &dns);
    assert_eq!(
        look(&fixture, "one", None).0.get("state"),
        Some(&json!("Authenticated"))
    );
    dns["epoch"] = json!("current");
    fixture.put_json("/run/y2/dns.json", &dns);
    assert_eq!(
        look(&fixture, "one", None).0.get("state"),
        Some(&json!("Online"))
    );
}

#[test]
fn rssi_cache_is_bounded_and_invalidates_on_peer_or_boot_change() {
    let fixture = associated();
    let (first, _) = look(&fixture, "one", None);
    assert_eq!(first.get("rssi_dbm"), Some(&json!(-50)));
    let (cached, calls) = look(&fixture, "one", Some(&first));
    assert_eq!(cached.get("rssi_dbm"), Some(&json!(-50)));
    assert!(!polled(&calls));
    let mut record = plain(&first);
    fixture.put_json("/run/y2/network.json", &record);
    let (published, calls) = look(&fixture, "one", None);
    assert_eq!(published.get("rssi_dbm"), Some(&json!(-50)));
    assert!(!polled(&calls));
    let status = calls.iter().find(|(argv, _)| argv[..] == STATUS).unwrap();
    assert_eq!(status.1, 3.0);
    let at = record["signal_observation"]["monotonic_s"]
        .as_f64()
        .unwrap();
    record["signal_observation"]["monotonic_s"] = json!(at - 31.0);
    fixture.put_json("/run/y2/network.json", &record);
    assert!(polled(&look(&fixture, "one", None).1));
    record["signal_observation"]["monotonic_s"] = json!(clock::monotonic());
    fixture.put_json("/run/y2/network.json", &record);
    assert!(polled(&look(&fixture, "two", None).1));
    fixture.put("/proc/sys/kernel/random/boot_id", "new-boot");
    assert!(polled(&look(&fixture, "two", None).1));
}
