//! Wi-Fi link events (`wifi-event`, network.lease_event): what tests/test_platform_network_time.py
//! checked of the Python platform. `dhcp-event` reads udhcpc's environment and is tested on the command
//! line (tests/connectivity.rs).
// SPDX-License-Identifier: GPL-2.0-only
use super::wifi_event;
use crate::testing::Fixture;
use serde_json::{json, Value};

fn event(fixture: &Fixture, name: &str) -> Value {
    let reply = wifi_event(&fixture.root, name).expect("event");
    serde_json::from_str(&reply.stdout).unwrap()
}

fn player() -> Fixture {
    let fixture = Fixture::new();
    fixture.put("/proc/sys/kernel/random/boot_id", "boot-one");
    fixture
}

#[test]
fn cleanup_disconnect_is_not_counted_as_another_link_loss() {
    let fixture = player();
    assert_eq!(event(&fixture, "DISCONNECTED")["state"], "Unavailable");
    assert_eq!(
        fixture.json("/run/y2/wifi-events.json")["real_disconnects"],
        0
    );
    event(&fixture, "CONNECTED");
    event(&fixture, "DISCONNECTED");
    event(&fixture, "DISCONNECTED"); // interface-removal cleanup duplicates the action
    let events = fixture.json("/run/y2/wifi-events.json");
    assert_eq!(events["disconnected"], 3); // raw action API stays compatible
    assert_eq!(events["real_disconnects"], 1);
    event(&fixture, "CONNECTED");
    assert_eq!(fixture.json("/run/y2/wifi-events.json")["reconnects"], 1);
    event(&fixture, "CONNECTED"); // repeated notification is not another reconnect
    assert_eq!(fixture.json("/run/y2/wifi-events.json")["reconnects"], 1);
    fixture.put("/proc/sys/kernel/random/boot_id", "next-boot");
    event(&fixture, "CONNECTED");
    let events = fixture.json("/run/y2/wifi-events.json");
    assert_eq!(events["connected"], 1);
    assert_eq!(events["real_disconnects"], 0);
    assert_eq!(events["reconnects"], 0);
}

#[test]
fn sleep_radio_off_is_not_a_real_link_loss() {
    let fixture = player();
    fixture
        .put("/sys/class/rfkill/rfkill0/type", "bluetooth")
        .put("/sys/class/rfkill/rfkill0/soft", "1")
        .put("/sys/class/rfkill/rfkill1/type", "wlan")
        .put("/sys/class/rfkill/rfkill1/soft", "1");
    let counts = |fixture: &Fixture| {
        let events = fixture.json("/run/y2/wifi-events.json");
        json!([events["real_disconnects"], events["radio_off_disconnects"]])
    };
    event(&fixture, "CONNECTED");
    event(&fixture, "DISCONNECTED");
    assert_eq!(counts(&fixture), json!([0, 1]));
    fixture.put("/sys/class/rfkill/rfkill1/soft", "0");
    event(&fixture, "CONNECTED");
    event(&fixture, "DISCONNECTED");
    assert_eq!(counts(&fixture), json!([1, 1]));
}
