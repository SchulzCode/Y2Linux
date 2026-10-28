//! `network.lease_event` and `network.dhcp_event`: one DHCP lease lifecycle above wpa_supplicant
//! (`wifi-event` from wpa-event, `dhcp-event` from udhcpc's script with its environment).
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::{
    atomic_json, lock, private_directory, unlink_missing_ok, write_text, Failure, Outcome, Reply,
};
use serde_json::{json, Map, Value};
use std::{env, fs, os::unix::fs::PermissionsExt, path::Path};
use y2p_core::{
    clock, command,
    json::Ordered,
    py::{self, is_true, truthy},
    Root,
};

/// `publish(ctx, name, value)`: `value.update(schema, boot_id, monotonic_s)` and the runtime record.
fn publish(root: &Root, name: &str, mut value: Ordered) -> Result<Ordered, Failure> {
    private_directory(&root.path("/run/y2"))?;
    value
        .set("schema", 1)
        .set(
            "boot_id",
            json!(root.read("/proc/sys/kernel/random/boot_id")),
        )
        .set("monotonic_s", clock::monotonic());
    atomic_json(&root.path(&format!("/run/y2/{name}.json")), &value, false)?;
    Ok(value)
}

/// True while Wi-Fi is soft-blocked: sleep or the owner turned it off.
fn wlan_switched_off(root: &Root) -> bool {
    let mut entries: Vec<String> = fs::read_dir(root.path("/sys/class/rfkill"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("rfkill"))
        .collect();
    entries.sort();
    entries.iter().any(|name| {
        let base = format!("/sys/class/rfkill/{name}");
        root.read(&format!("{base}/type")).as_deref() == Some("wlan")
            && root.read(&format!("{base}/soft")).as_deref() == Some("1")
    })
}

/// `/etc/resolv.conf` is a production-owned symlink into /run; only Wi-Fi supplies DNS in this
/// product. Never import search strings from DHCP.
fn resolv(root: &Root, servers: &[String]) -> Result<(), Failure> {
    let path = root.path("/run/y2/resolv.conf");
    let id = py::uuid4()
        .map_err(|e| Failure::os(e, Path::new("/dev/urandom")))?
        .replace('-', "");
    let temporary = path.with_file_name(format!(".resolv.conf.{id}"));
    let text: String = servers
        .iter()
        .map(|s| format!("nameserver {s}\n"))
        .collect::<String>()
        + "options timeout:2 attempts:1\n";
    write_text(&temporary, &text)?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o644))
        .map_err(|e| Failure::os(e, &temporary))?;
    fs::rename(&temporary, &path).map_err(|e| Failure::os(e, &temporary))
}

const FLUSH: [&[&str]; 2] = [
    &[
        "ip", "-4", "address", "flush", "dev", "wlan0", "scope", "global",
    ],
    &["ip", "-4", "route", "flush", "dev", "wlan0"],
];

/// `isinstance(v, int) and v >= 0` (a bool too), else 0.
fn count(value: Option<&Value>) -> Value {
    match value {
        Some(v @ Value::Bool(_)) => v.clone(),
        Some(Value::Number(n)) if n.as_u64().is_some() => Value::Number(n.clone()),
        _ => json!(0),
    }
}

fn plus_one(value: &Value) -> Value {
    json!(py::number(Some(value)).unwrap_or(0.0) as u64 + 1)
}

/// `y2-platform wifi-event CONNECTED|DISCONNECTED|SERVICE_RESTART`.
pub fn wifi_event(root: &Root, event: &str) -> Outcome {
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    let _lock = lock(&runtime.join("dhcp.lock"))?;
    let epoch = py::uuid4().map_err(|e| Failure::os(e, Path::new("/dev/urandom")))?;
    private_directory(&runtime)?;
    write_text(&runtime.join("wifi-epoch"), &format!("{epoch}\n"))?;
    unlink_missing_ok(&runtime.join("dns.json"))?;
    let mut old = root.json_object("/run/y2/wifi-events.json");
    if old.get("boot_id").cloned().unwrap_or(Value::Null)
        != json!(root.read("/proc/sys/kernel/random/boot_id"))
    {
        old = Map::new();
    }
    let active = match old.get("association_active") {
        Some(value) => is_true(Some(value)),
        None => old.get("last_event").and_then(Value::as_str) == Some("CONNECTED"),
    };
    // Action calls include supervisory cleanup as well as native link events. Keep their historical
    // raw counts, but count a real loss only once after a known connection.
    let mut record = Ordered::from_map(&old);
    if record.get("link_counts_since_s").is_none() {
        record.set("link_counts_since_s", clock::monotonic());
    }
    for name in ["real_disconnects", "reconnects", "radio_off_disconnects"] {
        let value = count(record.get(name));
        record.set(name, value);
    }
    if event == "DISCONNECTED" && active {
        // Sleep switches the radio off: that is not a lost link (Beta 1 counted every sleep as a real
        // disconnect).
        let name = if wlan_switched_off(root) {
            "radio_off_disconnects"
        } else {
            "real_disconnects"
        };
        let value = plus_one(record.get(name).expect("set above"));
        record.set(name, value);
    }
    if event == "CONNECTED" && !active && record.get("connected").is_some_and(truthy) {
        let value = plus_one(record.get("reconnects").expect("set above"));
        record.set("reconnects", value);
    }
    record.set("association_active", event == "CONNECTED");
    let key = event.to_lowercase();
    let current = count(record.get(&key));
    record.set(&key, plus_one(&current));
    record.set("last_event", event);
    publish(root, "wifi-events", record)?;
    // Leases never survive a new association/service epoch. USB's fixed address and routes are outside
    // this interface-scoped lifecycle.
    for argv in FLUSH {
        command::run(root, argv, command::TIMEOUT);
    }
    resolv(root, &[])?;
    let state = Ordered::new()
        .with("epoch", epoch)
        .with(
            "state",
            if event == "CONNECTED" {
                "AcquiringIP"
            } else {
                "Unavailable"
            },
        )
        .with("error", Value::Null)
        .with("address", Value::Null);
    Reply::unsorted(&publish(root, "dhcp", state)?, 0)
}

