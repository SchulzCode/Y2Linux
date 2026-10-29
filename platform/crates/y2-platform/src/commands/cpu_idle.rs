//! `y2-platform cpu-idle-policy start|stop` (`cpu_idle.apply`): one boot-time admission of the
//! physically qualified CPU0 dormant policy. This only selects existing cpuidle/SPM controls; every
//! entry still passes the kernel's topology, workload, OPP, deadline, UART ACK and restoration guards.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::{atomic_json, create, mkdir_parents, write_text, Failure, Outcome, Reply};
use serde_json::{json, Map, Value};
use std::io;
use y2p_core::Root;

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
fn disable(root: &Root) -> Vec<Value> {
    let mut errors = Vec::new();
    for (path, value) in [(BUDGET.to_owned(), "0"), (disable_state(), "1")] {
        match write(root, &path, value) {
            Ok(()) if root.read(&path).as_deref() != Some(value) => {
                errors.push(json!(format!("{path}:readback")))
            }
            Ok(()) => {}
            Err(_) => errors.push(json!(format!("{path}:write"))),
        }
    }
    errors
}

fn kernel_trusted(root: &Root) -> bool {
    root.integer("/proc/sys/kernel/tainted")
        .is_some_and(|taint| taint & !TAINT_WARN == 0)
}

/// Fail closed, run once per boot, and never override later manual policy.
pub fn apply(root: &Root, action: &str) -> Outcome {
    mkdir_parents(&root.path("/run/y2"), 0o777)?;
    let boot = root.read("/proc/sys/kernel/random/boot_id");
    let mut result = Map::new();
    result.insert("schema".into(), json!("org.y2linux.cpu-idle-policy/v1"));
    result.insert("boot_id".into(), json!(boot));
    result.insert("policy".into(), json!("qualified_CPU0_DORMANT"));
    result.insert("state".into(), json!("Skipped"));
    let mut reasons: Vec<Value> = Vec::new();
    let lock = root.path(LOCK);
    if action == "start" {
        match create(&lock, true) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                result.insert("reasons".into(), json!(["already_applied_this_boot"]));
                result.insert("previous".into(), Value::Object(root.json_object(RECORD)));
                return finish(result);
            }
            Err(e) => return Err(Failure::os(e, &lock)),
        }
        let versions = root.json_object("/etc/y2linux/versions.json");
        let osrelease = root.read("/proc/sys/kernel/osrelease");
        if boot.as_deref().is_none_or(str::is_empty)
            || versions
                .get("kernel_version")
                .cloned()
                .unwrap_or(Value::Null)
                != json!(osrelease)
        {
            reasons.push(json!("installed_kernel_identity"));
        }
        if !kernel_trusted(root) {
            reasons.push(json!("kernel_tainted_or_unknown"));
        }
        if root
            .read("/sys/devices/system/cpu/cpuidle/current_driver")
            .as_deref()
            != Some("mt6582-idle")
        {
            reasons.push(json!("cpuidle_driver"));
        }
        if root.read(&format!("{STATE}/name")).as_deref() != Some("DORMANT") {
            reasons.push(json!("dormant_state_unavailable_or_boot_disabled"));
        }
        let preflight = root.read(PREFLIGHT).unwrap_or_default();
        let lines: Vec<&str> = preflight.lines().collect();
        reasons.extend(
            FOUNDATIONS
                .iter()
                .filter(|n| !lines.contains(&format!("prerequisite_{n}=1").as_str()))
                .map(|n| json!(format!("foundation:{n}"))),
        );
        if !reasons.is_empty() {
            // Initial controls remain quarantined; respect explicit boot options.
            result.insert("reasons".into(), json!(reasons));
            atomic_json(&root.path(RECORD), &result, false)?;
            return finish(result);
        }
        if root.read(BUDGET).as_deref() != Some("0")
            || root.read(&disable_state()).as_deref() != Some("1")
        {
            result.insert("reasons".into(), json!(["existing_owner_runtime_policy"]));
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
                result.insert("state".into(), json!("Enabled"));
            }
            Err(error) => {
                result.insert("state".into(), json!("Failed"));
                reasons = vec![json!(error.message())];
                result.insert("rollback_errors".into(), json!(disable(root)));
            }
        }
    } else {
        // Reserve boot ownership so a subsequent start cannot undo a manual stop.
        create(&lock, false).map_err(|e| Failure::os(e, &lock))?;
        reasons = disable(root);
        result.insert(
            "state".into(),
            json!(if reasons.is_empty() {
                "Disabled"
            } else {
                "Failed"
            }),
        );
    }
    result.insert("reasons".into(), json!(reasons));
    result.insert("budget".into(), json!(root.read(BUDGET)));
    result.insert("disabled".into(), json!(root.read(&disable_state())));
    if let Err(error) = atomic_json(&root.path(RECORD), &result, false) {
        if result.get("state") == Some(&json!("Enabled")) {
            disable(root);
        }
        return Err(error);
    }
    finish(result)
}

fn finish(result: Map<String, Value>) -> Outcome {
    let failed = result.get("state") == Some(&json!("Failed"));
    Reply::sorted(&result, u8::from(failed))
}

#[cfg(test)]
#[path = "cpu_idle_tests.rs"]
mod cpu_idle_tests;
