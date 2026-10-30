//! The `wifi` section: supplicant state, addresses, route, DNS readiness and signal.
// SPDX-License-Identifier: GPL-2.0-only
use crate::commands::connectivity::direct;
use crate::commands::connectivity::direct::{Link, Route};
use crate::commands::connectivity::wifi::failure::FailureRecord;
use crate::commands::observation::util::{fields, key_values, name, number, read, record_fields};
use crate::reply::Failure;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use y2p_core::{
    command,
    lenient::{self, BootId},
    Root,
};

/// A confirmed lease is re-probed every 300 s (network.DnsProbeSchedule); this keeps Online across
/// one slow or skipped re-check.
pub(crate) const DNS_FRESH_S: f64 = 420.0;

/// `/run/y2/dns.json`: the last DNS probe of a lease.
#[derive(Deserialize, Default)]
#[serde(default)]
struct DnsProbe {
    #[serde(deserialize_with = "lenient::is_true")]
    ok: bool,
    boot_id: BootId,
    #[serde(deserialize_with = "lenient::text")]
    epoch: Option<String>,
    #[serde(deserialize_with = "lenient::text")]
    address: Option<String>,
    #[serde(deserialize_with = "lenient::text")]
    bssid: Option<String>,
    #[serde(deserialize_with = "lenient::number")]
    monotonic_s: Option<f64>,
}

/// `/run/y2/dhcp.json`: the DHCP client's last answer for a lease.
#[derive(Deserialize, Default)]
#[serde(default)]
struct DhcpAnswer {
    boot_id: BootId,
    #[serde(deserialize_with = "lenient::text")]
    epoch: Option<String>,
    #[serde(deserialize_with = "lenient::text")]
    error: Option<String>,
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
    boot_id: BootId,
    #[serde(deserialize_with = "lenient::text")]
    bssid: Option<String>,
    #[serde(deserialize_with = "lenient::number")]
    monotonic_s: Option<f64>,
}

/// `0 <= now - stamp < window` (seconds on the monotonic clock).
fn fresh(root: &Root, stamp: Option<f64>, window: f64) -> bool {
    stamp.is_some_and(|t| (0.0..window).contains(&(root.clock().monotonic() - t)))
}

/// The state and reason above wpa_supplicant's `status` (its `key=value` properties, with the
/// platform's join and password failures added).
pub(crate) fn wifi_state(
    radio: bool,
    wpa: &BTreeMap<String, String>,
    address: bool,
    route: bool,
    dns: bool,
    dhcp_error: Option<&str>,
) -> (&'static str, Option<String>) {
    if !radio {
        return ("Off", Some("radio_off".into()));
    }
    if wpa.is_empty() {
        return ("Starting", Some("supplicant_unavailable".into()));
    }
    let state = wpa.get("wpa_state").map(String::as_str);
    // Y2Reborn #20: wrong password. The supplicant goes on scanning and trying; without this the
    // player showed "Connecting…" and "Saved", never why.
    let credential = wpa.get("credential_failure").filter(|c| !c.is_empty());
    if let (false, Some(credential)) = (state == Some("COMPLETED"), credential) {
        return ("Failed", Some(credential.clone()));
    }
    match state {
        Some("SCANNING") => return ("Scanning", None),
        Some("ASSOCIATING" | "ASSOCIATED" | "4WAY_HANDSHAKE" | "GROUP_HANDSHAKE") => {
            return ("Associating", None)
        }
        Some("COMPLETED") => {}
        _ => {
            let reason = wpa.get("failure_reason").map(String::as_str);
            return (
                "Failed",
                Some(reason.unwrap_or("not_authenticated").to_owned()),
            );
        }
    }
    if !address {
        return match dhcp_error.filter(|e| !e.is_empty()) {
            Some(error) => ("Failed", Some(error.to_owned())),
            None => ("AcquiringIP", Some("no_address".into())),
        };
    }
    if !route {
        return ("Authenticated", Some("no_default_route".into()));
    }
    if !dns {
        return ("Authenticated", Some("dns_unavailable".into()));
    }
    ("Online", None)
}

