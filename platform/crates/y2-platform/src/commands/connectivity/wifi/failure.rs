//! `/run/y2/wifi-failure.json`: the last join failure (the event monitor writes it, `status wifi`,
//! Reborn and the reports read it), with the F52 wrong-password latch and the scan evidence of a "not
//! found". Every field reads leniently: a field of another shape reads as absent.
// SPDX-License-Identifier: GPL-2.0-only
use super::scan::ScanEvidence;
use serde::{Deserialize, Serialize};
use y2p_core::lenient;

/// The record as the monitor writes it (`network::publish` adds `schema`, `boot_id` and
/// `monotonic_s`) and as it reads back.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct FailureRecord {
    /// The failure class (null after a connect).
    #[serde(deserialize_with = "lenient::text")]
    pub error: Option<String>,
    /// The Wi-Fi network epoch the failure belongs to.
    #[serde(deserialize_with = "lenient::text")]
    pub epoch: Option<String>,
    /// Failures since the record began.
    #[serde(deserialize_with = "lenient::or_default")]
    pub failures: u64,
    /// wpa_supplicant's own code (`CONN_FAILED`, a status code): separates a password from a
    /// timeout or SAE (F31). Never a name.
    #[serde(
        deserialize_with = "lenient::text",
        skip_serializing_if = "Option::is_none"
    )]
    pub reason: Option<String>,
    /// Failed handshakes on one network since the last connect.
    #[serde(
        deserialize_with = "lenient::or_none",
        skip_serializing_if = "Option::is_none"
    )]
    pub wrong_key: Option<WrongKey>,
    /// What the failure before a connect showed (F13's scan evidence was gone by the time a report
    /// was saved).
    #[serde(
        deserialize_with = "lenient::or_none",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_failure: Option<LastFailure>,
    /// A wrong password, kept until a connect (Y2Reborn #20).
    #[serde(
        deserialize_with = "lenient::or_none",
        skip_serializing_if = "Option::is_none"
    )]
    pub credential: Option<Credential>,
    /// The scan of an "ap_unavailable", at most every 30 s.
    #[serde(
        deserialize_with = "lenient::or_none",
        skip_serializing_if = "Option::is_none"
    )]
    pub scan: Option<ScanEvidence>,
    #[serde(
        deserialize_with = "lenient::or_none",
        skip_serializing_if = "Option::is_none"
    )]
    pub scan_monotonic_s: Option<f64>,
    /// The stamp `publish` writes (read back; never written from here).
    #[serde(deserialize_with = "lenient::text", skip_serializing)]
    pub boot_id: Option<String>,
    #[serde(deserialize_with = "lenient::or_none", skip_serializing)]
    pub monotonic_s: Option<f64>,
}

/// Failed handshakes on `network_id` this boot.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct WrongKey {
    #[serde(deserialize_with = "lenient::or_none")]
    pub network_id: Option<i64>,
    #[serde(deserialize_with = "lenient::or_default")]
    pub count: u64,
    #[serde(deserialize_with = "lenient::text")]
    pub boot_id: Option<String>,
}

/// A password failure on a saved network.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Credential {
    #[serde(deserialize_with = "lenient::text")]
    pub error: Option<String>,
    #[serde(deserialize_with = "lenient::text")]
    pub reason: Option<String>,
    #[serde(deserialize_with = "lenient::text")]
    pub boot_id: Option<String>,
    /// wpa_supplicant's id of the saved network.
    #[serde(deserialize_with = "lenient::or_none")]
    pub network_id: Option<i64>,
    #[serde(deserialize_with = "lenient::or_none")]
    pub monotonic_s: Option<f64>,
}

/// The fields of the failure before a connect.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct LastFailure {
    #[serde(deserialize_with = "lenient::text")]
    pub error: Option<String>,
    #[serde(
        deserialize_with = "lenient::text",
        skip_serializing_if = "Option::is_none"
    )]
    pub reason: Option<String>,
    #[serde(deserialize_with = "lenient::text")]
    pub epoch: Option<String>,
    #[serde(deserialize_with = "lenient::or_default")]
    pub failures: u64,
    #[serde(
        deserialize_with = "lenient::or_none",
        skip_serializing_if = "Option::is_none"
    )]
    pub scan: Option<ScanEvidence>,
    #[serde(
        deserialize_with = "lenient::or_none",
        skip_serializing_if = "Option::is_none"
    )]
    pub scan_monotonic_s: Option<f64>,
    #[serde(deserialize_with = "lenient::text")]
    pub boot_id: Option<String>,
    #[serde(deserialize_with = "lenient::or_none")]
    pub monotonic_s: Option<f64>,
}

impl FailureRecord {
    /// The record (none of its fields when it is missing or no object).
    pub fn read(root: &y2p_core::Root) -> FailureRecord {
        root.read("/run/y2/wifi-failure.json")
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// What a connect keeps of this failure.
    pub fn last(&self) -> LastFailure {
        LastFailure {
            error: self.error.clone(),
            reason: self.reason.clone(),
            epoch: self.epoch.clone(),
            failures: self.failures,
            scan: self.scan.clone(),
            scan_monotonic_s: self.scan_monotonic_s,
            boot_id: self.boot_id.clone(),
            monotonic_s: self.monotonic_s,
        }
    }
}
