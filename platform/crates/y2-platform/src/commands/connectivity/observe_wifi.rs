//! The Wi-Fi look the service daemon publishes as network.json and the DNS probe repeats after its
//! lookup: `status wifi`'s observation (observation/snapshot/wifi.rs), read leniently, and the record
//! helpers the connectivity group shares.
// SPDX-License-Identifier: GPL-2.0-only
use crate::commands::observation::snapshot::wifi::{observe, Wifi};
use y2p_core::Root;

/// What Wi-Fi looks like now (`status wifi`'s observation; network.json and the DNS probe's
/// before/after look), with the signal reading of `signal_cache` while it is fresh.
pub fn wifi(root: &Root, signal_cache: Option<&Wifi>) -> Wifi {
    observe(root, signal_cache)
}

#[cfg(test)]
#[path = "observe_wifi_tests.rs"]
mod observe_wifi_tests;

#[cfg(test)]
mod tests {
    use crate::commands::observation::snapshot::wifi::wifi_state;
    use serde_json::json;
    use serde_json::Value;
    use std::collections::BTreeMap;
    use y2p_core::py;

    #[test]
    fn states_follow_the_supplicant_and_the_lease() {
        let wpa = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        };
        let none = BTreeMap::new();
        let reason = |text: &str| Some(text.to_owned());
        assert_eq!(
            wifi_state(false, &none, false, false, false, None),
            ("Off", reason("radio_off"))
        );
        assert_eq!(
            wifi_state(true, &none, false, false, false, None).1,
            reason("supplicant_unavailable")
        );
        let failed = wpa(&[
            ("wpa_state", "DISCONNECTED"),
            ("credential_failure", "wrong_credentials"),
        ]);
        assert_eq!(
            wifi_state(true, &failed, false, false, false, None),
            ("Failed", reason("wrong_credentials"))
        );
        let inactive = wpa(&[("wpa_state", "INACTIVE")]);
        assert_eq!(
            wifi_state(true, &inactive, false, false, false, None).1,
            reason("not_authenticated")
        );
        let done = wpa(&[("wpa_state", "COMPLETED")]);
        assert_eq!(
            wifi_state(true, &done, false, false, false, Some("dhcp_nak")),
            ("Failed", reason("dhcp_nak"))
        );
        assert_eq!(
            wifi_state(true, &done, true, false, false, None).1,
            reason("no_default_route")
        );
        assert_eq!(
            wifi_state(true, &done, true, true, false, None).1,
            reason("dns_unavailable")
        );
        assert_eq!(
            wifi_state(true, &done, true, true, true, None),
            ("Online", None)
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
