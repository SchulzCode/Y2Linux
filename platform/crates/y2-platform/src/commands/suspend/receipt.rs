//! `suspend_record.py` and `sleep.transition`/`sleep.wake_irqs`: durable suspend receipts, written
//! only before entry or after Linux returns. No lock of its own: the caller holds
//! `/run/y2/suspend.lock` (sleep request, the helper, y2-suspend).
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    admission::spm_state, diagnostics, interrupted, is_text, journal, py_add, py_eq, py_gt, pyjson,
    text_value, E,
};
use serde_json::{json, Map, Value};
use std::{
    fs::{self, File},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    },
    path::Path,
};
use y2p_core::{
    json::{dumps, Separators},
    py::{is_true, truthy},
    root::{python_int, read},
    suspend_text::{fields, split, splitlines, Regex},
    Root,
};

pub const RECEIPT: &str = "/data/system/platform/suspend-last.json";
const PLATFORM: &str = "/data/system/platform";

/// Serviced PMIC child IRQs, matching the existing owner harness names.
///
/// Both RTC and Power feed SPM EINT. That shared SPM wake bit is deliberately insufficient; only a
/// same-boot child-IRQ delta attributes their source.
pub fn wake_irqs(text: Option<&str>) -> Map<String, Value> {
    let mut counts: Map<String, Value> = Map::new();
    for line in splitlines(text.unwrap_or_default()) {
        let fields = split(line);
        if fields.first().is_none_or(|f| !f.ends_with(':')) {
            continue;
        }
        for (label, name) in [("mt6397-rtc", "rtc"), ("mtk-pmic-keys", "power")] {
            if !fields.contains(&label) {
                continue;
            }
            let mut total: i64 = 0;
            for field in &fields[1..] {
                if !y2p_core::suspend_text::is_decimal(field) {
                    break;
                }
                total += field.parse::<i64>().unwrap_or(0);
            }
            let before = counts.get(name).and_then(Value::as_i64).unwrap_or(0);
            counts.insert(name.into(), json!(before + total));
        }
    }
    counts
}

fn set(p: &mut Map<String, Value>, pairs: &[(&str, Value)]) {
    for (key, value) in pairs {
        p.insert((*key).to_owned(), value.clone());
    }
}

fn state(p: &Map<String, Value>) -> &str {
    p.get("state").and_then(Value::as_str).unwrap_or_default()
}

/// `reason or fallback`.
fn either(reason: Option<&str>, fallback: Value) -> Value {
    match reason {
        Some(r) if !r.is_empty() => Value::String(r.to_owned()),
        _ => fallback,
    }
}

/// A receipt's dict (`value.get(key, {})`) for the IRQ counts; anything else is Python's TypeError
/// once it is indexed.
fn counts<'a>(
    value: &'a Map<String, Value>,
    key: &str,
) -> Result<Option<&'a Map<String, Value>>, E> {
    match value.get(key) {
        None => Ok(None),
        Some(Value::Object(map)) => Ok(Some(map)),
        Some(_) => Err(E::TypeError(format!("{key} is not a dict"))),
    }
}

