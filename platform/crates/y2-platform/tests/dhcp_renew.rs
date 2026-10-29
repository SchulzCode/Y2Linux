//! A DHCP renew of the same lease changes nothing on wlan0 and keeps resolv.conf and the DNS probe
//! result; a changed lease or a new bind configures wlan0 again and drops the DNS result.
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
            "192.168.1.40/16",
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
        &[
            "ip",
            "-4",
            "route",
            "replace",
            "default",
            "via",
            "192.168.1.254",
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

/// `dhcp-event <event>` for a lease on `ip`/24 via 192.168.1.1 with these DNS servers; `changes`
/// replace udhcpc's variables.
fn lease(
    fixture: &Fixture,
    event: &str,
    ip: &str,
    dns: &str,
    changes: &[(&str, &str)],
) -> common::Run {
    let mut env = vec![
        ("interface", "wlan0"),
        ("Y2_DHCP_GENERATION", EPOCH),
        ("ip", ip),
        ("subnet", "255.255.255.0"),
        ("router", "192.168.1.1"),
        ("dns", dns),
        ("lease", "86400"),
    ];
    for (name, value) in changes {
        env.retain(|(n, _)| n != name);
        env.push((name, value));
    }
    fixture.run_as(None, &["dhcp-event", event], &env)
}

fn event(fixture: &Fixture, event: &str, ip: &str, dns: &str) -> common::Run {
    lease(fixture, event, ip, dns, &[])
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
fn a_renew_of_the_same_lease_changes_nothing_on_wlan0() {
    let fixture = player();
    let bound = event(&fixture, "bound", "192.168.1.40", "192.168.1.1 9.9.9.9");
    assert_eq!(bound.code, Some(0), "{}", bound.stdout);
    assert_eq!(
        bound.argvs().len(),
        5,
        "flush twice, link up, address, route"
    );
    let published = bound.json();
    assert_eq!(published["prefix_length"], 24);
    assert_eq!(
        published["routes"],
        serde_json::json!([["default", "192.168.1.1"]])
    );
    // A stale resolver file would be written again; the same lease leaves it as it is.
    fixture.put_mode("/run/y2/resolv.conf", "kept\n", 0o644);
    fixture.put_mode("/run/y2/dns.json", DNS, 0o600);
    let renew = lease(
        &fixture,
        "renew",
        "192.168.1.40",
        "192.168.1.1 9.9.9.9",
        &[("lease", "43200")],
    );
    assert_eq!(renew.code, Some(0), "{}", renew.stdout);
    assert!(renew.calls.is_empty(), "{:?}", renew.argvs());
    assert_eq!(fixture.read("/run/y2/resolv.conf"), "kept\n");
    assert_eq!(fixture.read("/run/y2/dns.json"), DNS);
    // dhcp.json is published again with the renewed lease time.
    let record = fixture.json("/run/y2/dhcp.json");
    assert_eq!(record["state"], "Authenticated");
    assert_eq!(record["address"], "192.168.1.40");
    assert_eq!(record["lease_seconds"], "43200");
    assert_eq!(renew.json()["lease_seconds"], "43200");
}

#[test]
fn a_renew_with_another_prefix_route_or_server_configures_wlan0_again() {
    for (changes, what) in [
        (&[("subnet", "255.255.0.0")][..], "another prefix"),
        (&[("router", "192.168.1.254")][..], "another route"),
        (&[("dns", "9.9.9.9")][..], "other DNS servers"),
    ] {
        let fixture = player();
        let bound = event(&fixture, "bound", "192.168.1.40", "192.168.1.1 9.9.9.9");
        assert_eq!(bound.code, Some(0), "{}", bound.stdout);
        fixture.put_mode("/run/y2/resolv.conf", "old\n", 0o644);
        let renew = lease(
            &fixture,
            "renew",
            "192.168.1.40",
            "192.168.1.1 9.9.9.9",
            changes,
        );
        assert_eq!(renew.code, Some(0), "{what}: {}", renew.stdout);
        let argvs = renew.argvs();
        assert_eq!(argvs.len(), 5, "{what}: {argvs:?}");
        assert_eq!(argvs[0][..4], ["ip", "-4", "address", "flush"], "{what}");
        assert_ne!(fixture.read("/run/y2/resolv.conf"), "old\n", "{what}");
    }
    // A record from before the lease carried its prefix and routes is configured again too.
    let fixture = player();
    fixture.put_mode(
        "/run/y2/dhcp.json",
        &format!(
            r#"{{"address":"192.168.1.40","boot_id":"boot-a","dns_servers":["192.168.1.1"],"epoch":"{EPOCH}","state":"Authenticated"}}"#
        ),
        0o600,
    );
    let renew = event(&fixture, "renew", "192.168.1.40", "192.168.1.1");
    assert_eq!(renew.argvs().len(), 5, "{:?}", renew.argvs());
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
