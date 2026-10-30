//! `update apply`, `rollback`, `cancel` and `probe-recovery`: the battery gate, the maintenance
//! lease, the core's request and the restart through the power daemon.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    core, json, json_error, py, remove_package, sorted, state_is, status, truthy, Duration, Object,
    Raise, Result, Root, Value, MINIMUM_PERCENT,
};
use crate::commands::power::published::{Battery, PowerState};
use std::io::Read;
use std::io::Write;
use y2p_core::json::ObjectExt;
use y2p_core::lenient::BootId;
use y2p_core::py::PyValue;

/// Restart into the installer and back (proven on 43 and 45). Owner override:
/// /data/updates/restart.json {"mode": "poweroff"} turns off instead, and Power starts the installer and
/// the new system.
pub(super) fn restart_mode(root: &Root) -> &'static str {
    if root.json_object("/data/updates/restart.json").get("mode") == Some(&json!("poweroff")) {
        "poweroff"
    } else {
        "reboot"
    }
}

/// Charger connected, or at least MINIMUM_PERCENT from this boot's estimate.
pub(super) fn battery_gate(root: &Root) -> Result<()> {
    let power = PowerState::read(root);
    // A record without an owner counts; one of another boot reads as no battery estimate.
    let battery = match power.boot_id {
        BootId::Null => power.battery.reading(),
        _ if power.is_this_boot(root) => power.battery.reading(),
        _ => Battery::Absent.reading(),
    };
    if battery.charging_or_full() {
        return Ok(());
    }
    match battery.percent {
        Some(percent) if percent >= MINIMUM_PERCENT => Ok(()),
        _ => Err(Raise::value("update_needs_charger_or_half_battery")),
    }
}

/// One request to the power daemon's socket.
fn rpc(root: &Root, action: &str) -> Result<Value> {
    use std::os::unix::net::UnixStream;
    let request = Object::new()
        .with("action", action)
        .with("reason", "update");
    let data = y2p_core::json::dumps(&request, y2p_core::json::Separators::Spaced)
        .map_err(|e| Raise::Value(e.to_string()))?
        + "\n";
    if data.len() > 2048 {
        return Err(Raise::value("request_too_large"));
    }
    let os = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
            Raise::Os("timed out".into())
        }
        _ => Raise::Os(py::os_error(&e, None).0),
    };
    let mut client = UnixStream::connect(root.path("/run/y2/power.sock")).map_err(os)?;
    client
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(os)?;
    client
        .set_write_timeout(Some(Duration::from_secs(1)))
        .map_err(os)?;
    client.write_all(data.as_bytes()).map_err(os)?;
    let mut answer = Vec::new();
    while !answer.contains(&b'\n') {
        let mut part = [0u8; 4096];
        let n = client.read(&mut part).map_err(os)?;
        if n == 0 || answer.len() + n > 8192 {
            return Err(Raise::value("invalid_power_response"));
        }
        answer.extend_from_slice(&part[..n]);
    }
    let text = String::from_utf8_lossy(&answer).into_owned();
    let result: Value = serde_json::from_str(text.trim_end_matches(['\n', '\r', ' ', '\t']))
        .map_err(|e| Raise::Value(json_error(&text, &e)))?;
    let Value::Object(result) = result else {
        return Err(Raise::value("invalid_power_response"));
    };
    if !result.get("ok").is_some_and(truthy) {
        return Err(Raise::Value(result.get("error").map_or_else(
            || "power_request_failed".into(),
            |e| sorted(e.clone()).py_str(),
        )));
    }
    result
        .get("result")
        .cloned()
        .ok_or_else(|| Raise::value("invalid_power_response"))
}

/// `function` under the `Maintenance` workload lease (commands::workload).
pub(super) fn maintenance<T>(root: &Root, function: impl FnOnce() -> Result<T>) -> Result<T> {
    let _lease = crate::commands::workload::Lease::take(root, "Maintenance")?;
    function()
}

/// Install (or restore) in the Y2 recovery image: LK starts RECOVERY from the MISC
/// request on the restart (or, with the owner's override, on the next Power press).
pub(super) fn apply(root: &Root, rollback: bool) -> Result<Value> {
    maintenance(root, || {
        if !rollback && !state_is(&status(root), &["Queued"]) {
            return Err(Raise::value("no_verified_queued_update"));
        }
        // Before the core's rollback: a battery refusal must leave the journal as it was, not
        // RollbackPending with no restart to follow.
        battery_gate(root)?;
        if rollback {
            core(root, "rollback", None, 15.0)?;
        }
        core(root, "prepare", None, 120.0)?;
        rpc(root, restart_mode(root))
    })
}

/// Owner test: enter the recovery image, which records that it ran and shows its
/// screen, then returns like an install (restart by default).
pub(super) fn probe(root: &Root) -> Result<Value> {
    maintenance(root, || {
        core(root, "probe", None, 60.0)?;
        rpc(root, restart_mode(root))
    })
}

/// Cancel a queued update and free its package right away (OTA-46).
pub(super) fn cancel_update(root: &Root) -> Result<Value> {
    let result = core(root, "cancel", None, 15.0)?;
    let _ = remove_package(&root.path("/data/updates/pending"));
    Ok(Value::Object(result))
}
