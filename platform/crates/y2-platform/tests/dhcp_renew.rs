//! A DHCP renew of the same lease keeps the DNS probe result; a changed lease or a new bind drops it.
// SPDX-License-Identifier: GPL-2.0-only
mod common;
use common::Fixture;

const EPOCH: &str = "b6a2f0de-6a1c-4b7e-9c0e-3f1d2a4b5c6d";
const DNS: &str =
    r#"{"epoch":"b6a2f0de-6a1c-4b7e-9c0e-3f1d2a4b5c6d","ok":true,"address":"192.168.1.40"}"#;

fn player() -> Fixture {
    let mut fixture = Fixture::new();
    fixture
        .put("/proc/sys/kernel/random/boot_id", "boot-a\n")
        .put_mode("/run/y2/wifi-epoch", &format!("{EPOCH}\n"), 0o600);
    for argv in [
        &[
            "ip", "-4", "address", "flush", "dev", "wlan0", "scope", "global",
        ][..],
        &["ip", "-4", "route", "flush", "dev", "wlan0"],
        &["ip", "link", "set", "dev", "wlan0", "up"],
        &[
            "ip",
            "-4",
            "address",
            "add",
            "192.168.1.40/24",
            "dev",
            "wlan0",
        ],
        &[
            "ip",
            "-4",
            "address",
            "add",
            "192.168.1.41/24",
            "dev",
            "wlan0",
        ],
        &[
            "ip",
            "-4",
            "route",
            "replace",
            "default",
            "via",
            "192.168.1.1",
            "dev",
            "wlan0",
            "metric",
            "100",
        ],
    ] {
        fixture.answer(argv, true, "");
    }
    fixture
}

fn event(fixture: &Fixture, event: &str, ip: &str, dns: &str) -> common::Run {
    let env = [
        ("interface", "wlan0"),
        ("Y2_DHCP_GENERATION", EPOCH),
        ("ip", ip),
        ("subnet", "255.255.255.0"),
        ("router", "192.168.1.1"),
        ("dns", dns),
        ("lease", "86400"),
    ];
    fixture.run_as(None, &["dhcp-event", event], &env)
}

#[test]
fn a_renew_of_the_same_lease_keeps_the_dns_result() {
    let fixture = player();
    assert_eq!(
        event(&fixture, "bound", "192.168.1.40", "192.168.1.1 9.9.9.9").code,
        Some(0)
    );
    fixture.put_mode("/run/y2/dns.json", DNS, 0o600);
    let renew = event(&fixture, "renew", "192.168.1.40", "192.168.1.1 9.9.9.9");
    assert_eq!(renew.code, Some(0), "{}", renew.stdout);
    assert_eq!(fixture.read("/run/y2/dns.json"), DNS);
}

#[test]
fn a_changed_lease_or_a_new_bind_drops_it() {
    let fixture = player();
    assert_eq!(
        event(&fixture, "bound", "192.168.1.40", "192.168.1.1").code,
        Some(0)
    );
    fixture.put_mode("/run/y2/dns.json", DNS, 0o600);
    assert_eq!(
        event(&fixture, "renew", "192.168.1.40", "9.9.9.9").code,
        Some(0)
    );
    assert!(!fixture.exists("/run/y2/dns.json"), "other DNS servers");
    fixture.put_mode("/run/y2/dns.json", DNS, 0o600);
    assert_eq!(
        event(&fixture, "renew", "192.168.1.41", "9.9.9.9").code,
        Some(0)
    );
    assert!(!fixture.exists("/run/y2/dns.json"), "another address");
    fixture.put_mode("/run/y2/dns.json", DNS, 0o600);
    assert_eq!(
        event(&fixture, "bound", "192.168.1.41", "9.9.9.9").code,
        Some(0)
    );
    assert!(!fixture.exists("/run/y2/dns.json"), "a new bind");
}