/// `str(ipaddress.IPv4Address(text))`: four decimal octets, no leading zeros. Anything else is left to
/// Python, whose error text names the value.
fn ipv4(text: &str) -> Result<[u8; 4], Failure> {
    let parts: Vec<&str> = text.split('.').collect();
    if parts.len() != 4 {
        return Err(Failure::Fallback);
    }
    let mut octets = [0u8; 4];
    for (octet, part) in octets.iter_mut().zip(parts) {
        if part.is_empty()
            || part.len() > 3
            || !part.bytes().all(|b| b.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return Err(Failure::Fallback);
        }
        *octet = part.parse().map_err(|_| Failure::Fallback)?;
    }
    Ok(octets)
}

fn dotted(octets: [u8; 4]) -> String {
    format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3])
}

/// ipaddress's prefix: a length `0..=32` (RFC 3442 routes) or a netmask (udhcpc's `subnet`). A
/// hostmask (`0.0.0.255`), which ipaddress also takes, is left to Python.
fn prefix(text: &str) -> Result<u32, Failure> {
    if text.contains('.') {
        let mask = u32::from_be_bytes(ipv4(text)?);
        let length = mask.leading_ones();
        return if length == 32 || mask << length == 0 {
            Ok(length)
        } else {
            Err(Failure::Fallback)
        };
    }
    if text.is_empty()
        || !text.bytes().all(|b| b.is_ascii_digit())
        || (text.len() > 1 && text.starts_with('0'))
    {
        return Err(Failure::Fallback);
    }
    text.parse()
        .ok()
        .filter(|p| *p <= 32)
        .ok_or(Failure::Fallback)
}

/// `str(ipaddress.IPv4Network(text, strict=False))`.
fn network(text: &str) -> Result<String, Failure> {
    let (address, length) = match text.split_once('/') {
        Some((a, l)) => (ipv4(a)?, prefix(l)?),
        None => (ipv4(text)?, 32),
    };
    let mask = if length == 0 {
        0
    } else {
        u32::MAX << (32 - length)
    };
    Ok(format!(
        "{}/{length}",
        dotted((u32::from_be_bytes(address) & mask).to_be_bytes())
    ))
}

