//! `health.check`: bounded observational health; full scratch checks are explicit.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    snapshot::snapshot,
    util::{self, int, read},
};
use crate::reply::{Failure, Outcome, Reply};
use serde_json::{json, Map, Value};
use y2p_core::py::PyValue;
use y2p_core::{command, Root};

/// `health.check(ctx, full)` as a value.
pub fn check(root: &Root, full: bool) -> Result<Map<String, Value>, Failure> {
    let status = snapshot(root, &[], false, 0.0)?;
    let mut checks: Vec<Map<String, Value>> = Vec::new();
    let mut add = |name: &str, state: &str, reason: Value, details: Option<Map<String, Value>>| {
        let mut check = Map::new();
        check.insert("name".into(), json!(name));
        check.insert("state".into(), json!(state));
        check.insert("reason".into(), reason);
        check.extend(details.unwrap_or_default());
        checks.push(check);
    };
    let empty = Vec::new();
    for volume in status["storage"]["volumes"].as_array().unwrap_or(&empty) {
        let name = match volume["path"].as_str() {
            Some("/") => "root",
            Some("/data") => "data",
            Some("/media/sd") => "sd",
            _ => "internal_media",
        };
        let mut state = match volume["state"].as_str() {
            Some("Ready") => "OK",
            Some("Degraded") => "DEGRADED",
            Some("Failed") => "FAILED",
            _ => "UNAVAILABLE",
        };
        let space = &volume["space_state"];
        if matches!(space.as_str(), Some("LowSpace" | "CriticalSpace")) {
            state = "DEGRADED";
        }
        let reason = match volume.get("reason") {
            Some(r) if y2p_core::py::truthy(r) => r.clone(),
            _ => space.clone(),
        };
        add(name, state, reason, None);
    }
    let mut errors = Vec::new();
    let mut unreadable = Vec::new();
    let sources = root.glob("/sys/fs/ext4/*/errors_count");
    for path in &sources {
        match read(path).as_deref().and_then(int) {
            Some(count) if count >= 0 => {
                if count != 0 {
                    errors.push(json!({"filesystem": util::parent_name(path), "errors": count}));
                }
            }
            _ => unreadable.push(json!(util::parent_name(path))),
        }
    }
    let available = !sources.is_empty() && unreadable.is_empty();
    let mut details = Map::new();
    let failed = !errors.is_empty();
    details.insert("errors".into(), Value::Array(errors));
    details.insert("unreadable".into(), Value::Array(unreadable));
    add(
        "filesystem_errors",
        if failed {
            "FAILED"
        } else if available {
            "OK"
        } else {
            "UNAVAILABLE"
        },
        json!(if failed {
            "kernel_reported_errors"
        } else if available {
            "no_reported_ext4_errors"
        } else {
            "counter_unavailable"
        }),
        Some(details),
    );
    let non_empty = |v: &Value| v.as_array().is_some_and(|a| !a.is_empty());
    let thermal = status["thermal"]["zones"]
        .as_array()
        .is_some_and(|z| z.iter().any(|z| !z["temperature_millicelsius"].is_null()));
    for (name, present) in [
        ("render_node", !root.glob("/dev/dri/renderD*").is_empty()),
        ("display", !root.glob("/dev/dri/card*").is_empty()),
        ("audio", !root.glob("/dev/snd/pcm*p").is_empty()),
        ("input", !root.glob("/dev/input/event*").is_empty()),
        ("rtc", non_empty(&status["system"]["rtc"])),
        ("thermal", thermal),
        ("power_supply", non_empty(&status["power"]["supplies"])),
        ("usb", non_empty(&status["system"]["usb"]["udcs"])),
    ] {
        add(
            name,
            if present { "OK" } else { "UNAVAILABLE" },
            json!(if present {
                "observed"
            } else {
                "interface_missing"
            }),
            None,
        );
    }
    for name in ["wifi", "bluetooth", "update"] {
        let item = &status["readiness"][name];
        let state = match item["state"].as_str() {
            Some("Ready") => "OK",
            Some("Starting" | "Degraded") => "DEGRADED",
            Some("Failed") => "FAILED",
            Some("Unavailable") => "UNAVAILABLE",
            // `{...}[item['state']]`: a KeyError for any other state, a TypeError for a list or dict.
            _ => {
                return Err(match &item["state"] {
                    Value::String(state) => Failure::key_error(state),
                    state @ (Value::Array(_) | Value::Object(_)) => Failure::type_error(format!(
                        "unhashable type: '{}'",
                        PyValue::type_name(state)
                    )),
                    Value::Null => Failure::crash("KeyError", "None"),
                    Value::Bool(b) => Failure::crash("KeyError", if *b { "True" } else { "False" }),
                    number => Failure::crash("KeyError", number.to_string()),
                })
            }
        };
        add(name, state, item["reason"].clone(), None);
    }
    let timers = &status["cpu"]["timer"];
    for (name, key) in [
        ("highres_timer", "highres_active"),
        ("tickless_idle", "no_hz_active"),
    ] {
        let (state, reason) = match timers[key].as_bool() {
            Some(true) => ("OK", "runtime_observed"),
            Some(false) => ("DEGRADED", "legacy_fallback"),
            None => ("UNAVAILABLE", "runtime_unavailable"),
        };
        add(name, state, json!(reason), None);
    }
    if full {
        // Queries only: no render master takeover, ALSA playback, radio action or reboot.
        for (name, argv) in [
            ("alsa_query", &["y2-audio-contract"][..]),
            ("render_query", &["modetest", "-M", "mediatek", "-c"][..]),
        ] {
            let answer = command::run(root, argv, 2.0);
            add(
                name,
                if answer.ok { "OK" } else { "UNAVAILABLE" },
                json!(answer
                    .reason
                    .filter(|r| !r.is_empty())
                    .unwrap_or_else(|| "query_completed".into())),
                None,
            );
        }
    }
    let state = ["FAILED", "DEGRADED", "UNAVAILABLE", "OK"]
        .into_iter()
        .find(|s| checks.iter().any(|c| c["state"] == *s))
        .unwrap_or("OK");
    let mut parameters = Map::new();
    parameters.insert("mode".into(), json!(if full { "full" } else { "quick" }));
    let mut record = util::record(root, "health", parameters);
    record.insert("result".into(), json!(state));
    record.insert(
        "failure".into(),
        Value::Array(
            checks
                .iter()
                .filter(|c| c["state"] == "FAILED")
                .map(|c| c["name"].clone())
                .collect(),
        ),
    );
    let mut result = Map::new();
    result.insert("schema".into(), json!(y2p_contract::schema::HEALTH));
    result.insert("record".into(), Value::Object(record));
    result.insert("state".into(), json!(state));
    result.insert(
        "checks".into(),
        Value::Array(checks.into_iter().map(Value::Object).collect()),
    );
    Ok(result)
}

/// `y2-platform health [--json] [--full]`: exit 1 for FAILED (Reborn reads it as evidence).
pub fn run(root: &Root, full: bool) -> Outcome {
    let result = check(root, full)?;
    let failed = result["state"] == "FAILED";
    Reply::sorted(&result, u8::from(failed))
}

#[cfg(test)]
#[path = "health_tests.rs"]
mod health_tests;
