//! `product_sleep.py`'s gates: what must hold before a product sleep is acknowledged (`sleep
//! request`) and again under the inherited lock (the helper), with the quarantine guard and the
//! kernel-refusal retry gate. Also the two pieces of other modules the gates read: `power.load_policy`
//! and `update.status(ctx).get('state')`.
// SPDX-License-Identifier: GPL-2.0-only
use super::{is_text, py_gt, text_value, wall, E};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::{fs::Metadata, os::unix::fs::MetadataExt, path::Path};
use y2p_core::py::PyValue;
use y2p_core::{
    py::{exact_int, is_true, truthy},
    root::read,
    suspend_text::{field, fields, split, splitlines, Regex},
    Root,
};

pub const GUARD: &str = "/data/system/platform/product-sleep-guard.json";
// A kernel refusal before any task froze leaves nothing unproven; the next request waits this long
// instead of the image being quarantined.
pub const RETRY: &str = "/run/y2/product-sleep-retry.json";
const PREPARE_RETRY_SECONDS: u64 = 120;
// Stage marks of a kernel attempt that ended before freezing tasks: only the PM_SUSPEND_PREPARE
// notifiers and the filesystem sync ran, no device callback.
const PRE_FREEZE_MARKS: [&str; 5] = [
    "HELPER_REQUEST",
    "SUSPEND_REQUEST",
    "BACKSTOP_STARTED",
    "FILESYSTEM_SYNCED",
    "EXIT",
];
// TAINT_WARN alone: a kernel WARN this boot (F26: one cfg80211 WARN kept the player awake for
// hours). Every other bit (probe modules, forced loads, a machine check) still marks the kernel as
// not trusted for sleep or deep idle.
const TAINT_WARN: i64 = 1 << 9;

fn refused(reason: &str) -> E {
    E::refused(reason)
}

pub fn spm_state(root: &Root) -> Map<String, Value> {
    let paths = root.glob("/sys/bus/platform/devices/10006000*/state");
    if paths.len() != 1 {
        return Map::new();
    }
    fields(read(&paths[0], y2p_core::root::READ_LIMIT).as_deref())
}

pub fn identity(root: &Root) -> Value {
    let versions = root.json_object("/etc/y2linux/versions.json");
    let mut value = Map::new();
    for key in ["build_id", "build_git_commit", "reborn_source_commit"] {
        value.insert(
            key.into(),
            versions.get(key).cloned().unwrap_or(Value::Null),
        );
    }
    value.insert(
        "kernel".into(),
        text_value(root.read("/proc/sys/kernel/osrelease")),
    );
    Value::Object(value)
}

fn kernel_trusted(root: &Root) -> bool {
    root.integer("/proc/sys/kernel/tainted")
        .is_some_and(|taint| taint & !TAINT_WARN == 0)
}

/// `type(value) is int and boottime_ns < value`.
fn before(value: Option<&Value>, now: u64) -> bool {
    match value {
        Some(Value::Number(n)) if n.is_i64() || n.is_u64() => n
            .as_i64()
            .map(|v| i128::from(now) < i128::from(v))
            .or_else(|| n.as_u64().map(|v| now < v))
            .unwrap_or(false),
        _ => false,
    }
}