/// `sleep.transition`: the product state after `stage`.
pub fn transition(
    value: &Map<String, Value>,
    stage: &str,
    result: &Value,
    boot_id: Option<&str>,
    reason: Option<&str>,
) -> Result<Map<String, Value>, E> {
    let mut p = super::super::sleep::product(value, boot_id)
        .map_err(|_| E::TypeError("receipt product is not a dict".into()))?;
    let boot = boot_id.map_or(Value::Null, |b| json!(b));
    let reason_value = reason.map_or(Value::Null, |r| json!(r));
    if stage == "requested" || stage == "quiescing_radios" || stage.starts_with("refused_") {
        let same = if boot_id.is_some_and(|b| !b.is_empty()) {
            json!(true)
        } else {
            Value::Null
        };
        set(
            &mut p,
            &[
                ("state", json!("requested")),
                ("reason", Value::Null),
                ("request_boot_id", boot),
                ("same_boot", same),
                ("kernel_completed", json!(false)),
                ("wake_reason", json!("unknown")),
            ],
        );
    }
    if matches!(stage, "reborn_prepared" | "quiescing_radios" | "syncing") {
        set(&mut p, &[("state", json!("preparing"))]);
    }
    if let Some(rest) = stage.strip_prefix("refused_") {
        let reason = either(reason, json!(rest));
        set(&mut p, &[("state", json!("refused")), ("reason", reason)]);
    } else if stage == "kernel_suspend" {
        // Durable intent immediately before the blocking kernel write. Actual SPM entry is proven
        // only by the kernel journal after wake/reset.
        set(
            &mut p,
            &[
                ("state", json!("sleeping")),
                ("reason", Value::Null),
                ("kernel_completed", json!(false)),
            ],
        );
    } else if stage == "kernel_resumed" || stage == "restoring_radios" {
        let text = match value.get("persistent_kernel") {
            Some(Value::String(s)) => s.clone(),
            Some(other) if truthy(other) => {
                return Err(E::TypeError("expected string or bytes-like object".into()))
            }
            _ => String::new(),
        };
        let kernel = fields(Some(&text));
        let k = |key: &str| kernel.get(key).and_then(Value::as_str);
        let completed = k("valid") == Some("1")
            && k("stage") == Some("EXIT")
            && k("error") == Some("0")
            && is_true(p.get("same_boot"))
            && pm_test_is(value.get("pm_test"), "none")
            && k("reset_resume_entry") == Some("0x59325253")
            && k("pcm_length") == Some("596")
            && !matches!(k("wake"), None | Some("0") | Some("0x0"));
        p.insert("kernel_completed".into(), json!(completed));
        let (before, after) = (
            counts(value, "wake_irqs_before")?,
            counts(value, "wake_irqs_after")?,
        );
        if completed {
            let mut sources = Vec::new();
            for name in ["rtc", "power"] {
                if let (Some(b), Some(a)) = (
                    before.and_then(|m| m.get(name)),
                    after.and_then(|m| m.get(name)),
                ) {
                    if py_gt(a, b)? {
                        sources.push(name);
                    }
                }
            }
            let wake = match sources.as_slice() {
                [one] => *one,
                [] => "unknown",
                _ => "multiple",
            };
            p.insert("wake_reason".into(), json!(wake));
        }
        if state(&p) != "refused" {
            set(
                &mut p,
                &[("state", json!("waking")), ("reason", reason_value)],
            );
        }
        if k("failed_stage") == Some("DPM_PREPARED") && k("error") == Some("-16") {
            set(
                &mut p,
                &[
                    ("state", json!("refused")),
                    ("reason", json!("device_prepare_refused")),
                ],
            );
        }
    } else if stage == "product_refused" {
        set(
            &mut p,
            &[("state", json!("refused")), ("reason", reason_value)],
        );
    } else if stage == "product_failure" {
        set(
            &mut p,
            &[("state", json!("restore_failed")), ("reason", reason_value)],
        );
    } else if stage.starts_with("complete_result_") {
        let error = value.get("error").cloned().unwrap_or(Value::Null);
        if reason.is_some_and(|r| !r.is_empty()) || truthy(result) || truthy(&error) {
            if state(&p) != "refused" {
                let why = either(
                    reason,
                    if truthy(&error) {
                        error
                    } else {
                        json!("platform_restoration_failed")
                    },
                );
                set(
                    &mut p,
                    &[("state", json!("restore_failed")), ("reason", why)],
                );
            }
        } else if !pm_test_none(value.get("pm_test"))? {
            set(
                &mut p,
                &[
                    ("state", json!("idle")),
                    ("reason", json!("pm_test_completed")),
                    ("kernel_completed", json!(false)),
                ],
            );
        } else if !is_true(p.get("same_boot")) {
            set(
                &mut p,
                &[
                    ("state", json!("restore_failed")),
                    ("reason", json!("same_boot_not_proven")),
                ],
            );
        } else if !p.get("kernel_completed").is_some_and(truthy) {
            set(
                &mut p,
                &[
                    ("state", json!("restore_failed")),
                    ("reason", json!("kernel_exit_not_proven")),
                ],
            );
        } else if !value.get("reborn_restore").is_some_and(truthy) {
            set(
                &mut p,
                &[
                    ("state", json!("restore_failed")),
                    ("reason", json!("reborn_resume_probe_failed")),
                ],
            );
        } else {
            set(
                &mut p,
                &[("state", json!("restored")), ("reason", Value::Null)],
            );
        }
    }
    Ok(p)
}

