//! `y2-platform health`: each subsystem as OK, DEGRADED, UNAVAILABLE or FAILED from one snapshot;
//! `--full` adds the ALSA and DRM queries (queries only, no playback or render takeover).
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    snapshot::{snapshot, Snapshot},
    util::{self, int, read},
};
use crate::reply::{Failure, Outcome, Reply};
use serde_json::{json, Map, Value};
use y2p_contract::CheckState;
use y2p_core::volume::{SpaceState, VolumeState};
use y2p_core::{command, Root};

/// The health record: every check and the worst state among them.
pub fn check(root: &Root, full: bool) -> Result<Map<String, Value>, Failure> {
    check_from(root, &snapshot(root, &[], false, 0.0)?, full)
}

/// The health record of a snapshot the caller already took (diagnostics export takes one for both).
pub fn check_from(
    root: &Root,
    status: &Snapshot,
    full: bool,
) -> Result<Map<String, Value>, Failure> {
    let mut checks: Vec<Map<String, Value>> = Vec::new();
    let mut states: Vec<CheckState> = Vec::new();
    let mut add =
        |name: &str, state: CheckState, reason: Value, details: Option<Map<String, Value>>| {
            let mut check = Map::new();
            check.insert("name".into(), json!(name));
            check.insert("state".into(), json!(state));
            states.push(state);
            check.insert("reason".into(), reason);
            check.extend(details.unwrap_or_default());
            checks.push(check);
        };
    for volume in &status.storage.volumes {
        let name = match volume.path() {
            "/" => "root",
            "/data" => "data",
            "/media/sd" => "sd",
            _ => "internal_media",
        };
        let mut state = match volume.state {
            VolumeState::Ready => CheckState::Ok,
            VolumeState::Degraded => CheckState::Degraded,
            VolumeState::Failed => CheckState::Failed,
            VolumeState::Unavailable => CheckState::Unavailable,
        };
        if matches!(
            volume.space_state,
            SpaceState::LowSpace | SpaceState::CriticalSpace
        ) {
            state = CheckState::Degraded;
        }
        let reason = match volume.reason {
            Some(reason) if !reason.is_empty() => json!(reason),
            _ => json!(volume.space_state),
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
            CheckState::Failed
        } else if available {
            CheckState::Ok
        } else {
            CheckState::Unavailable
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
    let thermal = status
        .thermal
        .zones
        .iter()
        .any(|zone| zone.temperature_millicelsius.is_some());
    for (name, present) in [
        ("render_node", !root.glob("/dev/dri/renderD*").is_empty()),
        ("display", !root.glob("/dev/dri/card*").is_empty()),
        ("audio", !root.glob("/dev/snd/pcm*p").is_empty()),
        ("input", !root.glob("/dev/input/event*").is_empty()),
        ("rtc", !status.system.rtc.is_empty()),
        ("thermal", thermal),
        ("power_supply", !status.power.supplies.is_empty()),
        ("usb", !status.system.usb.udcs.is_empty()),
    ] {
        add(
            name,
            if present {
                CheckState::Ok
            } else {
                CheckState::Unavailable
            },
            json!(if present {
                "observed"
            } else {
                "interface_missing"
            }),
            None,
        );
    }
    for name in ["wifi", "bluetooth", "update"] {
        let item = &status.readiness[name];
        let state = match item["state"].as_str() {
            Some("Ready") => CheckState::Ok,
            Some("Starting" | "Degraded") => CheckState::Degraded,
            Some("Failed") => CheckState::Failed,
            Some("Unavailable") => CheckState::Unavailable,
            // The snapshot's readiness knows no other state.
            _ => return Err(Failure::value("invalid_readiness_state")),
        };
        add(name, state, item["reason"].clone(), None);
    }
    let timers = status.cpu.get("timer").unwrap_or(&Value::Null);
    for (name, key) in [
        ("highres_timer", "highres_active"),
        ("tickless_idle", "no_hz_active"),
    ] {
        let (state, reason) = match timers[key].as_bool() {
            Some(true) => (CheckState::Ok, "runtime_observed"),
            Some(false) => (CheckState::Degraded, "legacy_fallback"),
            None => (CheckState::Unavailable, "runtime_unavailable"),
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
                if answer.ok {
                    CheckState::Ok
                } else {
                    CheckState::Unavailable
                },
                json!(answer
                    .reason
                    .filter(|r| !r.is_empty())
                    .unwrap_or_else(|| "query_completed".into())),
                None,
            );
        }
    }
    // The worst state among the checks: FAILED, then DEGRADED, then UNAVAILABLE.
    let state = [
        CheckState::Failed,
        CheckState::Degraded,
        CheckState::Unavailable,
    ]
    .into_iter()
    .find(|s| states.contains(s))
    .unwrap_or(CheckState::Ok);
    let mut parameters = Map::new();
    parameters.insert("mode".into(), json!(if full { "full" } else { "quick" }));
    let mut record = util::record(root, "health", parameters);
    record.insert("result".into(), json!(state));
    record.insert(
        "failure".into(),
        Value::Array(
            checks
                .iter()
                .zip(&states)
                .filter(|(_, s)| **s == CheckState::Failed)
                .map(|(c, _)| c["name"].clone())
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
    let failed = result.get("state").is_some_and(|state| state == "FAILED");
    Reply::sorted(&result, u8::from(failed))
}

#[cfg(test)]
#[path = "health_tests.rs"]
mod health_tests;
