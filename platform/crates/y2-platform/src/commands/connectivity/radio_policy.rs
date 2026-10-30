//! The runtime Wi-Fi power-save policy (no invented firmware features).
//! `y2-platform wifi-power-policy` runs it without activity; the service daemon runs it in process every
//! 30 s while associated, with the coexistence record as activity.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::{atomic_json, Failure, Outcome, Reply};
use serde::{Deserialize, Serialize};
use y2p_core::{command, lenient, Root};

/// `iw dev wlan0 get power_save`: cfg80211's flag, or None, and the answer.
fn readback(root: &Root) -> (Option<bool>, command::Outcome) {
    let answer = super::direct::power_save(root, "wlan0", 3.0);
    let actual = if answer.ok {
        match answer.output.as_deref().map(y2p_core::root::python_strip) {
            Some("Power save: on") => Some(true),
            Some("Power save: off") => Some(false),
            _ => None,
        }
    } else {
        None
    };
    (actual, answer)
}

fn run(root: &Root, argv: &[&str], timeout: f64) -> command::Outcome {
    command::run(root, argv, timeout)
}

/// The owner's Wi-Fi power-save policy (`/data/network/power-save.json`); without one, automatic
/// without standard power save.
#[derive(Deserialize)]
struct Policy {
    #[serde(default, deserialize_with = "lenient::or_none")]
    schema: Option<i64>,
    #[serde(default, deserialize_with = "lenient::text")]
    mode: Option<String>,
    #[serde(default, deserialize_with = "lenient::or_default")]
    automatic_standard: bool,
}

impl Default for Policy {
    fn default() -> Policy {
        Policy {
            schema: Some(1),
            mode: Some("automatic".into()),
            automatic_standard: false,
        }
    }
}

/// `/run/y2/wifi-power.json`: what the policy requested and what the driver reads back, for this
/// boot, network epoch and interface (`status wifi` shows it, capabilities and sleep audits read it).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct WifiPower {
    pub schema: i64,
    #[serde(deserialize_with = "lenient::text")]
    pub mode: Option<String>,
    #[serde(deserialize_with = "lenient::or_default")]
    pub requested: bool,
    /// cfg80211's power-save flag as read back (null when it did not read).
    #[serde(deserialize_with = "lenient::or_none")]
    pub enabled: Option<bool>,
    #[serde(deserialize_with = "lenient::or_default")]
    pub applied: bool,
    #[serde(deserialize_with = "lenient::text")]
    pub state: Option<String>,
    #[serde(deserialize_with = "lenient::text")]
    pub reason: Option<String>,
    #[serde(deserialize_with = "lenient::or_default")]
    pub provisional: bool,
    #[serde(deserialize_with = "lenient::text")]
    pub requested_driver_profile: Option<String>,
    #[serde(deserialize_with = "lenient::text")]
    pub epoch: Option<String>,
    #[serde(deserialize_with = "lenient::text")]
    pub ifindex: Option<String>,
    #[serde(deserialize_with = "lenient::or_none")]
    pub monotonic_s: Option<f64>,
    #[serde(deserialize_with = "lenient::text")]
    pub boot_id: Option<String>,
}

impl WifiPower {
    /// The record (its defaults when it is missing or no object).
    pub fn read(root: &Root) -> WifiPower {
        root.read("/run/y2/wifi-power.json")
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn ready(&self) -> bool {
        self.state.as_deref() == Some("Ready")
    }
}

/// Apply the Wi-Fi power-save policy (`heavy`: the service daemon saw a heavy transfer): the record
/// written to `/run/y2/wifi-power.json`, or the refusal of an invalid policy (nothing written).
pub fn wifi_policy(root: &Root, heavy: bool) -> Result<WifiPower, Failure> {
    // A policy that is no object (or no JSON) is none: the default.
    let policy: Policy = match root.json("/data/network/power-save.json") {
        Some(object @ serde_json::Value::Object(_)) => {
            serde_json::from_value(object).unwrap_or_default()
        }
        _ => Policy::default(),
    };
    let Some(mode) = policy
        .mode
        .as_deref()
        .filter(|m| ["off", "standard", "automatic"].contains(m))
    else {
        return Ok(invalid());
    };
    if policy.schema != Some(1) {
        return Ok(invalid());
    }
    let requested =
        mode == "standard" || (mode == "automatic" && policy.automatic_standard && !heavy);
    let (mut actual, mut result) = readback(root);
    let boot = root.boot_id();
    let epoch = root.read("/run/y2/wifi-epoch");
    let ifindex = root.read("/sys/class/net/wlan0/ifindex");
    let previous = WifiPower::read(root);
    let initialized = previous.ready()
        && previous.boot_id == boot
        && previous.epoch == epoch
        && previous.ifindex == ifindex
        && previous.requested == requested;
    let applied = actual != Some(requested) || !initialized;
    if applied {
        result = run(
            root,
            &[
                "/usr/sbin/iw",
                "dev",
                "wlan0",
                "set",
                "power_save",
                if requested { "on" } else { "off" },
            ],
            2.0,
        );
        actual = if result.ok { readback(root).0 } else { None };
    }
    let ready = actual == Some(requested);
    let value = WifiPower {
        schema: 1,
        mode: Some(mode.to_owned()),
        requested,
        enabled: actual,
        applied,
        state: Some(if ready { "Ready" } else { "Unavailable" }.into()),
        reason: (!ready).then(|| {
            result
                .reason
                .filter(|r| !r.is_empty())
                .unwrap_or_else(|| "power_save_readback_unavailable".into())
        }),
        provisional: true,
        requested_driver_profile: Some(if requested { "FAST_PSP" } else { "CAM" }.into()),
        epoch,
        ifindex,
        monotonic_s: Some(root.clock().monotonic()),
        boot_id: boot,
    };
    atomic_json(&root.path("/run/y2/wifi-power.json"), &value, false)?;
    Ok(value)
}

/// The answer to a policy of another shape (nothing applied or written).
fn invalid() -> WifiPower {
    WifiPower {
        state: Some("Failed".into()),
        reason: Some("invalid_wifi_power_policy".into()),
        ..WifiPower::default()
    }
}

/// `y2-platform wifi-power-policy`: exit 0 only when the readback equals the request.
pub fn command(root: &Root) -> Outcome {
    let value = wifi_policy(root, false)?;
    if value.ready() {
        Reply::unsorted(&value, 0)
    } else if value.schema == 0 {
        // The refusal of an invalid policy: its state and reason only.
        Reply::unsorted(
            &serde_json::json!({"state": value.state, "reason": value.reason}),
            1,
        )
    } else {
        Reply::unsorted(&value, 1)
    }
}

#[cfg(test)]
#[path = "radio_policy_tests.rs"]
mod radio_policy_tests;
