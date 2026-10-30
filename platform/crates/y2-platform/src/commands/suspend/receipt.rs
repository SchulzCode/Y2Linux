//! The suspend receipt (`suspend-last.json`) and the Power/RTC wake counts: durable records, written
//! only before entry or after Linux returns. No lock of its own: the caller holds
//! `/run/y2/suspend.lock` (sleep request, the helper, y2-suspend).
//!
//! The receipt is a typed record. One that does not read (a field of another type, a product that is
//! no object) is refused as `suspend_receipt_unreadable`: the sleep path fails closed, and the next
//! request starts a new receipt. A receipt file that is no JSON object reads as none.
// SPDX-License-Identifier: GPL-2.0-only
mod rpc;
mod transition;

pub use rpc::{application_rpc, SleepAck};
pub use transition::transition;

use super::{
    admission::spm_state, diagnostics, int_literal, journal, name, parent_name, pattern, E,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    os::unix::fs::OpenOptionsExt,
    path::Path,
    sync::OnceLock,
};
use y2p_core::{
    lenient,
    root::{parse_int, read},
    suspend_text::Regex,
    Root,
};

pub const RECEIPT: &str = "/data/system/platform/suspend-last.json";
const PLATFORM: &str = "/data/system/platform";
/// The refusal for a receipt that does not read.
pub const UNREADABLE: &str = "suspend_receipt_unreadable";

/// The Power and RTC child-IRQ counts (`rtc`, `power`).
pub type WakeIrqs = BTreeMap<String, i64>;

/// Serviced PMIC child IRQs, matching the existing owner harness names.
///
/// Both RTC and Power feed SPM EINT. That shared SPM wake bit is deliberately insufficient; only a
/// same-boot child-IRQ delta attributes their source.
pub fn wake_irqs(text: Option<&str>) -> WakeIrqs {
    let mut counts = WakeIrqs::new();
    for row in y2p_core::text::interrupt_rows(text.unwrap_or_default()) {
        for (label, name) in [("mt6397-rtc", "rtc"), ("mtk-pmic-keys", "power")] {
            if !row.rest.contains(&label) {
                continue;
            }
            let total: i64 = row
                .counts
                .iter()
                .map(|c| c.parse::<i64>().unwrap_or(0))
                .sum();
            *counts.entry(name.into()).or_insert(0) += total;
        }
    }
    counts
}

/// The product sleep state (`product.state`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductState {
    Idle,
    Requested,
    Preparing,
    Refused,
    Sleeping,
    Restoring,
    Waking,
    Restored,
    RestoreFailed,
}

/// The product state a receipt keeps (`product`) and `sleep status` prints: proven only by the same
/// boot's kernel journal and Reborn's acknowledgement. Read leniently: a state no version writes is
/// none (the receipt then reads as idle); fields of other versions are kept.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Product {
    #[serde(deserialize_with = "lenient::or_none")]
    pub state: Option<ProductState>,
    #[serde(deserialize_with = "lenient::text")]
    pub reason: Option<String>,
    #[serde(deserialize_with = "lenient::text")]
    pub request_boot_id: Option<String>,
    #[serde(deserialize_with = "lenient::or_none")]
    pub same_boot: Option<bool>,
    #[serde(deserialize_with = "lenient::is_true")]
    pub kernel_completed: bool,
    #[serde(deserialize_with = "lenient::text")]
    pub wake_reason: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    pub schema: Option<String>,
    /// The boot that read it (`sleep status`).
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::there"
    )]
    pub boot_id: Option<Option<String>>,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

impl Product {
    /// The product of a receipt that holds no known state.
    pub fn idle() -> Product {
        Product {
            state: Some(ProductState::Idle),
            wake_reason: Some("unknown".into()),
            ..Product::default()
        }
    }

    pub fn state_is(&self, state: ProductState) -> bool {
        self.state == Some(state)
    }

