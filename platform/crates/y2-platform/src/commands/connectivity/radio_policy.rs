//! `radio_policy.wifi_policy`: the runtime Wi-Fi power-save policy (no invented firmware features).
//! `y2-platform wifi-power-policy` runs it without activity; the service daemon runs it in process every
//! 30 s while associated, with the coexistence record as activity.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::{atomic_json, Failure, Outcome, Reply};
use serde_json::{json, Map, Value};
use y2p_core::{
    command,
    json::{Object, ObjectExt},
    py::{eq_int, is_true, truthy},
    Root,
};

/// `iw dev wlan0 get power_save`: cfg80211's flag, or None, and the answer.
fn readback(root: &Root) -> (Option<bool>, command::Outcome) {
    let answer = run(
        root,
        &["/usr/sbin/iw", "dev", "wlan0", "get", "power_save"],
        3.0,
    );
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

/// `wifi_policy(ctx, activity)`: the record written to `/run/y2/wifi-power.json`, or the refusal of
/// an invalid policy (nothing written).
pub fn wifi_policy(root: &Root, activity: Option<&Map<String, Value>>) -> Result<Object, Failure> {
    let policy = match root.json("/data/network/power-save.json") {
        Some(Value::Object(map)) => map,
        _ => json!({"schema": 1, "mode": "automatic", "automatic_standard": false})
            .as_object()
            .cloned()
            .expect("object"),
    };
    let mode = policy.get("mode").and_then(Value::as_str);
    let Some(mode) = mode.filter(|m| ["off", "standard", "automatic"].contains(m)) else {
        return Ok(invalid());
    };
    if !eq_int(policy.get("schema"), 1) {
        return Ok(invalid());
    }
    let heavy = activity
        .and_then(|a| a.get("wifi_heavy_transfer"))
        .is_some_and(truthy);
    let requested = mode == "standard"
        || (mode == "automatic" && is_true(policy.get("automatic_standard")) && !heavy);
    // GET checks cfg80211's current state, including interface recreation and external changes.
    // Avoid resending CMD_ID_POWER_SAVE_MODE every 30 seconds when this same setting is already in
    // effect. Readback is host state, not proof of firmware sleep or electrical savings.
    let (mut actual, mut result) = readback(root);
    let boot = root.boot_id();
    let epoch = root.read("/run/y2/wifi-epoch");
    let ifindex = root.read("/sys/class/net/wlan0/ifindex");
    let previous = root.json_object("/run/y2/wifi-power.json");
    let same = |key: &str, value: &Option<String>| match (previous.get(key), value) {
        (None | Some(Value::Null), None) => true,
        (Some(Value::String(a)), Some(b)) => a == b,
        _ => false,
    };
    let initialized = previous.get("state").and_then(Value::as_str) == Some("Ready")
        && same("boot_id", &boot)
        && same("epoch", &epoch)
        && same("ifindex", &ifindex)
        && previous.get("requested") == Some(&Value::Bool(requested));
    // Establish policy once per association/interface; cfg80211's initial flag alone cannot
    // establish what firmware chose during interface creation.
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
    let value = Object::new()
        .with("schema", 1)
        .with("mode", mode)
        .with("requested", requested)
        .with("enabled", json!(actual))
        .with("applied", applied)
        .with("state", if ready { "Ready" } else { "Unavailable" })
        .with(
            "reason",
            if ready {
                Value::Null
            } else {
                json!(result
                    .reason
                    .filter(|r| !r.is_empty())
                    .unwrap_or_else(|| "power_save_readback_unavailable".into()))
            },
        )
        .with("provisional", true)
        .with(
            "requested_driver_profile",
            if requested { "FAST_PSP" } else { "CAM" },
        )
        .with("epoch", json!(epoch))
        .with("ifindex", json!(ifindex))
        .with("monotonic_s", root.clock().monotonic())
        .with("boot_id", json!(boot));
    // No private_directory: a missing /run/y2 is an OSError (main.py's record).
    atomic_json(&root.path("/run/y2/wifi-power.json"), &value, false)?;
    Ok(value)
}

fn invalid() -> Object {
    Object::new()
        .with("state", "Failed")
        .with("reason", "invalid_wifi_power_policy")
}

/// `y2-platform wifi-power-policy`: exit 0 only when the readback equals the request.
pub fn command(root: &Root) -> Outcome {
    let value = wifi_policy(root, None)?;
    let ready = value.get("state").and_then(Value::as_str) == Some("Ready");
    Reply::unsorted(&value, if ready { 0 } else { 1 })
}

#[cfg(test)]
#[path = "radio_policy_tests.rs"]
mod radio_policy_tests;
