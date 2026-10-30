//! The Wi-Fi look the service daemon publishes as network.json and the DNS probe repeats after its
//! lookup: `status wifi`'s observation (observation/snapshot/wifi.rs), read leniently, and the record
//! helpers the connectivity group shares.
// SPDX-License-Identifier: GPL-2.0-only
use crate::commands::observation::snapshot::wifi::{observe, Odd};
use serde_json::Value;
use y2p_core::{json::Object, py::number, Root};

/// A clock reading of a record: any number (a bool reads as 0 or 1).
pub fn clock_value(value: Option<&Value>) -> Option<f64> {
    number(value)
}

/// What Wi-Fi looks like now (`status wifi`'s observation; network.json and the DNS probe's
/// before/after look). An odd record reads as empty: the daemons that call this keep running.
pub fn wifi(root: &Root, signal_cache: Option<&Object>) -> Object {
    match observe(root, signal_cache, Odd::Empty) {
        Ok(Value::Object(map)) => map,
        _ => Object::new(),
    }
}

#[cfg(test)]
#[path = "observe_wifi_tests.rs"]
mod observe_wifi_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::observation::snapshot::wifi::wifi_state;
    use serde_json::{json, Map};
    use y2p_core::py;

    #[test]
    fn states_follow_observe_wifi_state() {
        let wpa = |pairs: &[(&str, &str)]| -> Map<String, Value> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), json!(v)))
                .collect()
        };
        let s = |r: (Value, Value)| (r.0.as_str().unwrap().to_owned(), r.1);
        assert_eq!(
            s(wifi_state(
                false,
                &Map::new(),
                false,
                false,
                false,
                &Value::Null
            )),
            ("Off".into(), json!("radio_off"))
        );
        assert_eq!(
            s(wifi_state(
                true,
                &Map::new(),
                false,
                false,
                false,
                &Value::Null
            ))
            .1,
            json!("supplicant_unavailable")
        );
        let failed = wpa(&[
            ("wpa_state", "DISCONNECTED"),
            ("credential_failure", "wrong_credentials"),
        ]);
        assert_eq!(
            s(wifi_state(true, &failed, false, false, false, &Value::Null)),
            ("Failed".into(), json!("wrong_credentials"))
        );
        assert_eq!(
            s(wifi_state(
                true,
                &wpa(&[("wpa_state", "INACTIVE")]),
                false,
                false,
                false,
                &Value::Null
            ))
            .1,
            json!("not_authenticated")
        );
        let done = wpa(&[("wpa_state", "COMPLETED")]);
        assert_eq!(
            s(wifi_state(
                true,
                &done,
                false,
                false,
                false,
                &json!("dhcp_nak")
            )),
            ("Failed".into(), json!("dhcp_nak"))
        );
        assert_eq!(
            s(wifi_state(true, &done, true, false, false, &Value::Null)).1,
            json!("no_default_route")
        );
        assert_eq!(
            s(wifi_state(true, &done, true, true, false, &Value::Null)).1,
            json!("dns_unavailable")
        );
        assert_eq!(
            s(wifi_state(true, &done, true, true, true, &Value::Null)),
            ("Online".into(), Value::Null)
        );
    }

    #[test]
    fn python_equality() {
        assert!(py::eq(Some(&json!(1)), Some(&json!(1.0))));
        assert!(py::eq(Some(&json!(true)), Some(&json!(1))));
        assert!(py::eq(None, Some(&Value::Null)));
        assert!(!py::eq(Some(&json!("1")), Some(&json!(1))));
    }
}