    /// Set the state and its reason.
    pub fn set(&mut self, state: ProductState, reason: Option<String>) {
        self.state = Some(state);
        self.reason = reason;
    }
}

/// One row of the receipt's `stages` (the last 32 are kept): a full record's (`result`) or a
/// progress row's (`boottime_s`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StageRow {
    pub stage: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::there"
    )]
    pub result: Option<Option<i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boottime_s: Option<f64>,
}

/// The battery and clocks at a suspend entry (`entry`, `first_entry`) or a return (`resume`, with
/// the SPM state). `battery` is power.json's as it was.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Moment {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boottime_s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monotonic_s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spm: Option<BTreeMap<String, String>>,
    /// `null`: the voltage did not read.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::there"
    )]
    pub voltage_uv: Option<Option<i64>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::there"
    )]
    pub battery: Option<Value>,
}

impl Moment {
    /// The clocks, battery and voltage now (`spm` for a return).
    fn now(root: &Root, spm: Option<BTreeMap<String, String>>) -> Moment {
        Moment {
            boottime_s: Some(super::boottime()),
            monotonic_s: Some(super::monotonic()),
            spm,
            voltage_uv: Some(root.integer("/sys/class/power_supply/BAT0/voltage_now")),
            battery: Some(battery(root)),
        }
    }
}

/// The USB state a receipt keeps (`usb_restore`): each controller's status, each UDC's state.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct UsbRestore {
    pub controllers: BTreeMap<String, Option<String>>,
    pub udcs: BTreeMap<String, Option<String>>,
}

/// suspend-last.json. Fields this version does not know are kept as they are.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Receipt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_mode: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::there"
    )]
    pub request_boot_id: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started: Option<f64>,
    #[serde(default)]
    pub stages: Vec<StageRow>,
    /// The selected `/sys/power/pm_test` mode at the start (`null`: none selected).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::there"
    )]
    pub pm_test: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::there"
    )]
    pub error: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(default)]
    pub result: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<f64>,
    #[serde(default)]
    pub current_boot_id: Option<String>,
    #[serde(default)]
    pub persistent_kernel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usb_restore: Option<UsbRestore>,
    #[serde(default)]
    pub radio_restore: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wake_irqs_before: Option<WakeIrqs>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wake_irqs_after: Option<WakeIrqs>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<Moment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_entry: Option<Moment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<Moment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycles: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suspended_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maintenance_wakes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reborn_prepared: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reborn_restore: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "product"
    )]
    pub product: Option<Product>,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

/// A product that is an object (`null`: none); anything else does not read.
fn product<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<Product>, D::Error> {
    match Value::deserialize(deserializer)? {
        Value::Null => Ok(None),
        object @ Value::Object(_) => Product::deserialize(object)
            .map(Some)
            .map_err(serde::de::Error::custom),
        _ => Err(serde::de::Error::custom("product is no object")),
    }
}

impl Receipt {
    /// The receipt on Y2DATA: none for a missing file or one that is no JSON object;
    /// `suspend_receipt_unreadable` for an object that does not read as a receipt.
    pub fn read(root: &Root) -> Result<Receipt, E> {
        match root.json(RECEIPT) {
            Some(Value::Object(map)) => serde_json::from_value(Value::Object(map))
                .map_err(|_| E::Invalid(UNREADABLE.into())),
            _ => Ok(Receipt::default()),
        }
    }

    /// The product state as this record left it (none: idle).
    pub fn product(&self) -> Option<&Product> {
        self.product.as_ref()
    }

    /// The product state, or an empty one.
    pub fn into_product(self) -> Product {
        self.product.unwrap_or_default()
    }

    /// Linux returned through the qualified resume path on this boot (`product.kernel_completed`).
    pub fn kernel_completed(&self) -> bool {
        self.product.as_ref().is_some_and(|p| p.kernel_completed)
    }

    /// The Power/RTC IRQ counts after the return (none when there are none).
    pub fn wake_irqs_after(&self) -> WakeIrqs {
        self.wake_irqs_after.clone().unwrap_or_default()
    }