/// `y2-platform dhcp-event deconfig|leasefail|nak|bound|renew` with udhcpc's environment.
pub fn dhcp_event(root: &Root, event: &str) -> Outcome {
    let var = |name: &str| env::var(name).ok();
    if var("interface").as_deref() != Some("wlan0") {
        return Err(Failure::value("dhcp_interface_not_owned"));
    }
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    let _lock = lock(&runtime.join("dhcp.lock"))?;
    let epoch = root.read("/run/y2/wifi-epoch").filter(|e| !e.is_empty());
    if epoch.is_none() || var("Y2_DHCP_GENERATION") != epoch {
        return Err(Failure::value("stale_dhcp_epoch"));
    }
    unlink_missing_ok(&runtime.join("dns.json"))?;
    let mut state = Ordered::new()
        .with("epoch", json!(epoch))
        .with("state", "AcquiringIP")
        .with("error", Value::Null)
        .with("address", Value::Null);
    match event {
        "deconfig" | "nak" | "leasefail" => {
            for argv in FLUSH {
                command::run(root, argv, command::TIMEOUT);
            }
            resolv(root, &[])?;
            if event != "deconfig" {
                state.set("state", "Failed").set(
                    "error",
                    if event == "leasefail" {
                        "dhcp_timeout"
                    } else {
                        "dhcp_nak"
                    },
                );
            }
        }
        "bound" | "renew" => {
            let address = ipv4(&var("ip").unwrap_or_default())?;
            let length = prefix(&var("subnet").unwrap_or_default())?;
            if address == [0, 0, 0, 0] || (224..=239).contains(&address[0]) || address[0] == 127 {
                return Err(Failure::value("invalid_dhcp_address"));
            }
            let servers: Vec<String> = var("dns")
                .unwrap_or_default()
                .split_whitespace()
                .take(3)
                .map(|s| ipv4(s).map(dotted))
                .collect::<Result<_, _>>()?;
            let gateways: Vec<String> = var("router")
                .unwrap_or_default()
                .split_whitespace()
                .take(3)
                .map(|s| ipv4(s).map(dotted))
                .collect::<Result<_, _>>()?;
            // RFC3442: classless routes supersede the router option. Bound the list and restrict syntax
            // to parsed addresses, never shell text.
            let static_routes: Vec<String> = var("staticroutes")
                .unwrap_or_default()
                .split_whitespace()
                .map(str::to_owned)
                .collect();
            let routes: Vec<(String, String)> = if !static_routes.is_empty() {
                if static_routes.len() > 32 || static_routes.len() % 2 == 1 {
                    return Err(Failure::value("invalid_classless_routes"));
                }
                static_routes
                    .chunks(2)
                    .map(|pair| Ok((network(&pair[0])?, dotted(ipv4(&pair[1])?))))
                    .collect::<Result<_, Failure>>()?
            } else {
                gateways
                    .iter()
                    .take(1)
                    .map(|g| ("default".to_owned(), g.clone()))
                    .collect()
            };
            let interface = format!("{}/{length}", dotted(address));
            let mut commands: Vec<Vec<String>> = FLUSH
                .iter()
                .map(|a| a.iter().map(|s| s.to_string()).collect())
                .collect();
            commands.push(
                ["ip", "link", "set", "dev", "wlan0", "up"]
                    .map(str::to_owned)
                    .to_vec(),
            );
            commands.push(
                ["ip", "-4", "address", "add", &interface, "dev", "wlan0"]
                    .map(str::to_owned)
                    .to_vec(),
            );
            for (destination, gateway) in &routes {
                let mut argv = vec![
                    "ip".to_owned(),
                    "-4".into(),
                    "route".into(),
                    "replace".into(),
                    destination.clone(),
                ];
                if gateway != "0.0.0.0" {
                    argv.extend(["via".into(), gateway.clone()]);
                }
                argv.extend(["dev".into(), "wlan0".into(), "metric".into(), "100".into()]);
                commands.push(argv);
            }
            for argv in &commands {
                let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
                if !command::run(root, &argv, command::TIMEOUT).ok {
                    state
                        .set("state", "Failed")
                        .set("error", "lease_configuration_failed");
                    publish(root, "dhcp", state)?;
                    return Err(Failure::value("lease_configuration_failed"));
                }
            }
            resolv(root, &servers)?;
            state
                .set("state", "Authenticated")
                .set("address", dotted(address))
                .set("dns_servers", json!(servers))
                .set(
                    "error",
                    if servers.is_empty() {
                        json!("dns_unavailable")
                    } else {
                        Value::Null
                    },
                )
                .set("lease_seconds", json!(var("lease")));
        }
        _ => return Err(Failure::value("unknown_dhcp_event")),
    }
    Reply::unsorted(&publish(root, "dhcp", state)?, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_parse_as_ipaddress_does() {
        assert_eq!(dotted(ipv4("192.168.1.40").unwrap()), "192.168.1.40");
        for bad in [
            "01.2.3.4",
            "1.2.3",
            "256.1.1.1",
            "",
            "1.2.3.4.5",
            " 1.2.3.4",
        ] {
            assert!(matches!(ipv4(bad), Err(Failure::Fallback)), "{bad}");
        }
        assert_eq!(network("10.1.2.3/8").unwrap(), "10.0.0.0/8");
        assert_eq!(network("0.0.0.0/0").unwrap(), "0.0.0.0/0");
        assert_eq!(network("192.168.5.1").unwrap(), "192.168.5.1/32");
        assert_eq!(prefix("255.255.255.0").unwrap(), 24);
        assert_eq!(prefix("255.255.255.255").unwrap(), 32);
        assert_eq!(prefix("0.0.0.0").unwrap(), 0);
        assert!(matches!(prefix("0.0.0.255"), Err(Failure::Fallback)));
        assert!(matches!(prefix("255.0.255.0"), Err(Failure::Fallback)));
        assert_eq!(network("10.1.2.3/255.0.0.0").unwrap(), "10.0.0.0/8");
    }
}