pub fn admission(root: &Root) -> Result<(), E> {
    if root.read("/sys/class/power_supply/BAT0/status").as_deref() == Some("Charging") {
        return Err(refused("charger_active"));
    }
    let spm = spm_state(root);
    if field(&spm, "system_sleep_abi") != Some("1") {
        return Err(refused("physical_qualification_required"));
    }
    if !kernel_trusted(root) || field(&spm, "broken") != Some("0") {
        return Err(refused("kernel_sleep_unavailable"));
    }
    if !split(&root.read("/sys/power/state").unwrap_or_default()).contains(&"mem") {
        return Err(refused("kernel_sleep_unavailable"));
    }
    if !root
        .read("/sys/power/pm_test")
        .unwrap_or_default()
        .contains("[none]")
    {
        return Err(refused("pm_test_in_progress"));
    }
    let backstop = fields(root.read("/sys/firmware/y2_pm/backstop_s").as_deref());
    if field(&backstop, "provider") != Some("1") {
        return Err(refused("kernel_sleep_unavailable"));
    }
    if field(&backstop, "armed_s") != Some("0") || field(&backstop, "running") != Some("0") {
        return Err(refused("pm_test_in_progress"));
    }
    let retry = root.json_object(RETRY);
    if retry
        .get("boot_id")
        .unwrap_or(&Value::Null)
        .py_eq(&Value::from(root.boot_id()))
        && before(retry.get("not_before_ns"), root.clock().boottime_ns())
    {
        return Err(refused("kernel_prepare_retry_wait"));
    }
    let prior = root.json_object(GUARD);
    // Only an attempt that reached the kernel can leave an unproven sleep. A pending record written
    // before kernel entry (radio quiesce, Reborn) is a helper failure, not a kernel quarantine; 12
    // kept those across reboots.
    if prior
        .get("identity")
        .unwrap_or(&Value::Null)
        .py_eq(&identity(root))
        && prior.get("pending").is_some_and(truthy)
        && prior.get("kernel_entry").is_none_or(truthy)
    {
        // A clean, explicit owner trial on this image can recover a quarantine.
        let receipt = root.json_object(super::receipt::RECEIPT);
        let empty = Map::new();
        let p = match receipt.get("product") {
            Some(Value::Object(p)) => p,
            Some(other) if truthy(other) => {
                return Err(E::Crash("'product' object has no attribute 'get'".into()))
            }
            _ => &empty,
        };
        let zero = json!(0);
        let recovered = !receipt
            .get("mode")
            .unwrap_or(&Value::Null)
            .py_eq(&json!("product"))
            && p.get("state")
                .unwrap_or(&Value::Null)
                .py_eq(&json!("restored"))
            && is_true(p.get("kernel_completed"))
            && is_true(p.get("same_boot"))
            && receipt
                .get("current_boot_id")
                .unwrap_or(&Value::Null)
                .py_eq(&Value::from(root.boot_id()))
            && py_gt(
                receipt.get("updated").unwrap_or(&zero),
                prior.get("updated").unwrap_or(&zero),
            )?;
        if !recovered {
            return Err(refused("sleep_recovery_required"));
        }
        guard_record(root, false, false)?;
    }
    Ok(())
}

/// True while any host has configured the gadget (SSH/MTP in use).
fn usb_host_configured(root: &Root) -> bool {
    root.glob("/sys/class/udc/*/state")
        .iter()
        .any(|p| read(p, y2p_core::root::READ_LIMIT).as_deref() == Some("configured"))
}

/// `power.load_policy`: the qualified low-battery thresholds, or why there are none.
pub fn load_policy(root: &Root) -> Result<Map<String, Value>, &'static str> {
    let shipped = root.json_object("/etc/y2linux/power-policy.json");
    let value = match root.json("/data/system/platform/power-policy.json") {
        Some(Value::Object(map)) => map,
        _ => shipped,
    };
    // No derived or fallback voltage thresholds. A reference records who/what qualified the
    // policy; software cannot certify that physical evidence.
    let schema = value.get("schema").cloned().unwrap_or(Value::Null);
    if !(schema.py_eq(&json!(1)) || schema.py_eq(&json!(2))) || !is_true(value.get("enabled")) {
        return Err("thresholds_disabled_PHYSICAL_GATE");
    }
    let int = |key: &str| exact_int(value.get(key));
    let keys = [
        "critical_uv",
        "low_uv",
        "recover_uv",
        "critical_samples",
        "grace_seconds",
    ];
    if keys.iter().any(|k| int(k).is_none()) {
        return Err("invalid_policy");
    }
    let (critical, low, recover) = (
        int("critical_uv").unwrap_or(0),
        int("low_uv").unwrap_or(0),
        int("recover_uv").unwrap_or(0),
    );
    let reference = match value.get("qualification_reference") {
        Some(v) if truthy(v) => Some(v),
        _ => value.get("source"),
    };
    let named = match reference {
        Some(Value::String(text)) => (1..=256).contains(&text.chars().count()),
        _ => false,
    };
    if !(0 < critical
        && critical < low
        && low < recover
        && (1..=60).contains(&int("critical_samples").unwrap_or(0))
        && (3..=30).contains(&int("grace_seconds").unwrap_or(0))
        && named)
    {
        return Err("invalid_policy");
    }
    if schema.py_eq(&json!(2)) {
        let soc = ["warning_soc", "critical_soc", "shutdown_soc"].map(int);
        match soc {
            [Some(warning), Some(critical), Some(shutdown)]
                if 0 <= shutdown && shutdown < critical && critical < warning && warning <= 50 => {}
            _ => return Err("invalid_policy"),
        }
    }
    Ok(value)
}