    /// The result the record kept, when it is a failure (not 0 and not none).
    pub fn failed_result(&self) -> Option<i64> {
        self.result.filter(|r| *r != 0)
    }

    /// The record names an error (`y2-suspend-record` exits 1 then).
    pub fn has_error(&self) -> bool {
        matches!(&self.error, Some(Some(error)) if !error.is_empty())
    }

    /// The error the record names (a non-empty text).
    pub fn error(&self) -> Option<&str> {
        match &self.error {
            Some(Some(error)) if !error.is_empty() => Some(error),
            _ => None,
        }
    }

    /// The fields a journal row keeps of it (`suspend`).
    pub fn summary(&self) -> Summary {
        Summary {
            mode: self.mode.clone(),
            stage: self.stage.clone(),
            first_entry: self.first_entry.clone(),
            resume: self.resume.clone(),
            cycles: self.cycles,
            suspended_seconds: self.suspended_seconds,
            maintenance_wakes: self.maintenance_wakes,
            error: self.error.clone().flatten(),
            // POWER-FINAL-18: 17 lost why two requests were refused.
            reason: self.product.as_ref().and_then(|p| p.reason.clone()),
        }
    }

    /// Add a stage row, keeping the last 32.
    fn append_stage(&mut self, row: StageRow) {
        self.stages.push(row);
        if self.stages.len() > 32 {
            self.stages.drain(..self.stages.len() - 32);
        }
    }
}

/// The receipt's fields a journal row keeps, so that a later refusal replacing suspend-last.json
/// does not lose the completed intervals.
#[derive(Serialize, Debug)]
pub struct Summary {
    pub mode: Option<String>,
    pub stage: Option<String>,
    pub first_entry: Option<Moment>,
    pub resume: Option<Moment>,
    pub cycles: Option<i64>,
    pub suspended_seconds: Option<f64>,
    pub maintenance_wakes: Option<i64>,
    pub error: Option<String>,
    pub reason: Option<String>,
}

/// `record(ctx, stage, result, reason, reborn_ack, mode, restore_ack, dark)`'s arguments.
#[derive(Default)]
pub struct Stage<'a> {
    pub stage: &'a str,
    pub result: Option<i64>,
    pub reason: Option<String>,
    pub reborn_ack: Option<bool>,
    pub mode: Option<&'a str>,
    pub restore_ack: Option<SleepAck>,
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

/// Whether `record` starts a new receipt for this stage (`reborn_prepared` of the receipt so far).
pub fn starts_receipt(stage: &str, prepared: bool) -> bool {
    stage == "requested"
        || (stage == "quiescing_radios" && !prepared)
        || stage.starts_with("refused_")
}

/// power.json's `battery` as it is (`null` without one).
fn battery(root: &Root) -> Value {
    root.json_object("/run/y2/power.json")
        .get("battery")
        .cloned()
        .unwrap_or(Value::Null)
}

