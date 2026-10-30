//! `y2-platform cpu-idle-policy start|stop` (`cpu_idle.apply`): one boot-time admission of the
//! physically qualified CPU0 dormant policy. This only selects existing cpuidle/SPM controls; every
//! entry still passes the kernel's topology, workload, OPP, deadline, UART ACK and restoration guards.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::At;
use crate::reply::{atomic_json, create, mkdir_parents, write_text, Failure, Outcome, Reply};
use serde::{Deserialize, Serialize};
use std::io;
use y2p_core::{lenient, Root};

const STATE: &str = "/sys/devices/system/cpu/cpu0/cpuidle/state2";
const BUDGET: &str = "/sys/module/spm/parameters/dormant_budget";
const PREFLIGHT: &str = "/sys/devices/platform/10006000.power-controller/dormant_preflight";
const RECORD: &str = "/run/y2/cpu-idle-policy.json";
const LOCK: &str = "/run/y2/cpu-idle-policy.lock";
/// Other prerequisites describe transient activity and remain entry-time guards.
const FOUNDATIONS: [&str; 8] = [
    "system_running",
    "boot_policy",
    "spm",
    "local_events",
    "cirq",
    "linux_context",
    "timer_context",
    "broken",
];
/// TAINT_WARN alone (F26: one cfg80211 WARN kept the player awake for hours) still trusts the kernel.
const TAINT_WARN: i64 = 1 << 9;

fn disable_state() -> String {
    format!("{STATE}/disable")
}

fn write(root: &Root, path: &str, value: &str) -> Result<(), Failure> {
    write_text(&root.path(path), &format!("{value}\n"))
}

/// Close admission first, then quarantine the state. Do not change C1/C2.
fn disable(root: &Root) -> Vec<String> {
    let mut errors = Vec::new();
    for (path, value) in [(BUDGET.to_owned(), "0"), (disable_state(), "1")] {
        match write(root, &path, value) {
            Ok(()) if root.read(&path).as_deref() != Some(value) => {
                errors.push(format!("{path}:readback"))
            }
            Ok(()) => {}
            Err(_) => errors.push(format!("{path}:write")),
        }
    }
    errors
}

/// The policy record (/run/y2/cpu-idle-policy.json), also what the command prints; read back
/// leniently (`previous` of a second start this boot).
#[derive(Serialize, Deserialize, Default, Debug)]
#[serde(default)]
struct Policy {
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    schema: Option<String>,
    /// `null` for an unreadable boot id.
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::there"
    )]
    boot_id: Option<Option<String>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    policy: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    state: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::texts"
    )]
    reasons: Option<Vec<String>>,
    /// The record of the start that already ran this boot.
    #[serde(skip_serializing_if = "Option::is_none", skip_deserializing)]
    previous: Option<Box<Policy>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::texts"
    )]
    rollback_errors: Option<Vec<String>>,
    /// The controls as they read at the end (`null`: unreadable).
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::there"
    )]
    budget: Option<Option<String>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::there"
    )]
    disabled: Option<Option<String>>,
}

fn kernel_trusted(root: &Root) -> bool {
    root.integer("/proc/sys/kernel/tainted")
        .is_some_and(|taint| taint & !TAINT_WARN == 0)
}