pub fn battery_and_cable(root: &Root) -> Result<(), E> {
    let status = root.read("/sys/class/power_supply/BAT0/status");
    if root
        .read("/sys/class/power_supply/y2-usb-presence/online")
        .as_deref()
        != Some("0")
    {
        // POWER-FINAL-16 Defect C (archive 109): VBUS from a powered-off or sleeping PC with
        // nothing charging must not hold the Y2 awake. Sleep while the cable is in only when no
        // host uses the gadget and the charger is idle (Full, or inhibited/paused).
        if !matches!(status.as_deref(), Some("Full" | "Not charging")) || usb_host_configured(root)
        {
            return Err(refused("charger_or_usb_active"));
        }
    } else if status.as_deref() != Some("Discharging") {
        return Err(refused("battery_state_unavailable"));
    }
    let policy = load_policy(root);
    let voltage = root.integer("/sys/class/power_supply/BAT0/voltage_now");
    let state = root.json_object("/run/y2/power.json");
    let age: i128 = match state.get("monotonic_ns") {
        Some(Value::Number(n)) if n.is_i64() || n.is_u64() => {
            let sampled = n
                .as_i64()
                .map(i128::from)
                .or_else(|| n.as_u64().map(i128::from))
                .unwrap_or(0);
            i128::from(root.clock().monotonic_ns()) - sampled
        }
        _ => -1,
    };
    // F55: Low (20 %) and Critical (10 %) may sleep; asleep drains ~8 mA, awake and dark far more.
    // Every sleep returns within the 30-min SPM maintenance timer and admits again here, so a
    // sleeping player still reaches the low-battery power-off (shutdown_soc / critical_uv) awake.
    if !state
        .get("boot_id")
        .unwrap_or(&Value::Null)
        .py_eq(&Value::from(root.boot_id()))
        || !(0..=10_000_000_000).contains(&age)
        || !["Normal", "Low", "Critical"]
            .iter()
            .any(|s| is_text(state.get("state"), s))
    {
        return Err(refused("battery_safety_owner"));
    }
    // F55 (189640af): one loaded sample at 3.56-3.58 V (37 %) refused every sleep below recover_uv.
    // Judge the filtered voltage and refuse only where the low-battery owner is about to power off
    // (power.LowBattery).
    let empty = Map::new();
    let battery = state
        .get("battery")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let filtered = exact_int(battery.get("filtered_voltage_uv")).filter(|f| *f > 0);
    let level = filtered.or(voltage);
    let percent = exact_int(battery.get("percent"));
    let policy_ok = policy.as_ref().ok();
    let flat = match (percent, policy_ok) {
        (Some(percent), Some(policy)) => {
            policy
                .get("schema")
                .unwrap_or(&Value::Null)
                .py_eq(&json!(2))
                && ["hardware", "calibrated_estimate"]
                    .iter()
                    .any(|c| is_text(battery.get("confidence"), c))
                && percent <= exact_int(policy.get("shutdown_soc")).unwrap_or(i64::MIN)
        }
        _ => false,
    };
    let reserve = match (policy_ok, voltage, level) {
        (Some(policy), Some(_), Some(level)) => {
            level <= exact_int(policy.get("critical_uv")).unwrap_or(0) || flat
        }
        _ => true,
    };
    if reserve {
        return Err(refused("battery_reserve"));
    }
    if ["ShutdownPending", "Executing", "Failed"]
        .iter()
        .any(|s| is_text(root.json_object("/run/y2/shutdown.json").get("state"), s))
    {
        return Err(refused("shutdown_in_progress"));
    }
    Ok(())
}

/// `update.status(ctx).get('state')`: the update journal's state as the update page reads it.
fn update_state(root: &Root) -> Value {
    let path = root.path("/data/updates/state.json");
    let mut value = root.json("/data/updates/state.json");
    if value.is_none() && !path.exists() && path.symlink_metadata().is_err() {
        let ready = root.read("/data/.y2data-schema").as_deref() == Some("1")
            && root.path("/data/updates").is_dir();
        value = Some(json!({"schema": 1, "state": if ready { "Idle" } else { "Unavailable" }}));
    }
    // The native core writes journal schema 2 since OTA-35 (BOOTIMG + root).
    match value {
        Some(Value::Object(map))
            if map
                .get("schema")
                .is_some_and(|s| s.py_eq(&json!(1)) || s.py_eq(&json!(2))) =>
        {
            map.get("state").cloned().unwrap_or(Value::Null)
        }
        _ => json!("Failed"),
    }
}

