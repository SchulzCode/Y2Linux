//! The `wifi` section: supplicant state, addresses, route, DNS readiness and signal.
// SPDX-License-Identifier: GPL-2.0-only
use crate::commands::connectivity::direct;
use crate::commands::observation::util::{
    self, fields, fresh_seconds, key_values, name, number, read, record_fields, text,
};
use crate::reply::Failure;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use y2p_core::py::PyValue;
use y2p_core::{
    command,
    py::{is_true, truthy},
    Root,
};

/// A confirmed lease is re-probed every 300 s (network.DnsProbeSchedule); this keeps Online across
/// one slow or skipped re-check.
pub(crate) const DNS_FRESH_S: f64 = 420.0;

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

pub(crate) fn wifi_state(
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
    let Some(links) = entries(&parsed)? else {
        return Ok(None);
    };
    let mut found = Vec::new();
    for link in links {
        let Value::Object(link) = link else {
            return Err(invalid_ip_output());
        };
        let infos = match link.get("addr_info") {
            None => &[][..],
            Some(value) => match entries(value)? {
                Some(items) => items,
                None => return Ok(None),
            },
        };
        for info in infos {
            let Value::Object(info) = info else {
                return Err(invalid_ip_output());
            };
            if info.get("scope").and_then(Value::as_str) == Some("global") {
                match info.get("local") {
                    Some(local) => found.push(local.clone()),
                    None => return Ok(None),
                }
            }
        }
    }
    Ok(Some(found))
}

fn invalid_ip_output() -> Failure {
    Failure::value("invalid_ip_output")
}

/// The entries of a list in `ip -j`'s output. An empty object or text holds none; a non-empty one
/// holds keys or characters, which are no entries (refused); any other value is no list (`None`).
fn entries(value: &Value) -> Result<Option<&[Value]>, Failure> {
    match value {
        Value::Array(items) => Ok(Some(items)),
        Value::Object(map) if map.is_empty() => Ok(Some(&[])),
        Value::String(text) if text.is_empty() => Ok(Some(&[])),
        Value::Object(_) | Value::String(_) => Err(invalid_ip_output()),
        _ => Ok(None),
    }
}

/// How an odd record (an `ip -j` entry or a network.json signal stamp of the wrong shape) is read:
/// `status` refuses it; the connectivity and service daemons, which must keep running, read it as
/// empty.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Odd {
    Refuse,
    Empty,
}

/// The `wifi` section of `status`.
pub fn wifi(root: &Root) -> Result<Value, Failure> {
    observe(root, None, Odd::Refuse)
}

/// What Wi-Fi looks like now: readiness above wpa_supplicant, the lease, DNS and the signal. The last
/// RSSI poll (reused for 30 s on the same peer) is `signal_cache` (the daemon's previous look), else
/// the one network.json published.
pub fn observe(
    root: &Root,
    signal_cache: Option<&Map<String, Value>>,
    odd: Odd,
) -> Result<Value, Failure> {
    let radio = root.path("/sys/class/net/wlan0").exists();
    let started = root.clock().monotonic_ns();
    // Station statistics can hold the control loop for about 2.1 s on this firmware. Bound the
    // observation above that measured delay.
    // On the player wpa_supplicant's control socket answers in process (connectivity::direct).
    let wpa = radio.then(|| direct::wpa_cli(root, "status", 3.0));
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
            direct::ipv4_addresses(root, "wlan0", command::TIMEOUT).output,
            direct::default_route(root, "wlan0", command::TIMEOUT).output,
        )
    } else {
        (None, None)
    };
    let found = match addresses(ip.as_deref()) {
        Err(refusal) if odd == Odd::Refuse => return Err(refusal),
        Err(_) => None,
        Ok(found) => found,
    };
    let (addresses, default_route) = match found {
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
    match root
        .is_real()
        .then(|| direct::link_statistics("wlan0", 3.0))
        .flatten()
    {
        Some(counters) => {
            for (counter, value) in counters {
                stats.insert(
                    counter.into(),
                    i64::try_from(value).map_or(Value::Null, Value::from),
                );
            }
        }
        None => {
            for path in root.glob("/sys/class/net/wlan0/statistics/*") {
                stats.insert(name(&path), number(read(&path).as_deref()));
            }
        }
    }
    let previous: Published = match signal_cache {
        Some(cache) => fields(cache),
        None => record_fields(root, "/run/y2/network.json"),
    };
    let stamp: SignalStamp = match &previous.signal_observation {
        Value::Object(map) => fields(map),
        other if truthy(other) && odd == Odd::Refuse => {
            return Err(Failure::value("invalid_network_record"))
        }
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
        let poll = direct::wpa_cli(root, "signal_poll", 3.0);
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
    let section = Wifi {
        failed_network_id: if state == "Failed" && reason.py_eq(&credential) {
            failed_network
        } else {
            Value::Null
        },
        state,
        reason,
        radio_present: radio,
        supplicant_observation: SupplicantObservation {
            ok: wpa.as_ref().map(|w| w.ok),
            reason: match &wpa {
                Some(w) => w.reason.clone(),
                None => Some("radio_off".into()),
            },
            duration_ns: duration,
        },
        association,
        association_id: bssid,
        ip_addresses: addresses,
        default_route,
        dns_ready,
        rssi_dbm: rssi,
        signal_observation: signal,
        traffic_counters: stats,
        events: root.json("/run/y2/wifi-events.json"),
        reconnect_owner: "wpa_supplicant",
        power_save: root.json("/run/y2/wifi-power.json"),
        coexistence: root.json("/run/y2/coexistence.json"),
    };
    serde_json::to_value(section).map_err(|_| Failure::value("wifi_section_unserializable"))
}

/// The `wifi` section and the daemons' look (network.json `observation`), in this key order.
#[derive(Serialize)]
struct Wifi {
    state: Value,
    reason: Value,
    radio_present: bool,
    /// The saved network whose password failed (wpa_supplicant's id).
    failed_network_id: Value,
    /// A missed control query does not prove a radio disconnect: the query's own result and
    /// latency, apart from the event counters.
    supplicant_observation: SupplicantObservation,
    association: Value,
    association_id: Value,
    ip_addresses: Vec<Value>,
    default_route: Value,
    dns_ready: bool,
    rssi_dbm: Value,
    signal_observation: Value,
    traffic_counters: Map<String, Value>,
    events: Option<Value>,
    reconnect_owner: &'static str,
    power_save: Option<Value>,
    coexistence: Option<Value>,
}

#[derive(Serialize)]
struct SupplicantObservation {
    ok: Option<bool>,
    reason: Option<String>,
    duration_ns: Option<u64>,
}
