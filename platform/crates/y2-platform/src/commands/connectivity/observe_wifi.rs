//! This group's copy of `observe.wifi` and `observe.wifi_state` (the observation group ports
//! `status`; the DNS probe needs the same look after its lookup, plan §5a). Truthful readiness above
//! wpa_supplicant: null means unobserved, never zero or success.
// SPDX-License-Identifier: GPL-2.0-only
use serde_json::{json, Map, Value};
use y2p_core::py;
use y2p_core::{
    clock, command,
    json::{sorted_object, Object, ObjectExt},
    py::{number, truthy},
    root::{python_int, read, READ_LIMIT},
    Root,
};

/// A confirmed lease is re-probed every 300 s (network.DnsProbeSchedule); this keeps Online across one
/// slow or skipped re-check.
pub const DNS_FRESH_S: f64 = 420.0;

/// `common.key_values(text, separator)`: one `key<sep>value` per line, both stripped, a later key wins.
pub fn key_values(text: Option<&str>, separator: char) -> Map<String, Value> {
    let mut result = Map::new();
    for line in text.unwrap_or_default().lines() {
        if let Some((key, value)) = line.split_once(separator) {
            result.insert(
                y2p_core::root::python_strip(key).to_owned(),
                json!(y2p_core::root::python_strip(value)),
            );
        }
    }
    result
}

/// `x == text` for a JSON value and a text read from a file (`None` when unreadable).
pub fn eq_text(value: Option<&Value>, text: Option<&str>) -> bool {
    match (value, text) {
        (None | Some(Value::Null), None) => true,
        (Some(Value::String(a)), Some(b)) => a == b,
        _ => false,
    }
}

/// `isinstance(v, (int, float))` (a bool too) as a number.
pub fn clock_value(value: Option<&Value>) -> Option<f64> {
    number(value)
}

/// `wifi_state(radio, wpa, address, route, dns, dhcp_error)`.
pub fn wifi_state(
    radio: bool,
    wpa: &Map<String, Value>,
    address: &[Value],
    route: &Value,
    dns: bool,
    dhcp_error: Option<&Value>,
) -> (Value, Value) {
    let pair = |state: &str, reason: Value| (json!(state), reason);
    if !radio {
        return pair("Off", json!("radio_off"));
    }
    if wpa.is_empty() {
        return pair("Starting", json!("supplicant_unavailable"));
    }
    let state = wpa.get("wpa_state").and_then(Value::as_str);
    // Y2Reborn #20: wrong password. The supplicant goes on scanning and trying; without this the
    // player showed "Connecting…" and "Saved", never why.
    if state != Some("COMPLETED") {
        if let Some(failure) = wpa.get("credential_failure").filter(|v| truthy(v)) {
            return pair("Failed", failure.clone());
        }
    }
    match state {
        Some("SCANNING") => return pair("Scanning", Value::Null),
        Some("ASSOCIATING" | "ASSOCIATED" | "4WAY_HANDSHAKE" | "GROUP_HANDSHAKE") => {
            return pair("Associating", Value::Null)
        }
        Some("COMPLETED") => {}
        _ => {
            return pair(
                "Failed",
                wpa.get("failure_reason")
                    .cloned()
                    .unwrap_or(json!("not_authenticated")),
            )
        }
    }
    if address.is_empty() {
        return match dhcp_error.filter(|e| truthy(e)) {
            Some(error) => pair("Failed", error.clone()),
            None => pair("AcquiringIP", json!("no_address")),
        };
    }
    if !truthy(route) {
        return pair("Authenticated", json!("no_default_route"));
    }
    if !dns {
        return pair("Authenticated", json!("dns_unavailable"));
    }
    pair("Online", Value::Null)
}

