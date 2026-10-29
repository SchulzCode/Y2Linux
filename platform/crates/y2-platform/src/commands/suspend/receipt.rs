//! The suspend receipt (`suspend-last.json`) and the Power/RTC wake counts: durable records, written
//! only before entry or after Linux returns. No lock of its own: the caller holds
//! `/run/y2/suspend.lock` (sleep request, the helper, y2-suspend).
// SPDX-License-Identifier: GPL-2.0-only
mod rpc;
mod transition;

pub use rpc::application_rpc;
pub use transition::{raised, transition, transition_keys};
use transition::{set, state};

use super::{admission::spm_state, diagnostics, journal, pattern, py_add, text_value, E};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::{
    fs::{self, File},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    sync::OnceLock,
};
use y2p_core::{
    py::{is_true, truthy},
    root::{python_int, read},
    suspend_text::{split, splitlines, Regex},
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

/// The receipt as `record` wrote it (`suspend-last.json`): every key it holds, with the views the
/// helper and the commands read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Receipt(pub Map<String, Value>);

impl std::ops::Deref for Receipt {
    type Target = Map<String, Value>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Receipt {
    /// The product state (`sleep.product`'s fields as this record left them), if it is an object.
    pub fn product(&self) -> Option<&Map<String, Value>> {
        self.0.get("product").and_then(Value::as_object)
    }

    /// The product state, or an empty one.
    pub fn into_product(mut self) -> Map<String, Value> {
        match self.0.shift_remove("product") {
            Some(Value::Object(product)) => product,
            _ => Map::new(),
        }
    }

    /// Linux returned through the qualified resume path on this boot (`product.kernel_completed`).
    pub fn kernel_completed(&self) -> bool {
        self.product()
            .is_some_and(|p| is_true(p.get("kernel_completed")))
    }

    /// The Power/RTC IRQ counts after the return (`{}` when there are none).
    pub fn wake_irqs_after(&self) -> Map<String, Value> {
        self.0
            .get("wake_irqs_after")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default()
    }

    /// The result the record kept (`None` when it is falsy: 0, null or absent).
    pub fn failed_result(&self) -> Option<&Value> {
        self.0.get("result").filter(|r| truthy(r))
    }

    /// The record names an error (`y2-suspend-record` exits 1 then).
    pub fn has_error(&self) -> bool {
        self.0.get("error").is_some_and(truthy)
    }
}

/// One row of the receipt's `stages` (the last 32 are kept).
#[derive(Serialize)]
struct StageRow<'a> {
    stage: &'a str,
    time: f64,
    result: Option<i64>,
}

/// A `progress` row: no product state, only where the helper is.
#[derive(Serialize)]
struct ProgressRow<'a> {
    stage: &'a str,
    time: f64,
    boottime_s: f64,
}

/// The battery and clocks at a suspend entry (`entry`, `first_entry`) or a return (`resume`, with
/// the SPM state).
#[derive(Serialize)]
struct Moment {
    boottime_s: f64,
    monotonic_s: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    spm: Option<Map<String, Value>>,
    voltage_uv: Option<i64>,
    battery: Value,
}

/// The USB state a receipt keeps (`usb_restore`): each controller's status, each UDC's state.
#[derive(Serialize)]
struct UsbRestore {
    controllers: Map<String, Value>,
    udcs: Map<String, Value>,
}

fn to_value(value: impl Serialize) -> Result<Value, E> {
    serde_json::to_value(value).map_err(|e| E::value(e.to_string()))
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

/// `suspend_record.record`.
pub fn record(root: &Root, s: Stage) -> Result<Receipt, E> {
    let directory = root.path(PLATFORM);
    private_directory(&directory)?;
    let path = directory.join("suspend-last.json");
    let mut value = root.json_object(RECEIPT);
    let boot = || text_value(root.boot_id());
    if starts_receipt(s.stage, &value) {
        static SELECTED: OnceLock<Option<Regex>> = OnceLock::new();
        let modes = root.read("/sys/power/pm_test").unwrap_or_default();
        let pm_test = pattern(&SELECTED, r"\[(\w+)\]")
            .and_then(|p| p.captures(&modes))
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
    let current = root.boot_id();
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
            ("usb_restore", to_value(UsbRestore { controllers, udcs })?),
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
    let voltage = || root.integer("/sys/class/power_supply/BAT0/voltage_now");
    if s.stage == "kernel_suspend" {
        value.insert(
            "wake_irqs_before".into(),
            Value::Object(wake_irqs(root.read("/proc/interrupts").as_deref())),
        );
        let entry = to_value(Moment {
            boottime_s: super::boottime(),
            monotonic_s: super::monotonic(),
            spm: None,
            voltage_uv: voltage(),
            battery: battery(),
        })?;
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
        let resume = Moment {
            boottime_s: super::boottime(),
            monotonic_s: super::monotonic(),
            spm: Some(spm.clone()),
            voltage_uv: voltage(),
            battery: battery(),
        };
        value.insert("resume".into(), to_value(resume)?);
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
    let row = StageRow {
        stage: s.stage,
        time: super::wall(),
        result: s.result,
    };
    append_stage(&mut value, to_value(row)?)?;
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
    let (product, odd) = transition_keys(
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
    if odd {
        // json.dump(sort_keys=True) raises in the temporary file, which atomic_json removes.
        return Err(raised(super::super::sleep::odd_keys()));
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
    Ok(Receipt(value))
}

/// `value.setdefault('stages', []).append(row); value['stages'] = value['stages'][-32:]`.
fn append_stage(value: &mut Map<String, Value>, row: Value) -> Result<(), E> {
    let stages = value.entry("stages").or_insert_with(|| json!([]));
    let Value::Array(list) = stages else {
        return Err(E::Invalid("object has no attribute 'append'".into()));
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
    let row = ProgressRow {
        stage,
        time: super::wall(),
        boottime_s: super::boottime(),
    };
    append_stage(&mut value, to_value(row)?)?;
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

#[cfg(test)]
#[path = "receipt_tests.rs"]
mod receipt_tests;