pub fn operation_state(root: &Root, workloads: bool) -> Result<(), E> {
    let state = update_state(root);
    if !["Idle", "Acknowledged", "RolledBack", "Failed"]
        .iter()
        .any(|t| is_text(Some(&state), t))
    {
        return Err(refused("firmware_update"));
    }
    if root
        .path("/data/system/platform/maintenance-pending")
        .exists()
    {
        return Err(refused("maintenance_recovery"));
    }
    if !workloads {
        return Ok(());
    }
    let Some(leases) = root.read("/sys/module/workload/parameters/leases") else {
        return Err(refused("workload_owner_unavailable"));
    };
    let pattern = Regex::new(r"^(\w+) remaining_ms=(\d+) min_khz=-?\d+$").expect("pattern");
    for line in splitlines(&leases) {
        let Some(found) = pattern.captures(line) else {
            return Err(refused("workload_owner_unavailable"));
        };
        let remaining: u128 = found[2].parse().unwrap_or(u128::MAX);
        if &found[1] != "Idle" && remaining > 0 {
            return Err(refused(&format!("workload_{}", &found[1])));
        }
    }
    Ok(())
}

/// Name actual live flock holders; never infer ownership from old files.
pub fn lock_holders(root: &Root, meta: &Metadata) -> Vec<String> {
    let dev = meta.dev();
    let (major, minor) = (libc::major(dev) as i64, libc::minor(dev) as i64);
    let inode = meta.ino() as i128;
    let mut holders = Vec::new();
    let comm = Regex::new(r"[^\w.-]").expect("pattern");
    for line in splitlines(&root.read("/proc/locks").unwrap_or_default()) {
        let fields = split(line);
        if fields.len() < 6
            || fields[1] != "FLOCK"
            || !y2p_core::suspend_text::is_decimal(fields[4])
        {
            continue;
        }
        let parts: Vec<&str> = fields[5].split(':').collect();
        let [a, b, c] = parts.as_slice() else {
            continue;
        };
        let (Some(a), Some(b), Some(c)) = (
            y2p_core::suspend_text::int16(a),
            y2p_core::suspend_text::int16(b),
            y2p_core::root::python_int(c),
        ) else {
            continue;
        };
        if (a, b, i128::from(c)) == (major, minor, inode) {
            let pid = fields[4];
            let name = root
                .read(&format!("/proc/{pid}/comm"))
                .unwrap_or_else(|| "unknown".into());
            let name: String = comm.replace_all(&name, "_").chars().take(32).collect();
            holders.push(format!("{pid}/{name}"));
        }
    }
    holders.truncate(8);
    holders
}

fn private_directory(path: &Path) -> Result<(), E> {
    y2p_core::record::private_directory(path).map_err(|e| E::os(&e, Some(path)))
}

/// `product-sleep-guard.json`: whether an attempt of this image (`identity`) is still unproven.
/// Only `pending` with `kernel_entry` quarantines the image (a record without `kernel_entry`, from
/// 12, counts as one that reached the kernel).
#[derive(Serialize)]
struct Guard {
    identity: Value,
    pending: bool,
    kernel_entry: bool,
    boot_id: Option<String>,
    updated: f64,
}

pub fn guard_record(root: &Root, pending: bool, kernel_entry: bool) -> Result<(), E> {
    let path = root.path(GUARD);
    private_directory(path.parent().unwrap_or(Path::new("/")))?;
    let guard = Guard {
        identity: identity(root),
        pending,
        kernel_entry,
        boot_id: root.boot_id(),
        updated: wall(),
    };
    y2p_core::record::atomic_json(&path, &guard, true, 0o600).map_err(|e| E::os(&e, Some(&path)))
}

