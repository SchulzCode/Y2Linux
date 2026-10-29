//! The Wi-Fi power-save policy: what tests/test_radio_policy.py checked of the Python platform
//! (radio_policy.wifi_policy). The fixture's `iw` answers are fixed per call, so cfg80211's flag is
//! given as it reads before and after the set.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::{failed, ok, Fixture};

const GET: [&str; 5] = ["/usr/sbin/iw", "dev", "wlan0", "get", "power_save"];

fn fixture() -> Fixture {
    let fixture = Fixture::new();
    fixture.mkdir("/run/y2");
    fixture
}

/// `wifi_policy(root, activity)` with cfg80211's flag reading `enabled`; the record and whether a
/// set was sent.
fn policy(fixture: &Fixture, enabled: bool, activity: Option<Value>) -> (Object, bool) {
    let state = if enabled { "on" } else { "off" };
    let answers = vec![
        ok(&GET, &format!("Power save: {state}")),
        ok(
            &["/usr/sbin/iw", "dev", "wlan0", "set", "power_save", "on"],
            "",
        ),
        ok(
            &["/usr/sbin/iw", "dev", "wlan0", "set", "power_save", "off"],
            "",
        ),
    ];
    let activity = activity.map(|a| a.as_object().cloned().unwrap());
    let root = fixture.root.clone().with_answers(answers);
    let value = wifi_policy(&root, activity.as_ref()).unwrap();
    let set = root
        .calls()
        .iter()
        .any(|(argv, _)| argv.iter().any(|a| a == "set"));
    (value, set)
}

fn applied(value: &Object) -> bool {
    value.get("applied") == Some(&json!(true))
}

#[test]
fn standard_ps_has_readback_and_failure_is_not_success() {
    let fixture = fixture();
    fixture.put_json(
        "/data/network/power-save.json",
        &json!({"schema": 1, "mode": "standard"}),
    );
    let (value, _) = policy(&fixture, true, None);
    assert_eq!(value.get("enabled"), Some(&json!(true)));
    assert_eq!(value.get("state"), Some(&json!("Ready")));
    let root = fixture
        .root
        .clone()
        .with_answers(vec![failed(&GET, "timeout")]);
    let value = wifi_policy(&root, None).unwrap();
    assert_eq!(value.get("enabled"), Some(&Value::Null));
}

#[test]
fn set_only_on_change_and_reestablish_after_epoch_or_interface_change() {
    let fixture = fixture();
    fixture.put_json(
        "/data/network/power-save.json",
        &json!({"schema": 1, "mode": "standard"}),
    );
    assert!(applied(&policy(&fixture, true, None).0));
    let (value, set) = policy(&fixture, true, None);
    assert!(!applied(&value));
    assert_eq!(
        value.get("requested_driver_profile"),
        Some(&json!("FAST_PSP"))
    );
    assert!(!set);
    // An external change, new association or interface recreation requires a fresh firmware command
    // even when the cfg80211 flag already agrees.
    assert!(applied(&policy(&fixture, false, None).0));
    // (The fixture's flag still read off after that set: established again, then left alone.)
    assert!(applied(&policy(&fixture, true, None).0));
    assert!(!applied(&policy(&fixture, true, None).0));
    fixture.put("/run/y2/wifi-epoch", "new-lease");
    assert!(applied(&policy(&fixture, true, None).0));
    assert!(!applied(&policy(&fixture, true, None).0));
    fixture.put("/sys/class/net/wlan0/ifindex", "5");
    assert!(applied(&policy(&fixture, true, None).0));
    assert!(!applied(&policy(&fixture, true, None).0));
}

#[test]
fn automatic_ps_conservative_default_and_heavy_transfer_hook() {
    // The Coexistence half (the heavy-transfer observation) is the service daemon's
    // (service/daemon_connectivity_tests.rs).
    let fixture = fixture();
    assert_eq!(
        policy(&fixture, false, None).0.get("enabled"),
        Some(&json!(false))
    );
    fixture.put_json(
        "/data/network/power-save.json",
        &json!({"schema": 1, "mode": "automatic", "automatic_standard": true}),
    );
    let (value, _) = policy(&fixture, true, None);
    assert_eq!(value.get("requested"), Some(&json!(true)));
    assert_eq!(value.get("enabled"), Some(&json!(true)));
    let (value, _) = policy(&fixture, false, Some(json!({"wifi_heavy_transfer": true})));
    assert_eq!(value.get("requested"), Some(&json!(false)));
    assert_eq!(value.get("enabled"), Some(&json!(false)));
}
