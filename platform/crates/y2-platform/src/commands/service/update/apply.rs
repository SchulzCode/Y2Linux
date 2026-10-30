//! `update apply`, `rollback`, `cancel` and `probe-recovery`: the battery gate, the maintenance
//! lease, the core's request and the restart through the power daemon.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    core, journal_state, remove_package, JournalState, Object, Raise, Result, Root, Value,
    MINIMUM_PERCENT,
};
use crate::commands::power::published::{Battery, PowerState};
use serde::Deserialize;
use y2p_core::json::ObjectExt;
use y2p_core::lenient::{self, BootId};

/// Restart into the installer and back (proven on 43 and 45). Owner override:
/// /data/updates/restart.json {"mode": "poweroff"} turns off instead, and Power starts the installer and
/// the new system.
pub(super) fn restart_mode(root: &Root) -> &'static str {
    let restart: Restart = lenient::read(root.json_object("/data/updates/restart.json"));
    if restart.mode.as_deref() == Some("poweroff") {
        "poweroff"
    } else {
        "reboot"
    }
}

/// /data/updates/restart.json, the owner's override.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Restart {
    #[serde(deserialize_with = "lenient::text")]
    mode: Option<String>,
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

/// The restart (or power-off) through the power daemon, its intent as the answer.
fn rpc(root: &Root, action: &str) -> Result<Value> {
    let request = Object::new()
        .with("action", action)
        .with("reason", "update");
    let intent = crate::commands::power::client::rpc(root, &request)?;
    serde_json::to_value(intent).map_err(|e| Raise::Value(e.to_string()))
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
        if !rollback && journal_state(root) != JournalState::Queued {
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
