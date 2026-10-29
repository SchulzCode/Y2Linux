//! The `wifi` section: supplicant state, addresses, route, DNS readiness and signal.
// SPDX-License-Identifier: GPL-2.0-only
use crate::commands::observation::util::{
    self, fields, fresh_seconds, key_values, name, number, read, record_fields, text,
};
use crate::reply::Failure;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use y2p_core::py::PyValue;
use y2p_core::{
    command,
    py::{is_true, truthy},
    Root,
};

/// A confirmed lease is re-probed every 300 s (network.DnsProbeSchedule); this keeps Online across
/// one slow or skipped re-check.
const DNS_FRESH_S: f64 = 420.0;

/// `/run/y2/wifi-failure.json` (connectivity): the last join failure, and a saved network's password
/// failure (kept until the next connect).
#[derive(Deserialize, Default)]
#[serde(default)]
struct JoinFailure {
    boot_id: Value,
    monotonic_s: Value,
    error: Value,
    credential: Value,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct CredentialFailure {
    boot_id: Value,
    error: Value,
    network_id: Value,
}

/// `/run/y2/dns.json`: the last DNS probe of a lease.
#[derive(Deserialize, Default)]
#[serde(default)]
struct DnsProbe {
    ok: Value,
    boot_id: Value,
    epoch: Value,
    address: Value,
    bssid: Value,
    monotonic_s: Value,
}

/// `/run/y2/dhcp.json`: the DHCP client's last answer for a lease.
#[derive(Deserialize, Default)]
#[serde(default)]
struct DhcpAnswer {
    boot_id: Value,
    epoch: Value,
    error: Value,
}

/// The parts of `/run/y2/network.json` (the last status the connectivity daemon published) a new
/// observation reuses: the signal reading while it is fresh.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Published {
    signal_observation: Value,
    rssi_dbm: Value,
}

/// When and for which access point a signal was polled.
#[derive(Deserialize, Default)]
#[serde(default)]
struct SignalStamp {
    boot_id: Value,
    bssid: Value,
    monotonic_s: Value,
}

fn wifi_state(
    radio: bool,
    wpa: &Map<String, Value>,
    address: bool,
    route: bool,
    dns: bool,
    dhcp_error: &Value,
) -> (Value, Value) {
    let s = |v: &str| json!(v);
    if !radio {
        return (s("Off"), s("radio_off"));
    }
    if wpa.is_empty() {
        return (s("Starting"), s("supplicant_unavailable"));
    }
    let state = wpa.get("wpa_state").and_then(Value::as_str);
    // Y2Reborn #20: wrong password. The supplicant goes on scanning and trying; without this the
    // player showed "Connecting…" and "Saved", never why.
    if state != Some("COMPLETED") && wpa.get("credential_failure").is_some_and(truthy) {
        return (s("Failed"), wpa["credential_failure"].clone());
    }
    match state {
        Some("SCANNING") => return (s("Scanning"), Value::Null),
        Some("ASSOCIATING" | "ASSOCIATED" | "4WAY_HANDSHAKE" | "GROUP_HANDSHAKE") => {
            return (s("Associating"), Value::Null)
        }
        Some("COMPLETED") => {}
        _ => {
            return (
                s("Failed"),
                wpa.get("failure_reason")
                    .cloned()
                    .unwrap_or(s("not_authenticated")),
            )
        }
    }
    if !address {
        return if truthy(dhcp_error) {
            (s("Failed"), dhcp_error.clone())
        } else {
            (s("AcquiringIP"), s("no_address"))
        };
    }
    if !route {
        return (s("Authenticated"), s("no_default_route"));
    }
    if !dns {
        return (s("Authenticated"), s("dns_unavailable"));
    }
    (s("Online"), Value::Null)
}

/// The global addresses of `ip -j -4 addr`: Ok(None) for output that is no JSON, no list, an
/// `addr_info` that is no list or a global address without `local` (no addresses then); a link or
/// address entry that is no object refuses the snapshot (`invalid_ip_output`).
fn addresses(output: Option<&str>) -> Result<Option<Vec<Value>>, Failure> {
    let text = output.filter(|o| !o.is_empty()).unwrap_or("[]");
    let Ok(parsed) = serde_json::from_str::<Value>(text) else {
        return Ok(None);
    };
    let Some(links) = y2p_core::py::iterate(&parsed) else {
        return Ok(None); // not iterable: TypeError
    };
    let mut found = Vec::new();
    for link in links {
        let Value::Object(link) = link else {
            return Err(Failure::value("invalid_ip_output"));
        };
        let infos = match link.get("addr_info") {
            None => Vec::new(),
            Some(value) => match y2p_core::py::iterate(value) {
                Some(items) => items,
                None => return Ok(None),
            },
        };
        for info in infos {
            let Value::Object(info) = info else {
                return Err(Failure::value("invalid_ip_output"));
            };
            if info.get("scope").and_then(Value::as_str) == Some("global") {
                match info.get("local") {
                    Some(local) => found.push(local.clone()),
                    None => return Ok(None), // KeyError
                }
            }
        }
    }
    Ok(Some(found))
}