/// Fail closed, run once per boot, and never override later manual policy.
pub fn apply(root: &Root, action: &str) -> Outcome {
    mkdir_parents(&root.path("/run/y2"), 0o777)?;
    let boot = root.boot_id();
    let mut result = Policy {
        schema: Some("org.y2linux.cpu-idle-policy/v1".into()),
        boot_id: Some(boot.clone()),
        policy: Some("qualified_CPU0_DORMANT".into()),
        state: Some("Skipped".into()),
        ..Policy::default()
    };
    let mut reasons: Vec<String> = Vec::new();
    let lock = root.path(LOCK);
    if action == "start" {
        match create(&lock, true) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                result.reasons = Some(vec!["already_applied_this_boot".into()]);
                result.previous = Some(Box::new(lenient::read(root.json_object(RECORD))));
                return finish(result);
            }
            Err(e) => return Err(Failure::os(e, &lock)),
        }
        let versions: Versions = lenient::read(root.json_object("/etc/y2linux/versions.json"));
        let osrelease = root.read("/proc/sys/kernel/osrelease");
        if boot.as_deref().is_none_or(str::is_empty) || versions.kernel_version != osrelease {
            reasons.push("installed_kernel_identity".into());
        }
        if !kernel_trusted(root) {
            reasons.push("kernel_tainted_or_unknown".into());
        }
        if root
            .read("/sys/devices/system/cpu/cpuidle/current_driver")
            .as_deref()
            != Some("mt6582-idle")
        {
            reasons.push("cpuidle_driver".into());
        }
        if root.read(&format!("{STATE}/name")).as_deref() != Some("DORMANT") {
            reasons.push("dormant_state_unavailable_or_boot_disabled".into());
        }
        let preflight = root.read(PREFLIGHT).unwrap_or_default();
        let lines: Vec<&str> = preflight.lines().collect();
        reasons.extend(
            FOUNDATIONS
                .iter()
                .filter(|n| !lines.contains(&format!("prerequisite_{n}=1").as_str()))
                .map(|n| format!("foundation:{n}")),
        );
        if !reasons.is_empty() {
            // Initial controls remain quarantined; respect explicit boot options.
            result.reasons = Some(reasons);
            atomic_json(&root.path(RECORD), &result, false)?;
            return finish(result);
        }
        if root.read(BUDGET).as_deref() != Some("0")
            || root.read(&disable_state()).as_deref() != Some("1")
        {
            result.reasons = Some(vec!["existing_owner_runtime_policy".into()]);
            atomic_json(&root.path(RECORD), &result, false)?;
            return finish(result);
        }
        // State remains disabled until the qualified budget is read back.
        let enabled = write(root, BUDGET, "-1")
            .and_then(|()| {
                if root.read(BUDGET).as_deref() == Some("-1") {
                    Ok(())
                } else {
                    Err(Failure::os_message("dormant budget readback"))
                }
            })
            .and_then(|()| write(root, &disable_state(), "0"))
            .and_then(|()| {
                if root.read(&disable_state()).as_deref() == Some("0") {
                    Ok(())
                } else {
                    Err(Failure::os_message("cpuidle state readback"))
                }
            });
        match enabled {
            Ok(()) => {
                result.state = Some("Enabled".into());
            }
            Err(error) => {
                result.state = Some("Failed".into());
                reasons = vec![error.message()];
                result.rollback_errors = Some(disable(root));
            }
        }
    } else {
        // Reserve boot ownership so a subsequent start cannot undo a manual stop.
        create(&lock, false).at(&lock)?;
        reasons = disable(root);
        result.state = Some(
            if reasons.is_empty() {
                "Disabled"
            } else {
                "Failed"
            }
            .into(),
        );
    }
    result.reasons = Some(reasons);
    result.budget = Some(root.read(BUDGET));
    result.disabled = Some(root.read(&disable_state()));
    if let Err(error) = atomic_json(&root.path(RECORD), &result, false) {
        if result.state.as_deref() == Some("Enabled") {
            disable(root);
        }
        return Err(error);
    }
    finish(result)
}

/// /etc/y2linux/versions.json: the kernel the build installed.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Versions {
    #[serde(deserialize_with = "lenient::text")]
    kernel_version: Option<String>,
}

fn finish(result: Policy) -> Outcome {
    let failed = result.state.as_deref() == Some("Failed");
    Reply::sorted(&result, u8::from(failed))
}

#[cfg(test)]
#[path = "cpu_idle_tests.rs"]
mod cpu_idle_tests;
