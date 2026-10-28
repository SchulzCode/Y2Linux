//! The power journal row every suspend receipt writes (`PowerJournal(ctx).observe('Suspend', battery,
//! event='suspend_<stage>')` in `suspend_record.record`): bounded incident history on Y2DATA, compact
//! JSON in insertion order (not sorted), one durable row per stage.
//!
//! A fresh `PowerJournal` changes nothing in `observe_usb` (its first call only takes the UDC
//! signature) and always writes, because an event is given. Every failure is counted and dropped:
//! a journal failure must never delay or disable battery protection.
// SPDX-License-Identifier: GPL-2.0-only
use super::{append, diagnostics, text_value, E};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use y2p_core::{
    json::{dumps, Separators},
    root::{python_int, read},
    suspend_text::{split, splitlines},
    Root,
};

pub const PATH: &str = "/data/system/platform/power-journal.jsonl";
// Two bounded files. POWER-FINAL-16: rows grew to ~6.6 KB and refusal rows add to the five-minute
// snapshot, so 2 MiB kept only ~4.5 h (archive 109 lost the morning). 8 MiB keeps more than a day
// per file; no change in write rate.
const LIMIT: u64 = 8_388_608;
const RUNTIME: [&str; 8] = [
    "11200000.",
    "11220000.",
    "11230000.",
    "11240000.",
    "13010000.",
    "14007000.",
    "14008000.",
    "18070000.",
];

/// A JSON value whose objects keep insertion order (a Python dict written without `sort_keys`).
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    V(Value),
    O(Vec<(String, Node)>),
    L(Vec<Node>),
}

