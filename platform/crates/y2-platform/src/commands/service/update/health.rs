//! `update health-ack`: the first boot of an installed update confirms itself once Reborn draws and
//! answers, or asks for the rollback after 180 s.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    atomic_json, command, core, fs, is_true, journal_state, json, object, private_directory, py,
    remove_package, status, JournalState, Map, Raise, Result, Root, Value, RUNNING_PANIC_SECONDS,
};

/// Reborn's own verdict (`healthy`, Beta 8): drawing, database working,
/// no stuck worker. A library scan that could not read some songs is not a reason to roll an update
/// back (V1 code review B3). Without the field (a Reborn before Beta 8), the drawing and uptime checks
/// alone.
pub fn application_healthy(app: &Value) -> bool {
    let Value::Object(app) = app else {
        return false;
    };
    if let Some(healthy) = app.get("healthy") {
        return is_true(Some(healthy));
    }
    matches!(app.get("graphics"), Some(Value::Object(g)) if is_true(g.get("available")))
        && py::exact_int(app.get("uptime_seconds")).is_some_and(|u| u >= 10)
}

/// `[0-9]+(\s+[0-9]+)*` on the stripped text.
pub(super) fn numbers_only(text: &str) -> bool {
    let space = |c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c);
    !text.is_empty()
        && text
            .split(space)
            .filter(|t| !t.is_empty())
            .all(|t| t.bytes().all(|b| b.is_ascii_digit()))
        && !text.starts_with(space)
}

/// Undo init's health-boot console level and stall panic (F23); best
/// effort. The panic timeout stays at 10 s: only the health boot's RCU stall panic and console level go
/// back to the saved values.
pub(super) fn restore_health_debug(root: &Root) {
    // strict=False: a raw tab from /proc/sys/kernel/printk (8.22 test build).
    let Some(Value::Object(saved)) =
        loads_lenient(&root.read("/run/y2/health-debug.json").unwrap_or_default())
    else {
        return;
    };
    for name in ["printk", "panic", "panic_on_rcu_stall"] {
        let value = if name == "panic" {
            Some(RUNNING_PANIC_SECONDS.to_owned())
        } else {
            saved.get(name).and_then(Value::as_str).map(str::to_owned)
        };
        if let Some(value) = value {
            let stripped = y2p_core::root::python_strip(&value);
            if numbers_only(stripped) {
                let _ = fs::write(
                    root.path(&format!("/proc/sys/kernel/{name}")),
                    format!("{stripped}\n"),
                );
            }
        }
    }
    let _ = fs::remove_file(root.path("/run/y2/health-debug.json"));
}

/// `json.loads(text, strict=False)`: init writes /proc/sys/kernel/printk's raw tabs into the string.
pub(super) fn loads_lenient(text: &str) -> Option<Value> {
    let mut escaped = String::with_capacity(text.len());
    let (mut in_string, mut backslash) = (false, false);
    for c in text.chars() {
        if in_string {
            if backslash {
                backslash = false;
            } else if c == '\\' {
                backslash = true;
            } else if c == '"' {
                in_string = false;
            } else if (c as u32) < 0x20 {
                escaped.push_str(&format!("\\u{:04x}", c as u32));
                continue;
            }
        } else if c == '"' {
            in_string = true;
        }
        escaped.push(c);
    }
    serde_json::from_str(&escaped).ok()
}

/// /run/y2/application-ready.json: Reborn's first frame, and the process that drew it.
pub(crate) struct ApplicationReady(Map<String, Value>);

impl ApplicationReady {
    pub(crate) fn read(root: &Root) -> Self {
        Self(root.json_object("/run/y2/application-ready.json"))
    }

    /// Whether Reborn drew its first frame in this boot.
    pub(crate) fn drawn(&self, root: &Root) -> bool {
        root.is_this_boot(self.0.get("boot_id")) && is_true(self.0.get("first_frame"))
    }

    fn pid(&self) -> Option<i64> {
        py::exact_int(self.0.get("pid")).filter(|p| *p > 0)
    }
}