pub fn wifi(root: &Root) -> Result<Value, Failure> {
    let radio = root.path("/sys/class/net/wlan0").exists();
    let started = root.clock().monotonic_ns();
    // Station statistics can hold the control loop for about 2.1 s on this firmware. Bound the
    // observation above that measured delay.
    let wpa = radio.then(|| command::run(root, &["wpa_cli", "-i", "wlan0", "status"], 3.0));
    let duration = radio.then(|| root.clock().monotonic_ns() - started);
    let mut properties: Map<String, Value> =
        key_values(wpa.as_ref().and_then(|w| w.output.as_deref()), "=")
            .into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect();
    let boot = Value::from(root.boot_id());
    let failure: JoinFailure = record_fields(root, "/run/y2/wifi-failure.json");
    if failure.boot_id.py_eq(&boot)
        && fresh_seconds(root, Some(&failure.monotonic_s), 120.0)
        && truthy(&failure.error)
    {
        properties.insert("failure_reason".into(), failure.error);
    }
    let mut failed_network = Value::Null;
    if let Value::Object(credential) = &failure.credential {
        let credential: CredentialFailure = fields(credential);
        if truthy(&credential.error) && credential.boot_id.py_eq(&boot) {
            properties.insert("credential_failure".into(), credential.error);
            failed_network = credential.network_id;
        }
    }
    let (ip, routes) = if radio {
        (
            command::run(
                root,
                &["ip", "-j", "-4", "addr", "show", "dev", "wlan0"],
                command::TIMEOUT,
            )
            .output,
            command::run(
                root,
                &["ip", "-j", "-4", "route", "show", "default", "dev", "wlan0"],
                command::TIMEOUT,
            )
            .output,
        )
    } else {
        (None, None)
    };
    let (addresses, default_route) = match addresses(ip.as_deref())? {
        Some(found) => match serde_json::from_str::<Value>(
            routes.as_deref().filter(|r| !r.is_empty()).unwrap_or("[]"),
        ) {
            Ok(route) => (found, route),
            Err(_) => (Vec::new(), json!([])),
        },
        None => (Vec::new(), json!([])),
    };
    // A probe of this lease (boot, network epoch, address, access point), not configuration alone,
    // makes DNS ready.
    let dns: DnsProbe = record_fields(root, "/run/y2/dns.json");
    let epoch = text(root.read("/run/y2/wifi-epoch"));
    let bssid = properties.get("bssid").cloned().unwrap_or(Value::Null);
    let dns_ready = util::is_number(Some(&dns.monotonic_s)).is_some()
        && is_true(Some(&dns.ok))
        && dns.boot_id.py_eq(&boot)
        && truthy(&epoch)
        && dns.epoch.py_eq(&epoch)
        && util::contains(&addresses, &dns.address)
        && dns.bssid.py_eq(&bssid)
        && fresh_seconds(root, Some(&dns.monotonic_s), DNS_FRESH_S);
    let dhcp: DhcpAnswer = record_fields(root, "/run/y2/dhcp.json");
    let dhcp_error = if dhcp.boot_id.py_eq(&boot) && dhcp.epoch.py_eq(&epoch) {
        dhcp.error
    } else {
        Value::Null
    };
    let (state, reason) = wifi_state(
        radio,
        &properties,
        !addresses.is_empty(),
        truthy(&default_route),
        dns_ready,
        &dhcp_error,
    );
    let mut stats = Map::new();
    for path in root.glob("/sys/class/net/wlan0/statistics/*") {
        stats.insert(name(&path), number(read(&path).as_deref()));
    }
    let previous: Published = record_fields(root, "/run/y2/network.json");
    let stamp: SignalStamp = match &previous.signal_observation {
        Value::Object(map) => fields(map),
        other if truthy(other) => return Err(Failure::value("invalid_network_record")),
        _ => SignalStamp::default(),
    };
    let fresh = stamp.boot_id.py_eq(&boot)
        && stamp.bssid.py_eq(&bssid)
        && fresh_seconds(root, Some(&stamp.monotonic_s), 30.0);
    let (mut signal, mut rssi) = match previous.signal_observation {
        observed @ Value::Object(_) if fresh => (observed, previous.rssi_dbm),
        observed @ Value::Object(_) => (observed, Value::Null),
        _ => (json!({}), Value::Null),
    };
    let association = properties.get("wpa_state").cloned().unwrap_or(Value::Null);
    if association.as_str() == Some("COMPLETED") && !fresh {
        let poll = command::run(root, &["wpa_cli", "-i", "wlan0", "signal_poll"], 3.0);
        let values = key_values(poll.output.as_deref(), "=");
        rssi = number(values.get("RSSI").map(String::as_str));
        signal = json!({"boot_id": boot, "bssid": bssid, "monotonic_s": root.clock().monotonic(),
                        "ok": poll.ok, "reason": poll.reason});
    } else if association.as_str() != Some("COMPLETED") {
        rssi = Value::Null;
        signal = Value::Null;
    }
    let credential = properties
        .get("credential_failure")
        .cloned()
        .unwrap_or(Value::Null);
    Ok(json!({
        "state": state, "reason": reason, "radio_present": radio,
        // The saved network whose password failed (wpa_supplicant's id).
        "failed_network_id": if state == "Failed" && reason.py_eq(&credential) { failed_network } else { Value::Null },
        // A missed control query does not prove a radio disconnect. Preserve the actual failure and
        // latency separately from event counters.
        "supplicant_observation": {"ok": wpa.as_ref().map(|w| w.ok),
                                   "reason": match &wpa { Some(w) => json!(w.reason), None => json!("radio_off") },
                                   "duration_ns": duration},
        "association": association, "association_id": bssid,
        "ip_addresses": addresses, "default_route": default_route, "dns_ready": dns_ready,
        "rssi_dbm": rssi, "signal_observation": signal, "traffic_counters": stats,
        "events": root.json("/run/y2/wifi-events.json"), "reconnect_owner": "wpa_supplicant",
        "power_save": root.json("/run/y2/wifi-power.json"),
        "coexistence": root.json("/run/y2/coexistence.json"),
    }))
}