impl Serialize for Node {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            Node::V(value) => value.serialize(serializer),
            Node::L(items) => items.serialize(serializer),
            Node::O(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

fn name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn int(value: Option<i64>) -> Node {
    Node::V(value.map_or(Value::Null, Value::from))
}

/// Cheap cumulative counters only when a journal row is already due.
///
/// No AUXADC, subprocess, RPC, tracing, or additional periodic wakeup. Missing interfaces stay null.
/// Hotplug can reset per-core counters; readers must not turn decreasing values into negative rates.
pub fn idle_counters(root: &Root) -> Node {
    let started = super::process_time();
    let mut idle = Vec::new();
    for cpu in 0..4 {
        let mut states: Vec<(String, Node)> = Vec::new();
        for index in 0..3 {
            let base = format!("/sys/devices/system/cpu/cpu{cpu}/cpuidle/state{index}");
            if let Some(state) = root.read_limited(&format!("{base}/name"), 64) {
                let fields = ["usage", "time", "rejected", "disable"]
                    .iter()
                    .map(|f| ((*f).to_owned(), int(root.integer(&format!("{base}/{f}")))))
                    .collect();
                set(&mut states, &state, Node::O(fields));
            }
        }
        if !states.is_empty() {
            idle.push((cpu.to_string(), Node::O(states)));
        }
    }
    let mut scheduler: Vec<(String, Node)> = Vec::new();
    for line in splitlines(&root.read("/proc/stat").unwrap_or_default()) {
        let fields = split(line);
        if let Some(&first) = fields.first() {
            if ["ctxt", "intr", "processes"].contains(&first) {
                let value = fields.get(1).and_then(|f| python_int(f));
                set(&mut scheduler, first, int(value));
            }
        }
    }
    let runtime = root
        .glob("/sys/bus/platform/devices/*/power/runtime_status")
        .iter()
        .filter_map(|path| {
            let device = path.parent()?.parent().map(name)?;
            RUNTIME
                .iter()
                .any(|p| device.starts_with(p))
                .then(|| (device, Node::V(text_value(read(path, 64)))))
        })
        .collect();
    let each = |pattern: &str| {
        Node::O(
            root.glob(pattern)
                .iter()
                .map(|p| (name(p), Node::V(text_value(read(p, 128)))))
                .collect(),
        )
    };
    let mut result = vec![
        (
            "online".to_owned(),
            Node::V(text_value(
                root.read_limited("/sys/devices/system/cpu/online", 64),
            )),
        ),
        ("cpuidle".into(), Node::O(idle)),
        ("scheduler".into(), Node::O(scheduler)),
        ("runtime_pm".into(), Node::O(runtime)),
        (
            "frequency_khz".into(),
            int(root.integer("/sys/devices/system/cpu/cpufreq/policy0/scaling_cur_freq")),
        ),
        (
            "frequency_ticks".into(),
            Node::V(text_value(root.read_limited(
                "/sys/devices/system/cpu/cpufreq/policy0/stats/time_in_state",
                512,
            ))),
        ),
        (
            "idle_rejections".into(),
            each("/sys/module/idle/parameters/*"),
        ),
        (
            "clock_blockers".into(),
            each("/sys/module/clocks/parameters/*blockers"),
        ),
        (
            "counter_reset_rule".into(),
            Node::V("decreasing per-core counters are unknown, not negative rates".into()),
        ),
    ];
    result.push((
        "collector_cpu_seconds".into(),
        Node::V((super::process_time() - started).into()),
    ));
    Node::O(result)
}

/// `dict[key] = value` on an ordered object.
fn set(entries: &mut Vec<(String, Node)>, key: &str, value: Node) {
    match entries.iter_mut().find(|(k, _)| k == key) {
        Some(entry) => entry.1 = value,
        None => entries.push((key.to_owned(), value)),
    }
}

/// One `suspend_<stage>` row. `battery` is `/run/y2/power.json`'s `battery` (or `{}`).
pub fn suspend(root: &Root, battery: &Value, event: &str) -> Result<bool, E> {
    let now = super::monotonic();
    // signature = (state, battery.get('status'), battery.get('charging')): outside the try.
    if !battery.is_object() {
        return Err(E::Crash(format!(
            "'{}' object has no attribute 'get'",
            python_type(battery)
        )));
    }
    match row(root, battery, event, now) {
        Ok(written) => Ok(written),
        Err(error) if error.is_recorded_failure() => Ok(false),
        Err(error) => Err(error),
    }
}

fn python_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) if n.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

fn row(root: &Root, battery: &Value, event: &str, now: f64) -> Result<bool, E> {
    // Do not create persistent state on an unverified/missing data mount.
    if !diagnostics::data_ready(root) {
        return Ok(false);
    }
    let receipt = root.json_object(super::receipt::RECEIPT);
    let boot_id = root.read("/proc/sys/kernel/random/boot_id");
    let usb = root
        .glob("/sys/class/udc/*")
        .iter()
        .map(|p| {
            (
                name(p),
                Node::V(text_value(read(&p.join("state"), 131_072))),
            )
        })
        .collect();
    let usb_net = diagnostics::usb_net(root)?;
    let backlight = root
        .glob("/sys/class/backlight/*/bl_power")
        .iter()
        .map(|p| {
            (
                p.parent().map(name).unwrap_or_default(),
                Node::V(text_value(read(p, 131_072))),
            )
        })
        .collect();
    let radios = root
        .glob("/sys/class/rfkill/*")
        .iter()
        .map(|p| {
            (
                name(p),
                Node::O(vec![
                    (
                        "type".into(),
                        Node::V(text_value(read(&p.join("type"), 131_072))),
                    ),
                    (
                        "blocked".into(),
                        Node::V(text_value(read(&p.join("soft"), 131_072))),
                    ),
                ]),
            )
        })
        .collect();
    let idle_policy = root
        .json("/run/y2/cpu-idle-policy.json")
        .unwrap_or(Value::Null);
    let idle_counters = idle_counters(root);
    let sleep = super::super::sleep::product(
        &root.json_object(super::receipt::RECEIPT),
        boot_id.as_deref(),
    )
    .map_err(|_| E::TypeError("unhashable type".into()))?;
    let mut summary: Vec<(String, Node)> = [
        "mode",
        "stage",
        "first_entry",
        "resume",
        "cycles",
        "suspended_seconds",
        "maintenance_wakes",
        "error",
    ]
    .iter()
    .map(|k| {
        (
            (*k).to_owned(),
            Node::V(receipt.get(*k).cloned().unwrap_or(Value::Null)),
        )
    })
    .collect();
    // POWER-FINAL-18: 17 lost why two requests were refused.
    let reason = match receipt.get("product") {
        Some(Value::Object(product)) => product.get("reason").cloned().unwrap_or(Value::Null),
        Some(other) if y2p_core::py::truthy(other) => {
            return Err(E::Crash(format!(
                "'{}' object has no attribute 'get'",
                python_type(other)
            )))
        }
        _ => Value::Null,
    };
    summary.push(("reason".into(), Node::V(reason)));
    let node = Node::O(vec![
        ("schema".into(), Node::V(1.into())),
        ("event".into(), Node::V(event.into())),
        ("wall_time".into(), Node::V(super::wall().into())),
        ("monotonic_s".into(), Node::V(now.into())),
        ("boottime_s".into(), Node::V(super::boottime().into())),
        ("boot_id".into(), Node::V(text_value(boot_id))),
        (
            "build".into(),
            Node::V(
                root.json("/etc/y2linux/versions.json")
                    .unwrap_or(Value::Null),
            ),
        ),
        ("state".into(), Node::V("Suspend".into())),
        ("battery".into(), Node::V(battery.clone())),
        ("shutdown".into(), Node::V(Value::Null)),
        ("usb".into(), Node::O(usb)),
        ("usb_net".into(), usb_net),
        (
            "charger".into(),
            Node::V(text_value(
                root.read("/sys/class/power_supply/BAT0/charging_state"),
            )),
        ),
        ("backlight".into(), Node::O(backlight)),
        ("radios".into(), Node::O(radios)),
        ("idle_policy".into(), Node::V(idle_policy)),
        ("idle_counters".into(), idle_counters),
        ("sleep".into(), Node::V(Value::Object(sleep))),
        // Preserve completed intervals when a later refusal replaces suspend-last.json. This
        // shares the existing event cadence.
        ("suspend".into(), Node::O(summary)),
        (
            "kernel_pm".into(),
            Node::V(text_value(
                root.read_limited("/sys/firmware/y2_pm/state", 8192),
            )),
        ),
    ]);
    let encoded = dumps(&node, Separators::Compact).map_err(|e| E::value(e.to_string()))? + "\n";
    if encoded.len() > 16384 {
        return Err(E::value("power_journal_row_too_large"));
    }
    let path = root.path(PATH);
    append::append_bounded(&path, encoded.as_bytes(), LIMIT, true)
        .map_err(|e| E::os(&e, Some(&path)))?;
    Ok(true)
}