/// Whether a number read from /proc (or none) is the recorded one (none equals null).
fn same_number(read: Option<i64>, recorded: Option<&Value>) -> bool {
    match (read, recorded) {
        (None, None | Some(Value::Null)) => true,
        (Some(n), Some(_)) => py::eq_int(recorded, n),
        _ => false,
    }
}

/// The first boot of an installed update confirms itself once Reborn draws and
/// answers, or asks for the rollback after 180 s.
pub(super) fn health_ack(root: &Root) -> Result<Value> {
    if journal_state(root) != JournalState::PendingHealth {
        return Ok(Value::Object(status(root)));
    }
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    let mut progress = root.json_object("/run/y2/update-health.json");
    let boot = Value::from(root.boot_id());
    if progress.get("boot_id").cloned().unwrap_or(Value::Null) != boot {
        progress = object(Some(json!({"boot_id": boot,
                                      "deadline_monotonic_s": root.clock().monotonic() + 180.0})));
        atomic_json(&runtime.join("update-health.json"), &progress, false)?;
    }
    // Only a record this call did not write can lack a number: refused before anything changes.
    let deadline = py::number(progress.get("deadline_monotonic_s"))
        .ok_or_else(|| Raise::value("update_health_record_unreadable"))?;
    if root.clock().monotonic() >= deadline {
        // Ask LK for the recovery image now, so even a later hang before the rescue restores on the
        // next power-on. Never an automatic power-off.
        core(root, "rollback", None, 15.0)?;
        return Ok(Value::Object(core(root, "prepare", None, 120.0)?));
    }
    // A successful frame alone is insufficient: app must still answer, core mounts/devices must be
    // usable, and the same boot/process must own readiness.
    let ready = ApplicationReady::read(root);
    if !ready.drawn(root) {
        return Ok(json!({"state": "PendingHealth", "reason": "application_first_frame_missing"}));
    }
    let Some(pid) = ready.pid() else {
        return Ok(json!({"state": "PendingHealth", "reason": "application_pid_missing"}));
    };
    if pid > 4_194_304 {
        return Err(Raise::value("invalid pid"));
    }
    let stat = root.read(&format!("/proc/{pid}/stat")).unwrap_or_default();
    let tail = stat.rsplit_once(") ").map_or(stat.as_str(), |(_, t)| t);
    let fields: Vec<&str> = tail.split_whitespace().collect();
    let start_ticks = fields.get(19).and_then(|f| y2p_core::root::python_int(f));
    if !same_number(start_ticks, ready.0.get("start_ticks")) {
        return Ok(json!({"state": "PendingHealth", "reason": "application_generation_changed"}));
    }
    if !core_health_ok(root)? {
        return Ok(json!({"state": "PendingHealth", "reason": "core_health_not_OK"}));
    }
    let answer = command::run(root, &["/usr/bin/rebornctl", "status", "--json"], 2.0);
    let app = if answer.ok {
        serde_json::from_str(answer.output.as_deref().unwrap_or("")).unwrap_or(json!({}))
    } else {
        json!({})
    };
    if !application_healthy(&app) {
        return Ok(json!({"state": "PendingHealth", "reason": "application_not_ready"}));
    }
    crate::commands::boot::mark(root, "application_ready")?;
    let result = core(root, "ack", None, 15.0)?;
    restore_health_debug(root);
    // The core keeps the installed package as current/ (the next install's rollback copy); anything
    // left in pending/ is no longer needed.
    let _ = remove_package(&root.path("/data/updates/pending"));
    Ok(Value::Object(result))
}

/// Root, data, filesystem_errors, display, render_node, audio and input all `OK`
/// (`checks.get(name, {}).get('state')`: the dict comprehension keeps the last check of a name).
fn core_health_ok(root: &Root) -> Result<bool> {
    let health = crate::commands::observation::health::check(root, false)?;
    let checks = health
        .get("checks")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let state = |name: &str| {
        checks
            .iter()
            .rev()
            .find(|item| item.get("name").and_then(Value::as_str) == Some(name))
            .and_then(|item| item.get("state"))
            .and_then(Value::as_str)
    };
    Ok([
        "root",
        "data",
        "filesystem_errors",
        "display",
        "render_node",
        "audio",
        "input",
    ]
    .iter()
    .all(|name| state(name) == Some("OK")))
}