/// The global IPv4 addresses of the links.
fn global_addresses(links: &[Link]) -> Vec<String> {
    links
        .iter()
        .flat_map(|link| &link.addr_info)
        .filter(|address| address.scope.as_deref() == Some("global"))
        .filter_map(|address| address.local.clone())
        .collect()
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
pub fn wifi(root: &Root) -> Result<Wifi, Failure> {
    observe(root, None, Odd::Refuse)
}

/// What Wi-Fi looks like now: readiness above wpa_supplicant, the lease, DNS and the signal. The last
/// RSSI poll (reused for 30 s on the same peer) is `signal_cache` (the daemon's previous look), else
/// the one network.json published.
pub fn observe(
    root: &Root,
    signal_cache: Option<&Map<String, Value>>,
    odd: Odd,
) -> Result<Wifi, Failure> {
    let radio = root.path("/sys/class/net/wlan0").exists();
    let started = root.clock().monotonic_ns();
    // Station statistics can hold the control loop for about 2.1 s on this firmware. Bound the
    // observation above that measured delay.
    // On the player wpa_supplicant's control socket answers in process (connectivity::direct).
    let wpa = radio.then(|| direct::wpa_cli(root, "status", 3.0));
    let duration = radio.then(|| root.clock().monotonic_ns() - started);
    let mut properties = key_values(wpa.as_ref().and_then(|w| w.output.as_deref()), "=");
    let boot = root.boot_id();
    let boot = boot.as_deref();
    let failure = FailureRecord::read(root);
    if failure.boot_id.as_deref() == boot && fresh(root, failure.monotonic_s, 120.0) {
        if let Some(error) = failure.error.filter(|e| !e.is_empty()) {
            properties.insert("failure_reason".into(), error);
        }
    }
    let mut failed_network = None;
    if let Some(credential) = failure.credential {
        if let (true, Some(error)) = (
            credential.boot_id.as_deref() == boot,
            credential.error.filter(|e| !e.is_empty()),
        ) {
            properties.insert("credential_failure".into(), error);
            failed_network = credential.network_id;
        }
    }
    // Addresses and the default route from one look; an unreadable answer is neither.
    let (addresses, default_route) = if radio {
        match (
            direct::ipv4_addresses(root, "wlan0", command::TIMEOUT),
            direct::default_route(root, "wlan0", command::TIMEOUT),
        ) {
            (Ok(links), Ok(routes)) => (global_addresses(&links), routes),
            (Ok(links), Err(_)) => (global_addresses(&links), Vec::new()),
            (Err(_), _) => (Vec::new(), Vec::new()),
        }
    } else {
        (Vec::new(), Vec::new())
    };
    // A probe of this lease (boot, network epoch, address, access point), not configuration alone,
    // makes DNS ready.
    let dns: DnsProbe = record_fields(root, "/run/y2/dns.json");
    let epoch = root.read("/run/y2/wifi-epoch").filter(|e| !e.is_empty());
    let bssid = properties.get("bssid").cloned();
    let dns_ready = dns.ok
        && dns.boot_id.is(boot)
        && epoch.is_some()
        && dns.epoch == epoch
        && dns.address.as_ref().is_some_and(|a| addresses.contains(a))
        && dns.bssid == bssid
        && fresh(root, dns.monotonic_s, DNS_FRESH_S);
    let dhcp: DhcpAnswer = record_fields(root, "/run/y2/dhcp.json");
    let dhcp_error = dhcp
        .error
        .filter(|_| dhcp.boot_id.is(boot) && dhcp.epoch == epoch);
    let has_route = !default_route.is_empty();
    let (state, reason) = wifi_state(
        radio,
        &properties,
        !addresses.is_empty(),
        has_route,
        dns_ready,
        dhcp_error.as_deref(),
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
        Value::Null => SignalStamp::default(),
        _ if odd == Odd::Refuse => return Err(Failure::value("invalid_network_record")),
        _ => SignalStamp::default(),
    };
    let fresh_signal =
        stamp.boot_id.is(boot) && stamp.bssid == bssid && fresh(root, stamp.monotonic_s, 30.0);
    let (mut signal, mut rssi) = match previous.signal_observation {
        observed @ Value::Object(_) if fresh_signal => (observed, previous.rssi_dbm),
        observed @ Value::Object(_) => (observed, Value::Null),
        _ => (json!({}), Value::Null),
    };
    let association = properties.get("wpa_state").cloned();
    let completed = association.as_deref() == Some("COMPLETED");
    if completed && !fresh_signal {
        let poll = direct::wpa_cli(root, "signal_poll", 3.0);
        let values = key_values(poll.output.as_deref(), "=");
        rssi = number(values.get("RSSI").map(String::as_str));
        signal = json!({"boot_id": boot, "bssid": bssid, "monotonic_s": root.clock().monotonic(),
                        "ok": poll.ok, "reason": poll.reason});
    } else if !completed {
        rssi = Value::Null;
        signal = Value::Null;
    }
    let credential = properties.get("credential_failure");
    Ok(Wifi {
        failed_network_id: if state == "Failed" && reason.as_ref() == credential {
            failed_network
        } else {
            None
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
    })
}

/// The `wifi` section and the daemons' look (network.json `observation`), in this key order.
#[derive(Serialize)]
pub struct Wifi {
    pub state: &'static str,
    pub reason: Option<String>,
    radio_present: bool,
    /// The saved network whose password failed (wpa_supplicant's id).
    failed_network_id: Option<i64>,
    /// A missed control query does not prove a radio disconnect: the query's own result and
    /// latency, apart from the event counters.
    supplicant_observation: SupplicantObservation,
    association: Option<String>,
    association_id: Option<String>,
    pub ip_addresses: Vec<String>,
    pub default_route: Vec<Route>,
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
pub struct SupplicantObservation {
    ok: Option<bool>,
    reason: Option<String>,
    duration_ns: Option<u64>,
}