/// `value.get('pm_test') == text`.
fn pm_test_is(value: Option<&Value>, text: &str) -> bool {
    value.is_some_and(|v| py_eq(v, &json!(text)))
}

/// `value.get('pm_test') in {None, 'none'}` (a list or dict is unhashable).
fn pm_test_none(value: Option<&Value>) -> Result<bool, E> {
    match value {
        None | Some(Value::Null) => Ok(true),
        Some(Value::Array(_) | Value::Object(_)) => Err(E::TypeError("unhashable type".into())),
        Some(other) => Ok(is_text(Some(other), "none")),
    }
}

/// `record(ctx, stage, result, reason, reborn_ack, mode, restore_ack, dark)`'s arguments.
#[derive(Default)]
pub struct Stage<'a> {
    pub stage: &'a str,
    pub result: Option<i64>,
    pub reason: Option<String>,
    pub reborn_ack: Option<bool>,
    pub mode: Option<&'a str>,
    pub restore_ack: Option<Map<String, Value>>,
    pub dark: bool,
}

impl<'a> Stage<'a> {
    pub fn new(stage: &'a str, result: Option<i64>) -> Self {
        Stage {
            stage,
            result,
            ..Default::default()
        }
    }

    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

fn private_directory(path: &Path) -> Result<(), E> {
    y2p_core::record::private_directory(path).map_err(|e| E::os(&e, Some(path)))
}

fn atomic_json(path: &Path, value: &Map<String, Value>, durable: bool) -> Result<(), E> {
    y2p_core::record::atomic_json(path, value, durable, 0o600).map_err(|e| E::os(&e, Some(path)))
}

/// Whether `record` starts a new receipt for this stage.
pub fn starts_receipt(stage: &str, value: &Map<String, Value>) -> bool {
    stage == "requested"
        || (stage == "quiescing_radios" && !value.get("reborn_prepared").is_some_and(truthy))
        || stage.starts_with("refused_")
}

/// Whether Python would read this receipt (for `stage`) the way `record` does: the fields it adds to,
/// indexes or compares hold what the platform itself writes. A command line meeting anything else
/// hands itself to Python before it changes anything.
pub fn plain(value: &Map<String, Value>, stage: &str) -> bool {
    if starts_receipt(stage, value) {
        return true;
    }
    let number = |key: &str| {
        value
            .get(key)
            .is_none_or(|v| matches!(v, Value::Number(_) | Value::Bool(_)))
    };
    let irqs = |key: &str| match value.get(key) {
        None => true,
        Some(Value::Object(map)) => map
            .values()
            .all(|v| matches!(v, Value::Number(_) | Value::Bool(_))),
        Some(_) => false,
    };
    let product = match value.get("product") {
        Some(Value::Object(p)) => {
            !matches!(p.get("state"), Some(Value::Array(_) | Value::Object(_)))
        }
        Some(other) => !truthy(other),
        None => true,
    };
    product
        && number("cycles")
        && number("maintenance_wakes")
        && number("suspended_seconds")
        && irqs("wake_irqs_before")
        && irqs("wake_irqs_after")
        && value
            .get("stages")
            .is_none_or(|s| matches!(s, Value::Array(_)))
        && value
            .get("persistent_kernel")
            .is_none_or(|v| v.is_string() || !truthy(v))
        && !matches!(
            value.get("pm_test"),
            Some(Value::Array(_) | Value::Object(_))
        )
}

/// `suspend_record.record`.
pub fn record(root: &Root, s: Stage) -> Result<Map<String, Value>, E> {
    let directory = root.path(PLATFORM);
    private_directory(&directory)?;
    let path = directory.join("suspend-last.json");
    let mut value = root.json_object(RECEIPT);
    let boot = || text_value(root.read("/proc/sys/kernel/random/boot_id"));
    if starts_receipt(s.stage, &value) {
        let pm_test = Regex::new(r"\[(\w+)\]")
            .expect("pattern")
            .captures(&root.read("/sys/power/pm_test").unwrap_or_default())
            .map_or(Value::Null, |c| json!(&c[1]));
        value = Map::new();
        set(
            &mut value,
            &[
                ("schema", json!(1)),
                ("requested_mode", json!("deep")),
                ("request_boot_id", boot()),
                ("started", json!(super::wall())),
                ("stages", json!([])),
                ("pm_test", pm_test),
            ],
        );
    }
    if let Some(mode) = s.mode {
        value.insert("mode".into(), json!(mode));
    }
    if s.stage == "product_failure" {
        value.insert(
            "error".into(),
            s.reason.clone().map_or(Value::Null, Value::String),
        );
    }
    let controllers: Map<String, Value> = root
        .glob("/sys/bus/platform/drivers/y2-usb/*/status")
        .iter()
        .map(|p| (parent_name(p), text_value(read(p, 8192))))
        .collect();
    let udcs: Map<String, Value> = root
        .glob("/sys/class/udc/*")
        .iter()
        .map(|p| (name(p), text_value(read(&p.join("state"), 131_072))))
        .collect();
    let current = root.read("/proc/sys/kernel/random/boot_id");
    let result = s.result.map_or(Value::Null, Value::from);
    set(
        &mut value,
        &[
            ("stage", json!(s.stage)),
            ("result", result.clone()),
            ("updated", json!(super::wall())),
            ("current_boot_id", text_value(current.clone())),
            (
                "persistent_kernel",
                text_value(root.read("/sys/firmware/y2_pm/state")),
            ),
            (
                "usb_restore",
                json!({"controllers": controllers, "udcs": udcs}),
            ),
            (
                "radio_restore",
                text_value(root.read("/sys/devices/platform/18070000.connectivity/status")),
            ),
        ],
    );
    let battery = || {
        root.json_object("/run/y2/power.json")
            .get("battery")
            .cloned()
            .unwrap_or(Value::Null)
    };
    let voltage = || {
        root.integer("/sys/class/power_supply/BAT0/voltage_now")
            .map_or(Value::Null, Value::from)
    };
    if s.stage == "kernel_suspend" {
        value.insert(
            "wake_irqs_before".into(),
            Value::Object(wake_irqs(root.read("/proc/interrupts").as_deref())),
        );
        let entry = json!({"boottime_s": super::boottime(), "monotonic_s": super::monotonic(),
                           "voltage_uv": voltage(), "battery": battery()});
        value.insert("entry".into(), entry.clone());
        // Maintenance returns update entry for the next segment. Retain the actual first
        // pre-suspend ADC/timestamp for the whole sleep interval.
        value.entry("first_entry").or_insert(entry);
    } else if s.stage == "kernel_resumed" || s.stage == "restoring_radios" {
        value.insert(
            "wake_irqs_after".into(),
            Value::Object(wake_irqs(root.read("/proc/interrupts").as_deref())),
        );
    }
    if s.stage == "kernel_resumed" {
        let spm = spm_state(root);
        value.insert(
            "resume".into(),
            json!({"boottime_s": super::boottime(), "monotonic_s": super::monotonic(),
                   "spm": spm.clone(), "voltage_uv": voltage(), "battery": battery()}),
        );
        let cycles = py_add(value.get("cycles"), 1.0, true)?;
        value.insert("cycles".into(), cycles);
        // Only returned reset-path PCM596 entry counts, never a PM test/abort.
        let completed = transition(&value, s.stage, &result, current.as_deref(), None)?;
        if completed.get("kernel_completed").is_some_and(truthy) {
            let text = match spm.get("ticks32k") {
                Some(Value::String(t)) => t.clone(),
                _ => "0".into(),
            };
            let ticks = python_int(&text).ok_or_else(|| {
                E::value(format!(
                    "invalid literal for int() with base 10: {}",
                    y2p_core::py::repr(&text)
                ))
            })?;
            let seconds = py_add(
                value.get("suspended_seconds"),
                ticks as f64 / 32768.0,
                false,
            )?;
            value.insert("suspended_seconds".into(), seconds);
        }
    }
    if s.stage == "maintenance_wake" {
        let wakes = py_add(value.get("maintenance_wakes"), 1.0, true)?;
        value.insert("maintenance_wakes".into(), wakes);
    }
    if let Some(ack) = s.reborn_ack {
        value.insert("reborn_prepared".into(), json!(ack));
    }
    append_stage(
        &mut value,
        json!({"stage": s.stage, "time": super::wall(), "result": result}),
    )?;
    if s.stage.starts_with("complete_result_") {
        // An earlier successful restore (screen before radios) is reused; asking again would redraw
        // and reset Reborn's wake gesture. A dark restore (refused sleep, F47) keeps the screen off.
        let acknowledgement = match s.restore_ack.filter(|a| !a.is_empty()) {
            Some(ack) => Ok(ack),
            None => application_rpc(root, "sleep_restore", s.dark),
        };
        let restored = match acknowledgement {
            Ok(ack) => {
                is_true(ack.get("sleep_ready")) && ack.get("screen_off") == Some(&json!(s.dark))
            }
            Err(error) if error.is_os() || error.is_value() => false,
            Err(error) => return Err(error),
        };
        value.insert("reborn_restore".into(), json!(restored));
        if !restored {
            let kept = value.get("result").cloned().unwrap_or(Value::Null);
            value.insert("result".into(), if truthy(&kept) { kept } else { json!(1) });
            value.insert("error".into(), json!("reborn_resume_probe_failed"));
        }
    }
    let result = value.get("result").cloned().unwrap_or(Value::Null);
    let current = value
        .get("current_boot_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let product = transition(
        &value,
        s.stage,
        &result,
        current.as_deref(),
        s.reason.as_deref(),
    )?;
    let product_state = state(&product).to_owned();
    if s.stage == "complete_result_0" && !matches!(product_state.as_str(), "restored" | "idle") {
        value.insert("result".into(), json!(1));
        value.insert(
            "error".into(),
            product.get("reason").cloned().unwrap_or(Value::Null),
        );
    }
    value.insert("product".into(), Value::Object(product));
    if product_state == "restored" {
        let marker = root.path("/sys/firmware/y2_pm/stage");
        if marker.exists() {
            // Path.write_text: a buffered text write, not the helper's single PM control write.
            fs::write(&marker, "REBORN_READY\n").map_err(|e| E::os(&e, Some(&marker)))?;
        }
    }
    atomic_json(&path, &value, true)?;
    let battery = match root.json_object("/run/y2/power.json").get("battery") {
        Some(b) => b.clone(),
        None => json!({}),
    };
    journal::suspend(root, &battery, &format!("suspend_{}", s.stage))?;
    if matches!(
        s.stage,
        "kernel_suspend" | "kernel_resumed" | "restoring_radios" | "product_failure"
    ) || s.stage.starts_with("complete_result_")
    {
        diagnostics::record(root, s.stage)?;
    }
    let parent = directory.parent().unwrap_or(Path::new("/"));
    let handle = File::options()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open(parent)
        .map_err(|e| E::os(&e, Some(parent)))?;
    handle.sync_all().map_err(|e| E::os(&e, None))?;
    Ok(value)
}

/// `value.setdefault('stages', []).append(row); value['stages'] = value['stages'][-32:]`.
fn append_stage(value: &mut Map<String, Value>, row: Value) -> Result<(), E> {
    let stages = value.entry("stages").or_insert_with(|| json!([]));
    let Value::Array(list) = stages else {
        return Err(E::Crash("object has no attribute 'append'".into()));
    };
    list.push(row);
    if list.len() > 32 {
        list.drain(..list.len() - 32);
    }
    Ok(())
}

/// `suspend_record.progress`: one stage row in the receipt, durably, reading no sysfs and changing
/// no product state: locates a stall between two full records.
pub fn progress(root: &Root, stage: &str) -> Result<(), E> {
    let directory = root.path(PLATFORM);
    private_directory(&directory)?;
    let mut value = root.json_object(RECEIPT);
    append_stage(
        &mut value,
        json!({"stage": stage, "time": super::wall(), "boottime_s": super::boottime()}),
    )?;
    atomic_json(&directory.join("suspend-last.json"), &value, true)
}

fn name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn parent_name(path: &Path) -> String {
    path.parent().map(name).unwrap_or_default()
}

const RESPONSE_LIMIT: usize = 131_072;
const RPC_TIMEOUT_MS: i32 = 5000;

fn timed_out() -> E {
    E::Os {
        class: "TimeoutError",
        text: "timed out".into(),
        errno: None,
    }
}

/// poll() one event on `fd` within the time left before `deadline` (CPython's `internal_select`).
fn wait(fd: i32, events: i16, deadline: std::time::Instant) -> Result<(), E> {
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let mut poll = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        // SAFETY: one valid pollfd.
        let ready =
            unsafe { libc::poll(&mut poll, 1, left.as_millis().min(i32::MAX as u128) as i32) };
        if ready > 0 {
            return Ok(());
        }
        if ready == 0 {
            return Err(timed_out());
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINTR) {
            return Err(E::os(&error, None));
        }
        interrupted()?;
    }
}