fn object(value: Option<Value>) -> Map<String, Value> {
    match value {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

/// The global IPv4 addresses in `ip -j -4 addr show` and the routes in `ip -j -4 route show`; both
/// empty when either cannot be parsed. (Python raises an AttributeError for valid JSON of another
/// shape, e.g. a link that is not an object; that crash is not reproduced: such output reads as
/// unparsable here.)
fn addresses(ip: Option<&str>, routes: Option<&str>) -> (Vec<Value>, Value) {
    let parse = || -> Option<(Vec<Value>, Value)> {
        let links: Value =
            serde_json::from_str(ip.filter(|t| !t.is_empty()).unwrap_or("[]")).ok()?;
        let mut found = Vec::new();
        for link in links.as_array()? {
            let info = match link.as_object()?.get("addr_info") {
                None => continue,
                Some(info) => info.as_array()?,
            };
            for address in info {
                let address = address.as_object()?;
                if address.get("scope").and_then(Value::as_str) == Some("global") {
                    found.push(address.get("local")?.clone());
                }
            }
        }
        let route: Value =
            serde_json::from_str(routes.filter(|t| !t.is_empty()).unwrap_or("[]")).ok()?;
        Some((found, route))
    };
    parse().unwrap_or((Vec::new(), json!([])))
}

/// `observe.wifi(ctx, signal_cache)`.
pub fn wifi(root: &Root, signal_cache: Option<&Object>) -> Object {
    let radio = root.path("/sys/class/net/wlan0").exists();
    let status_started = clock::monotonic_ns();
    // Station statistics can hold the control loop for about 2.1 s on this firmware. Bound the
    // observation above that measured delay.
    let wpa = radio.then(|| command::run(root, &["wpa_cli", "-i", "wlan0", "status"], 3.0));
    let status_duration_ns = radio.then(|| clock::monotonic_ns() - status_started);
    let mut properties = key_values(wpa.as_ref().and_then(|w| w.output.as_deref()), '=');
    let failure = root.json_object("/run/y2/wifi-failure.json");
    let boot = root.read("/proc/sys/kernel/random/boot_id");
    let fresh = |at: Option<f64>, window: f64| {
        at.is_some_and(|at| (0.0..window).contains(&(clock::monotonic() - at)))
    };
    if eq_text(failure.get("boot_id"), boot.as_deref())
        && fresh(clock_value(failure.get("monotonic_s")), 120.0)
        && failure.get("error").is_some_and(truthy)
    {
        properties.insert("failure_reason".into(), failure["error"].clone());
    }
    let mut failed_network = Value::Null;
    if let Some(Value::Object(credential)) = failure.get("credential") {
        if credential.get("error").is_some_and(truthy)
            && eq_text(credential.get("boot_id"), boot.as_deref())
        {
            // Until the next connect (network.py drops it there).
            properties.insert("credential_failure".into(), credential["error"].clone());
            failed_network = credential.get("network_id").cloned().unwrap_or(Value::Null);
        }
    }
    let ip = radio.then(|| {
        command::run(
            root,
            &["ip", "-j", "-4", "addr", "show", "dev", "wlan0"],
            command::TIMEOUT,
        )
    });
    let routes = radio.then(|| {
        command::run(
            root,
            &["ip", "-j", "-4", "route", "show", "default", "dev", "wlan0"],
            command::TIMEOUT,
        )
    });
    let (addresses, default_route) = addresses(
        ip.as_ref().and_then(|o| o.output.as_deref()),
        routes.as_ref().and_then(|o| o.output.as_deref()),
    );
    let dns = root.json_object("/run/y2/dns.json");
    // Configuration alone never constitutes DNS readiness. Match network epoch.
    let epoch = root.read("/run/y2/wifi-epoch");
    let dns_ready = clock_value(dns.get("monotonic_s")).is_some()
        && dns.get("ok") == Some(&Value::Bool(true))
        && eq_text(dns.get("boot_id"), boot.as_deref())
        && epoch.as_deref().is_some_and(|e| !e.is_empty())
        && eq_text(dns.get("epoch"), epoch.as_deref())
        && addresses
            .iter()
            .any(|a| py::eq(dns.get("address"), Some(a)))
        && py::eq(dns.get("bssid"), properties.get("bssid"))
        && fresh(clock_value(dns.get("monotonic_s")), DNS_FRESH_S);
    let dhcp = root.json_object("/run/y2/dhcp.json");
    let error = (eq_text(dhcp.get("boot_id"), boot.as_deref())
        && eq_text(
            dhcp.get("epoch"),
            root.read("/run/y2/wifi-epoch").as_deref(),
        ))
    .then(|| dhcp.get("error").cloned())
    .flatten();
    let (state, reason) = wifi_state(
        radio,
        &properties,
        &addresses,
        &default_route,
        dns_ready,
        error.as_ref(),
    );
    let mut stats = Object::new();
    for path in root.glob("/sys/class/net/wlan0/statistics/*") {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let value = read(&path, READ_LIMIT).and_then(|t| python_int(&t));
        stats.set(&name, json!(value));
    }
    let previous = match signal_cache {
        Some(cache) => cache.clone(),
        None => sorted_object(&root.json_object("/run/y2/network.json")),
    };
    let signal = object(previous.get("signal_observation").cloned().filter(truthy));
    let signal_fresh = eq_text(signal.get("boot_id"), boot.as_deref())
        && py::eq(signal.get("bssid"), properties.get("bssid"))
        && fresh(clock_value(signal.get("monotonic_s")), 30.0);
    let mut rssi = if signal_fresh {
        previous.get("rssi_dbm").cloned().unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let completed = properties.get("wpa_state").and_then(Value::as_str) == Some("COMPLETED");
    let mut signal_value = Value::Object(signal.clone());
    if completed && !signal_fresh {
        let poll = command::run(root, &["wpa_cli", "-i", "wlan0", "signal_poll"], 3.0);
        let values = key_values(poll.output.as_deref(), '=');
        rssi = json!(values
            .get("RSSI")
            .and_then(Value::as_str)
            .and_then(python_int));
        signal_value = Object::new()
            .with("boot_id", json!(boot))
            .with(
                "bssid",
                properties.get("bssid").cloned().unwrap_or(Value::Null),
            )
            .with("monotonic_s", clock::monotonic())
            .with("ok", poll.ok)
            .with("reason", json!(poll.reason))
            .into_value();
    } else if !completed {
        rssi = Value::Null;
        signal_value = Value::Null;
    }
    let failed = state.as_str() == Some("Failed")
        && py::eq(Some(&reason), properties.get("credential_failure"));
    Object::new()
        .with("state", state)
        .with("reason", reason)
        .with("radio_present", radio)
        // The saved network whose password failed (wpa_supplicant's id).
        .with(
            "failed_network_id",
            if failed { failed_network } else { Value::Null },
        )
        // A missed control query does not prove a radio disconnect. Preserve the actual failure and
        // latency separately from event counters.
        .with(
            "supplicant_observation",
            Object::new()
                .with("ok", json!(wpa.as_ref().map(|w| w.ok)))
                .with(
                    "reason",
                    match &wpa {
                        Some(w) => json!(w.reason),
                        None => json!("radio_off"),
                    },
                )
                .with("duration_ns", json!(status_duration_ns))
                .into_value(),
        )
        .with(
            "association",
            properties.get("wpa_state").cloned().unwrap_or(Value::Null),
        )
        .with(
            "association_id",
            properties.get("bssid").cloned().unwrap_or(Value::Null),
        )
        .with("ip_addresses", Value::Array(addresses))
        .with("default_route", default_route)
        .with("dns_ready", dns_ready)
        .with("rssi_dbm", rssi)
        .with("signal_observation", signal_value)
        .with("traffic_counters", stats.into_value())
        .with("events", json!(root.json("/run/y2/wifi-events.json")))
        .with("reconnect_owner", "wpa_supplicant")
        .with("power_save", json!(root.json("/run/y2/wifi-power.json")))
        .with("coexistence", json!(root.json("/run/y2/coexistence.json")))
}

/// A nested `Object` as a plain value, its keys sorted: the observation is written
/// sorted (`network.json`, `status`) or compared, never printed in insertion order.
trait IntoValue {
    fn into_value(self) -> Value;
}

impl IntoValue for Object {
    fn into_value(self) -> Value {
        Value::Object(sorted_object(&self))
    }
}

#[cfg(test)]
#[path = "observe_wifi_tests.rs"]
mod observe_wifi_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_follow_observe_wifi_state() {
        let wpa = |pairs: &[(&str, &str)]| -> Map<String, Value> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), json!(v)))
                .collect()
        };
        let none: [Value; 0] = [];
        let one = [json!("192.168.1.40")];
        let route = json!([{"dst": "default"}]);
        let s = |r: (Value, Value)| (r.0.as_str().unwrap().to_owned(), r.1);
        assert_eq!(
            s(wifi_state(
                false,
                &Map::new(),
                &none,
                &json!([]),
                false,
                None
            )),
            ("Off".into(), json!("radio_off"))
        );
        assert_eq!(
            s(wifi_state(
                true,
                &Map::new(),
                &none,
                &json!([]),
                false,
                None
            ))
            .1,
            json!("supplicant_unavailable")
        );
        let failed = wpa(&[
            ("wpa_state", "DISCONNECTED"),
            ("credential_failure", "wrong_credentials"),
        ]);
        assert_eq!(
            s(wifi_state(true, &failed, &none, &json!([]), false, None)),
            ("Failed".into(), json!("wrong_credentials"))
        );
        assert_eq!(
            s(wifi_state(
                true,
                &wpa(&[("wpa_state", "INACTIVE")]),
                &none,
                &json!([]),
                false,
                None
            ))
            .1,
            json!("not_authenticated")
        );
        let done = wpa(&[("wpa_state", "COMPLETED")]);
        assert_eq!(
            s(wifi_state(
                true,
                &done,
                &none,
                &json!([]),
                false,
                Some(&json!("dhcp_nak"))
            )),
            ("Failed".into(), json!("dhcp_nak"))
        );
        assert_eq!(
            s(wifi_state(true, &done, &one, &json!([]), false, None)).1,
            json!("no_default_route")
        );
        assert_eq!(
            s(wifi_state(true, &done, &one, &route, false, None)).1,
            json!("dns_unavailable")
        );
        assert_eq!(
            s(wifi_state(true, &done, &one, &route, true, None)),
            ("Online".into(), Value::Null)
        );
    }

    #[test]
    fn python_equality_and_addresses() {
        assert!(py::eq(Some(&json!(1)), Some(&json!(1.0))));
        assert!(py::eq(Some(&json!(true)), Some(&json!(1))));
        assert!(py::eq(None, Some(&Value::Null)));
        assert!(!py::eq(Some(&json!("1")), Some(&json!(1))));
        let ip = r#"[{"ifname": "wlan0", "addr_info": [{"local": "10.0.0.5", "scope": "global"},
                    {"local": "169.254.1.1", "scope": "link"}]}]"#;
        assert_eq!(
            addresses(Some(ip), Some("[]")),
            (vec![json!("10.0.0.5")], json!([]))
        );
        assert_eq!(addresses(Some("{bad"), None), (vec![], json!([])));
        assert_eq!(
            addresses(Some(r#"[{"addr_info": [{"scope": "global"}]}]"#), None),
            (vec![], json!([]))
        );
    }
}