/// `suspend_record.record`.
pub fn record(root: &Root, s: Stage) -> Result<Receipt, E> {
    let directory = root.path(PLATFORM);
    E::private_directory(&directory)?;
    let path = directory.join("suspend-last.json");
    let previous = Receipt::read(root);
    let prepared = matches!(&previous, Ok(r) if r.reborn_prepared == Some(true));
    // A receipt found unreadable fails the stage that found it; the failure and the completion are
    // still recorded, in a new receipt.
    let recorded_anyway = previous.is_err()
        && (s.stage == "product_failure" || s.stage.starts_with("complete_result_"));
    let mut value = if starts_receipt(s.stage, prepared) || recorded_anyway {
        static SELECTED: OnceLock<Option<Regex>> = OnceLock::new();
        let modes = root.read("/sys/power/pm_test").unwrap_or_default();
        let pm_test = pattern(&SELECTED, r"\[(\w+)\]")
            .and_then(|p| p.captures(&modes))
            .map(|c| c[1].to_owned());
        Receipt {
            schema: Some(1),
            requested_mode: Some("deep".into()),
            request_boot_id: Some(root.boot_id()),
            started: Some(super::wall()),
            pm_test: Some(pm_test),
            ..Receipt::default()
        }
    } else {
        previous?
    };
    if let Some(mode) = s.mode {
        value.mode = Some(mode.into());
    }
    if s.stage == "product_failure" {
        value.error = Some(s.reason.clone());
    }
    let controllers = root
        .glob("/sys/bus/platform/drivers/y2-usb/*/status")
        .iter()
        .map(|p| (parent_name(p), read(p, 8192)))
        .collect();
    let udcs = root
        .glob("/sys/class/udc/*")
        .iter()
        .map(|p| (name(p), read(&p.join("state"), 131_072)))
        .collect();
    let current = root.boot_id();
    value.stage = Some(s.stage.into());
    value.result = s.result;
    value.updated = Some(super::wall());
    value.current_boot_id = current.clone();
    value.persistent_kernel = root.read("/sys/firmware/y2_pm/state");
    value.usb_restore = Some(UsbRestore { controllers, udcs });
    value.radio_restore = root.read("/sys/devices/platform/18070000.connectivity/status");
    if s.stage == "kernel_suspend" {
        value.wake_irqs_before = Some(wake_irqs(root.read("/proc/interrupts").as_deref()));
        let entry = Moment::now(root, None);
        // Maintenance returns update entry for the next segment. Retain the actual first
        // pre-suspend ADC/timestamp for the whole sleep interval.
        value.first_entry.get_or_insert_with(|| entry.clone());
        value.entry = Some(entry);
    } else if s.stage == "kernel_resumed" || s.stage == "restoring_radios" {
        value.wake_irqs_after = Some(wake_irqs(root.read("/proc/interrupts").as_deref()));
    }
    if s.stage == "kernel_resumed" {
        let spm = spm_state(root);
        let texts = spm
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
            .collect();
        value.resume = Some(Moment::now(root, Some(texts)));
        value.cycles = Some(value.cycles.unwrap_or(0) + 1);
        // Only returned reset-path PCM596 entry counts, never a PM test/abort.
        let completed = transition(&value, s.stage, s.result, current.as_deref(), None);
        if completed.kernel_completed {
            let text = match spm.get("ticks32k") {
                Some(Value::String(t)) => t.clone(),
                _ => "0".into(),
            };
            let ticks = parse_int(&text).ok_or_else(|| int_literal(&text))?;
            value.suspended_seconds =
                Some(value.suspended_seconds.unwrap_or(0.0) + ticks as f64 / 32768.0);
        }
    }
    if s.stage == "maintenance_wake" {
        value.maintenance_wakes = Some(value.maintenance_wakes.unwrap_or(0) + 1);
    }
    if let Some(ack) = s.reborn_ack {
        value.reborn_prepared = Some(ack);
    }
    value.append_stage(StageRow {
        stage: s.stage.into(),
        time: Some(super::wall()),
        result: Some(s.result),
        boottime_s: None,
    });
    if s.stage.starts_with("complete_result_") {
        // An earlier successful restore (screen before radios) is reused; asking again would redraw
        // and reset Reborn's wake gesture. A dark restore (refused sleep, F47) keeps the screen off.
        let acknowledgement = match s.restore_ack.filter(|a| !a.is_empty()) {
            Some(ack) => Ok(ack),
            None => application_rpc::<SleepAck>(root, "sleep_restore", s.dark),
        };
        let restored = match acknowledgement {
            Ok(ack) => ack.ready() && ack.screen_off == Some(s.dark),
            Err(error) if error.is_os() || error.is_value() => false,
            Err(error) => return Err(error),
        };
        value.reborn_restore = Some(restored);
        if !restored {
            value.result = Some(value.failed_result().unwrap_or(1));
            value.error = Some(Some("reborn_resume_probe_failed".into()));
        }
    }
    let product = transition(
        &value,
        s.stage,
        value.result,
        value.current_boot_id.as_deref(),
        s.reason.as_deref(),
    );
    if s.stage == "complete_result_0"
        && !matches!(
            product.state,
            Some(ProductState::Restored | ProductState::Idle)
        )
    {
        value.result = Some(1);
        value.error = Some(product.reason.clone());
    }
    let restored = product.state_is(ProductState::Restored);
    value.product = Some(product);
    if restored {
        let marker = root.path("/sys/firmware/y2_pm/stage");
        if marker.exists() {
            // Path.write_text: a buffered text write, not the helper's single PM control write.
            fs::write(&marker, "REBORN_READY\n").map_err(|e| E::os(&e, Some(&marker)))?;
        }
    }
    E::atomic_json(&path, &value, true)?;
    let battery = match root.json_object("/run/y2/power.json").get("battery") {
        Some(b) => b.clone(),
        None => Value::Object(Map::new()),
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
    handle.sync_all()?;
    Ok(value)
}

/// `suspend_record.progress`: one stage row in the receipt, durably, reading no sysfs and changing
/// no product state: locates a stall between two full records.
pub fn progress(root: &Root, stage: &str) -> Result<(), E> {
    let directory = root.path(PLATFORM);
    E::private_directory(&directory)?;
    let mut value = Receipt::read(root)?;
    value.append_stage(StageRow {
        stage: stage.into(),
        time: Some(super::wall()),
        result: None,
        boottime_s: Some(super::boottime()),
    });
    E::atomic_json(&directory.join("suspend-last.json"), &value, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wake_irqs_sum_the_pmic_children() {
        let text = "           CPU0       CPU1\n 160:         12          3  mt6397  14 Edge  mt6397-rtc\n\
                    161:          5          1  mt6397  15 Edge  mtk-pmic-keys\nx 9 mtk-pmic-keys\n";
        let counts = wake_irqs(Some(text));
        assert_eq!(json!(counts), json!({"rtc": 15, "power": 6}));
        assert!(wake_irqs(None).is_empty());
    }

    #[test]
    fn transitions_follow_sleep_py() {
        let receipt: Receipt =
            serde_json::from_value(json!({"pm_test": "none", "persistent_kernel":
            "valid=1 stage=EXIT error=0 reset_resume_entry=0x59325253 pcm_length=596 wake=0x4",
            "wake_irqs_before": {"power": 2, "rtc": 1}, "wake_irqs_after": {"power": 4, "rtc": 1},
            "product": {"state": "sleeping", "request_boot_id": "b"}}))
            .unwrap();
        let p = transition(&receipt, "kernel_resumed", Some(0), Some("b"), None);
        assert_eq!(p.state, Some(ProductState::Waking));
        assert!(p.kernel_completed);
        assert_eq!(p.wake_reason.as_deref(), Some("power"));
        let refused = transition(&receipt, "refused_charger_active", Some(2), Some("b"), None);
        assert_eq!(
            (refused.state, refused.reason.as_deref()),
            (Some(ProductState::Refused), Some("charger_active"))
        );
        let other = transition(&receipt, "complete_result_0", Some(0), Some("c"), None);
        assert_eq!(other.reason.as_deref(), Some("same_boot_not_proven"));
    }

    #[test]
    fn a_receipt_of_another_shape_is_unreadable() {
        for receipt in [
            json!({"product": 5}),
            json!({"product": [["state", "idle"]]}),
            json!({"cycles": "x"}),
        ] {
            assert!(serde_json::from_value::<Receipt>(receipt).is_err());
        }
        // A product state no version writes reads as none (idle).
        let odd: Receipt = serde_json::from_value(json!({"product": {"state": []}})).unwrap();
        assert_eq!(odd.product.unwrap().state, None);
    }
}

#[cfg(test)]
#[path = "receipt_tests.rs"]
mod receipt_tests;