/// True when the kernel returned an error before freezing any task.
///
/// A PM_SUSPEND_PREPARE notifier refusal (c43e8711 report: system-idle bringing a parked CPU back
/// after hotplug was disabled, -EPERM) returns from the write before a single device callback ran.
/// The current cycle's ring then holds only these marks, ending in a failed EXIT. Anything else,
/// including a wrapped or unreadable ring, keeps the quarantine.
pub fn failed_before_freeze(root: &Root) -> bool {
    let text = root
        .read_limited("/sys/firmware/y2_pm/devices", 16384)
        .unwrap_or_default();
    let lines = splitlines(&text);
    if lines.first().is_none_or(|l| !l.starts_with("ring=valid ")) {
        return false;
    }
    let pattern =
        Regex::new(r"^\d+ phase=\d+ mark result=(-?\d+) ms=\d+ stage=(\w+)$").expect("pattern");
    let mut marks: Vec<(String, i64)> = Vec::new();
    for line in &lines[1..] {
        let Some(found) = pattern.captures(line) else {
            return false;
        };
        let Ok(result) = found[1].parse::<i64>() else {
            return false;
        };
        marks.push((found[2].to_owned(), result));
    }
    let kernel = fields(root.read("/sys/firmware/y2_pm/state").as_deref());
    marks
        .last()
        .is_some_and(|(stage, result)| stage == "EXIT" && *result < 0)
        && marks.iter().any(|(s, r)| s == "SUSPEND_REQUEST" && *r == 0)
        && marks
            .iter()
            .all(|(s, _)| PRE_FREEZE_MARKS.contains(&s.as_str()))
        && field(&kernel, "failed_stage") == Some("EXIT")
}

/// `product-sleep-retry.json`: no product sleep on this boot before `not_before_ns` (CLOCK_BOOTTIME).
#[derive(Serialize)]
struct Retry {
    boot_id: Option<String>,
    not_before_ns: u64,
}

pub fn retry_later(root: &Root) -> Result<(), E> {
    let path = root.path(RETRY);
    private_directory(path.parent().unwrap_or(Path::new("/")))?;
    let retry = Retry {
        boot_id: root.boot_id(),
        not_before_ns: root.clock().boottime_ns() + PREPARE_RETRY_SECONDS * 1_000_000_000,
    };
    y2p_core::record::atomic_json(&path, &retry, false, 0o600).map_err(|e| E::os(&e, Some(&path)))
}

/// True only while this boot's journal says the UI has not reached `application_ready` yet; unknown
/// or another boot is not "starting".
pub fn application_starting(root: &Root) -> bool {
    let journal = root.json_object("/data/system/platform/boot.json");
    journal
        .get("boot_id")
        .unwrap_or(&Value::Null)
        .py_eq(&Value::from(root.boot_id()))
        && ["platform_start", "services_started"]
            .iter()
            .any(|s| is_text(journal.get("last_stage"), s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::suspend::tests::Dir;
    use std::fs;

    fn put(dir: &Dir, name: &str, text: &str) {
        let path = dir.path().join(name.trim_start_matches('/'));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn policies_validate_as_power_py() {
        let dir = Dir::new();
        let root = Root::new(dir.path());
        assert_eq!(load_policy(&root), Err("thresholds_disabled_PHYSICAL_GATE"));
        let good = r#"{"schema": 2, "enabled": true, "critical_uv": 3400000, "low_uv": 3500000,
            "recover_uv": 3600000, "critical_samples": 3, "grace_seconds": 10, "source": "owner",
            "warning_soc": 20, "critical_soc": 10, "shutdown_soc": 5}"#;
        put(&dir, "/etc/y2linux/power-policy.json", good);
        assert!(load_policy(&root).is_ok());
        put(&dir, "/data/system/platform/power-policy.json", "[1]");
        assert!(
            load_policy(&root).is_ok(),
            "a non-dict override falls back to the shipped policy"
        );
        put(
            &dir,
            "/data/system/platform/power-policy.json",
            &good.replace("\"source\": \"owner\"", "\"source\": \"\""),
        );
        assert_eq!(load_policy(&root), Err("invalid_policy"));
        put(
            &dir,
            "/data/system/platform/power-policy.json",
            &good.replace("\"shutdown_soc\": 5", "\"shutdown_soc\": 10"),
        );
        assert_eq!(load_policy(&root), Err("invalid_policy"));
    }

    #[test]
    fn a_kernel_refusal_before_freeze_is_recognised() {
        let dir = Dir::new();
        let root = Root::new(dir.path());
        put(&dir, "/sys/firmware/y2_pm/devices", "ring=valid cycle=3\n1 phase=0 mark result=0 ms=1 stage=HELPER_REQUEST\n\
             2 phase=0 mark result=0 ms=2 stage=SUSPEND_REQUEST\n3 phase=0 mark result=-1 ms=3 stage=EXIT\n");
        put(
            &dir,
            "/sys/firmware/y2_pm/state",
            "valid=1 failed_stage=EXIT error=-1\n",
        );
        assert!(failed_before_freeze(&root));
        put(
            &dir,
            "/sys/firmware/y2_pm/state",
            "valid=1 failed_stage=DPM_PREPARED error=-16\n",
        );
        assert!(!failed_before_freeze(&root));
    }
}