fn last_error() -> std::io::Error {
    std::io::Error::last_os_error()
}

/// `application_rpc(ctx, operation, **fields)`: a bounded explicit application acknowledgement over
/// Reborn's control socket, never just process liveness. `dark` adds `"dark": true` (F47).
pub fn application_rpc(root: &Root, operation: &str, dark: bool) -> Result<Map<String, Value>, E> {
    let op = dumps(&operation, Separators::Spaced).map_err(|e| E::value(e.to_string()))?;
    let request = format!(
        "{{\"version\": 1, \"id\": 6582, \"command\": {{\"op\": {op}{}}}}}\n",
        if dark { ", \"dark\": true" } else { "" }
    );
    let path = root.path("/run/reborn/control.sock");
    // SAFETY: plain socket(2); the descriptor is owned below.
    let raw = unsafe {
        libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
            0,
        )
    };
    if raw < 0 {
        return Err(E::os(&last_error(), None));
    }
    // SAFETY: `raw` is a fresh descriptor nobody else owns.
    let socket = unsafe { OwnedFd::from_raw_fd(raw) };
    let fd = socket.as_raw_fd();
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: a zeroed sockaddr_un is valid.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.len() > address.sun_path.len() {
        return Err(E::Os {
            class: "OSError",
            text: "AF_UNIX path too long".into(),
            errno: None,
        });
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (slot, byte) in address.sun_path.iter_mut().zip(bytes) {
        *slot = *byte as libc::c_char;
    }
    let length = (std::mem::size_of::<libc::sa_family_t>() + bytes.len()) as libc::socklen_t;
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_millis(RPC_TIMEOUT_MS as u64);
    // SAFETY: `address` is an initialised sockaddr_un of `length` bytes.
    let connected =
        unsafe { libc::connect(fd, (&address as *const libc::sockaddr_un).cast(), length) };
    if connected < 0 {
        let error = last_error();
        match error.raw_os_error() {
            Some(libc::EINPROGRESS) | Some(libc::EINTR) => {
                interrupted()?;
                wait(fd, libc::POLLOUT, deadline)?;
                let mut code: libc::c_int = 0;
                let mut size = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
                // SAFETY: SO_ERROR into a c_int.
                unsafe {
                    libc::getsockopt(
                        fd,
                        libc::SOL_SOCKET,
                        libc::SO_ERROR,
                        (&mut code as *mut libc::c_int).cast(),
                        &mut size,
                    )
                };
                if code != 0 {
                    return Err(E::os(&std::io::Error::from_raw_os_error(code), None));
                }
            }
            _ => return Err(E::os(&error, None)),
        }
    }
    interrupted()?;
    // sendall: one deadline for the whole request.
    let mut data = request.as_bytes();
    let send_deadline =
        std::time::Instant::now() + std::time::Duration::from_millis(RPC_TIMEOUT_MS as u64);
    while !data.is_empty() {
        wait(fd, libc::POLLOUT, send_deadline)?;
        // SAFETY: `data` is a valid buffer of its length.
        let sent = unsafe { libc::send(fd, data.as_ptr().cast(), data.len(), libc::MSG_NOSIGNAL) };
        if sent < 0 {
            let error = last_error();
            match error.raw_os_error() {
                Some(libc::EINTR) => interrupted()?,
                Some(libc::EAGAIN) => {}
                _ => return Err(E::os(&error, None)),
            }
            continue;
        }
        data = &data[sent as usize..];
    }
    let mut response: Vec<u8> = Vec::new();
    let mut block = [0u8; 4096];
    while !response.contains(&b'\n') && response.len() <= RESPONSE_LIMIT {
        // Each recv has its own 5-s timeout.
        let until =
            std::time::Instant::now() + std::time::Duration::from_millis(RPC_TIMEOUT_MS as u64);
        wait(fd, libc::POLLIN, until)?;
        // SAFETY: `block` is writable for its length.
        let got = unsafe { libc::recv(fd, block.as_mut_ptr().cast(), block.len(), 0) };
        if got < 0 {
            let error = last_error();
            match error.raw_os_error() {
                Some(libc::EINTR) => {
                    interrupted()?;
                    continue;
                }
                Some(libc::EAGAIN) => continue,
                _ => return Err(E::os(&error, None)),
            }
        }
        if got == 0 {
            break;
        }
        response.extend_from_slice(&block[..got as usize]);
    }
    drop(socket);
    interrupted()?;
    if response.len() > RESPONSE_LIMIT {
        return Err(E::value("reborn_sleep_response_too_large"));
    }
    let line = response.split(|b| *b == b'\n').next().unwrap_or_default();
    let answer = pyjson::loads(line).map_err(|e| E::Value {
        class: e.class,
        text: e.text,
    })?;
    let Value::Object(answer) = answer else {
        return Err(E::value("reborn_sleep_acknowledgement_failed"));
    };
    if !answer.get("id").is_some_and(|id| py_eq(id, &json!(6582))) {
        return Err(E::value("reborn_sleep_acknowledgement_failed"));
    }
    if !is_true(answer.get("ok")) {
        return Err(E::value(match answer.get("error") {
            Some(Value::String(error)) if error.chars().count() <= 256 => error.clone(),
            _ => "reborn_sleep_acknowledgement_failed".into(),
        }));
    }
    match answer.get("result") {
        Some(Value::Object(result)) => Ok(result.clone()),
        _ => Err(E::value("reborn_sleep_acknowledgement_invalid")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_irqs_sum_the_pmic_children() {
        let text = "           CPU0       CPU1\n 160:         12          3  mt6397  14 Edge  mt6397-rtc\n\
                    161:          5          1  mt6397  15 Edge  mtk-pmic-keys\nx 9 mtk-pmic-keys\n";
        let counts = wake_irqs(Some(text));
        assert_eq!(Value::Object(counts), json!({"rtc": 15, "power": 6}));
        assert!(wake_irqs(None).is_empty());
    }

    #[test]
    fn transitions_follow_sleep_py() {
        let receipt = json!({"pm_test": "none", "persistent_kernel":
            "valid=1 stage=EXIT error=0 reset_resume_entry=0x59325253 pcm_length=596 wake=0x4",
            "wake_irqs_before": {"power": 2, "rtc": 1}, "wake_irqs_after": {"power": 4, "rtc": 1},
            "product": {"state": "sleeping", "request_boot_id": "b"}});
        let value = receipt.as_object().unwrap();
        let p = transition(value, "kernel_resumed", &json!(0), Some("b"), None).unwrap();
        assert_eq!(p["state"], "waking");
        assert_eq!(p["kernel_completed"], true);
        assert_eq!(p["wake_reason"], "power");
        let refused =
            transition(value, "refused_charger_active", &json!(2), Some("b"), None).unwrap();
        assert_eq!(
            (refused["state"].clone(), refused["reason"].clone()),
            (json!("refused"), json!("charger_active"))
        );
        let other = transition(value, "complete_result_0", &json!(0), Some("c"), None).unwrap();
        assert_eq!(other["reason"], "same_boot_not_proven");
    }
}
