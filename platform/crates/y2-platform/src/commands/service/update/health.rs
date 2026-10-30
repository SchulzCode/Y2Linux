//! `update health-ack`: the first boot of an installed update confirms itself once Reborn draws and
//! answers, or asks for the rollback after 180 s.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    atomic_json, command, core, fs, journal_state, json, private_directory, py, remove_package,
    status, JournalState, Raise, Result, Root, Value, RUNNING_PANIC_SECONDS,
};
use serde::Deserialize;
use y2p_core::lenient::{self, BootId};

/// What Reborn answers to `status` (`rebornctl status --json`): the fields the health check reads.
#[derive(Deserialize, Default)]
#[serde(default)]
struct AppStatus {
    #[serde(deserialize_with = "lenient::text")]
    overall: Option<String>,
    #[serde(deserialize_with = "lenient::or_none")]
    passed: Option<bool>,
    /// Reborn's own verdict (Beta 8), `None` without the field; only JSON `true` is healthy.
    #[serde(deserialize_with = "verdict")]
    healthy: Option<bool>,
    #[serde(deserialize_with = "lenient::object_or_default")]
    graphics: Graphics,
    #[serde(deserialize_with = "lenient::integer")]
    uptime_seconds: Option<i64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Graphics {
    #[serde(deserialize_with = "lenient::is_true")]
    available: bool,
}

/// A field that is there: whether it is JSON `true`.
fn verdict<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<bool>, D::Error> {
    lenient::is_true(deserializer).map(Some)
}

impl AppStatus {
    /// An answer that is no object reads as nothing.
    fn of(answer: Value) -> Self {
        match answer {
            Value::Object(answer) => lenient::read(answer),
            _ => Self::default(),
        }
    }

    /// rebornctl's exit rule: a status whose `overall` is failed or degraded, or that did not pass,
    /// is no success.
    fn succeeded(&self) -> bool {
        !(matches!(self.overall.as_deref(), Some("failed" | "degraded"))
            || self.passed == Some(false))
    }

    /// Reborn's own verdict: drawing, database working, no stuck worker. A library scan that could
    /// not read some songs is not a reason to roll an update back (V1 code review B3). Without the
    /// field (a Reborn before Beta 8), the drawing and uptime checks alone.
    fn healthy(&self) -> bool {
        self.healthy.unwrap_or_else(|| {
            self.graphics.available && self.uptime_seconds.is_some_and(|u| u >= 10)
        })
    }
}

#[cfg(test)]
pub fn application_healthy(app: &Value) -> bool {
    AppStatus::of(app.clone()).healthy()
}

/// What `rebornctl status --json` answers when it succeeds (exit 0), else nothing. On the player
/// straight from Reborn's control socket (the same request, no rebornctl process), with rebornctl's
/// exit rule. Below a fixture root the command as before (the goldens record it).
fn application_status(root: &Root) -> AppStatus {
    if root.is_real() {
        use crate::commands::observation::{reborn_control, REBORN_CONTROL};
        return reborn_control(&root.path(REBORN_CONTROL), "status", 2.0)
            .map(AppStatus::of)
            .filter(AppStatus::succeeded)
            .unwrap_or_default();
    }
    let answer = command::run(root, &["/usr/bin/rebornctl", "status", "--json"], 2.0);
    match answer.output.as_deref() {
        Some(text) if answer.ok => {
            serde_json::from_str(text).map_or_else(|_| AppStatus::default(), AppStatus::of)
        }
        _ => AppStatus::default(),
    }
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

/// /run/y2/health-debug.json: the kernel settings init saved before the health boot changed them.
#[derive(Deserialize, Default)]
#[serde(default)]
struct HealthDebug {
    #[serde(deserialize_with = "lenient::text")]
    printk: Option<String>,
    #[serde(deserialize_with = "lenient::text")]
    panic_on_rcu_stall: Option<String>,
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
    let saved: HealthDebug = lenient::read(saved);
    for (name, value) in [
        ("printk", saved.printk.as_deref()),
        ("panic", Some(RUNNING_PANIC_SECONDS)),
        ("panic_on_rcu_stall", saved.panic_on_rcu_stall.as_deref()),
    ] {
        if let Some(value) = value {
            let stripped = y2p_core::root::python_strip(value);
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

/// /run/y2/application-ready.json (written by Reborn): its first frame, and the process that drew it.
#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct ApplicationReady {
    boot_id: BootId,
    #[serde(deserialize_with = "lenient::is_true")]
    first_frame: bool,
    #[serde(deserialize_with = "lenient::integer")]
    pid: Option<i64>,
    /// As written: compared with the number /proc reads as Python's `==` did (`true` equals 1).
    start_ticks: Option<Value>,
}

impl ApplicationReady {
    pub(crate) fn read(root: &Root) -> Self {
        lenient::read(root.json_object("/run/y2/application-ready.json"))
    }

    /// Whether Reborn drew its first frame in this boot.
    pub(crate) fn drawn(&self, root: &Root) -> bool {
        self.boot_id.is(root.boot_id().as_deref()) && self.first_frame
    }

    /// Whether Reborn drew its first frame in the boot named `boot`.
    pub(crate) fn drawn_in(&self, boot: &str) -> bool {
        self.boot_id.is(Some(boot)) && self.first_frame
    }

    fn pid(&self) -> Option<i64> {
        self.pid.filter(|p| *p > 0)
    }
}

/// /run/y2/update-health.json: the boot the health check started in, and when it gives up.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Progress {
    boot_id: BootId,
    #[serde(deserialize_with = "lenient::number")]
    deadline_monotonic_s: Option<f64>,
}

/// Whether a number read from /proc (or none) is the recorded one (none equals null).
fn same_number(read: Option<i64>, recorded: Option<&Value>) -> bool {
    match (read, recorded) {
        (None, None | Some(Value::Null)) => true,
        (Some(n), Some(_)) => py::python_eq_int(recorded, n),
        _ => false,
    }
}

/// The first boot of an installed update confirms itself once Reborn draws and
/// answers, or asks for the rollback after 180 s.
pub(super) fn health_ack(root: &Root) -> Result<Value> {
    if journal_state(root) != JournalState::PendingHealth {
        return Ok(json!(status(root)));
    }
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    let progress: Progress = lenient::read(root.json_object("/run/y2/update-health.json"));
    let boot = root.boot_id();
    let deadline = if progress.boot_id.is(boot.as_deref()) {
        progress.deadline_monotonic_s
    } else {
        let deadline = root.clock().monotonic() + 180.0;
        let record = json!({"boot_id": boot, "deadline_monotonic_s": deadline});
        atomic_json(&runtime.join("update-health.json"), &record, false)?;
        Some(deadline)
    };
    // Only a record this call did not write can lack a number: refused before anything changes.
    let deadline = deadline.ok_or_else(|| Raise::value("update_health_record_unreadable"))?;
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
    let start_ticks = fields.get(19).and_then(|f| y2p_core::root::parse_int(f));
    if !same_number(start_ticks, ready.start_ticks.as_ref()) {
        return Ok(json!({"state": "PendingHealth", "reason": "application_generation_changed"}));
    }
    if !core_health_ok(root)? {
        return Ok(json!({"state": "PendingHealth", "reason": "core_health_not_OK"}));
    }
    if !application_status(root).healthy() {
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
    let state = |name: &str| {
        health
            .checks
            .iter()
            .rev()
            .find(|check| check.name == name)
            .map(|check| check.state)
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
    .all(|name| state(name) == Some(y2p_contract::CheckState::Ok)))
}
