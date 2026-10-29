//! `power_journal.PowerJournal`: bounded incident history, driven by the existing safety sample; no new
//! timer. With it the parts of `sleep_diagnostics` the power sample drives (USB state change rows,
//! POWER-FINAL-14) and `sleep.status` as a journal row holds it.
//!
//! Rows are `json.dumps(row, separators=(',', ':'))` without sorting: built in Python's insertion
//! order, records read from files in their file order. A journal failure never delays or
//! disables battery protection: it is counted and the row dropped.
// SPDX-License-Identifier: GPL-2.0-only
use super::crash;
use crate::commands::time::wall;
use serde_json::Value;
use std::{fs, io::Read, os::unix::fs::OpenOptionsExt};
use y2p_core::mounts::data_ready;
use y2p_core::{
    json::{dumps, Separators},
    json::{Object, ObjectExt},
    power_journal::append_bounded,
    power_text::{key_values, python_hex, python_split, trie_address},
    py::PyValue,
    root::{python_int, read},
    Root,
};

pub const PATH: &str = "/data/system/platform/power-journal.jsonl";
const PERIOD: f64 = 300.0;
// Two bounded files. POWER-FINAL-16: rows grew to ~6.6 KB and refusal rows add
// to the five-minute snapshot, so 2 MiB kept only ~4.5 h (archive 109 lost the
// morning). 8 MiB keeps more than a day per file; no change in write rate.
const LIMIT: u64 = 8388608;
const ROW: usize = 16384;

/// Why a row was not written (Python's `OSError`, `ValueError`, `TypeError`, all caught).
#[derive(Debug)]
pub struct Dropped;

fn glob_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn parent_name(path: &std::path::Path) -> String {
    path.parent().map(glob_name).unwrap_or_default()
}

/// `ctx.json(name)` without a default.
fn json(root: &Root, name: &str) -> Value {
    root.json(name).unwrap_or(Value::Null)
}

/// `idle_counters(ctx)`: cheap cumulative counters only when a journal row is already due.
///
/// No AUXADC, subprocess, RPC, tracing, or additional periodic wakeup. Missing interfaces stay null.
/// Hotplug can reset per-core counters; readers must not turn decreasing values into negative rates.
pub fn idle_counters(root: &Root) -> Object {
    let started = process_time();
    let mut idle = Object::new();
    for cpu in 0..4 {
        let mut states = Object::new();
        for index in 0..3 {
            let base = format!("/sys/devices/system/cpu/cpu{cpu}/cpuidle/state{index}");
            if let Some(name) = root.read_limited(&format!("{base}/name"), 64) {
                let mut fields = Object::new();
                for field in ["usage", "time", "rejected", "disable"] {
                    fields.set(field, root.integer(&format!("{base}/{field}")));
                }
                states.set(&name, fields);
            }
        }
        if !states.is_empty() {
            idle.set(&cpu.to_string(), states);
        }
    }
    let mut scheduler = Object::new();
    for line in root.read("/proc/stat").unwrap_or_default().lines() {
        let fields = python_split(line);
        if let Some(name) = fields
            .first()
            .filter(|f| ["ctxt", "intr", "processes"].contains(f))
        {
            scheduler.set(name, fields.get(1).and_then(|v| python_int(v)));
        }
    }
    let mut runtime = Object::new();
    for path in root.glob("/sys/bus/platform/devices/*/power/runtime_status") {
        let name = path
            .parent()
            .and_then(|p| p.parent())
            .map(glob_name)
            .unwrap_or_default();
        if [
            "11200000.",
            "11220000.",
            "11230000.",
            "11240000.",
            "13010000.",
            "14007000.",
            "14008000.",
            "18070000.",
        ]
        .iter()
        .any(|prefix| name.starts_with(prefix))
        {
            runtime.set(&name, read(&path, 64));
        }
    }
    let named = |pattern: &str, limit: usize| {
        let mut out = Object::new();
        for path in root.glob(pattern) {
            out.set(&glob_name(&path), read(&path, limit));
        }
        out
    };
    let mut result = Object::new()
        .with(
            "online",
            root.read_limited("/sys/devices/system/cpu/online", 64),
        )
        .with("cpuidle", idle)
        .with("scheduler", scheduler)
        .with("runtime_pm", runtime)
        .with(
            "frequency_khz",
            root.integer("/sys/devices/system/cpu/cpufreq/policy0/scaling_cur_freq"),
        )
        .with(
            "frequency_ticks",
            root.read_limited(
                "/sys/devices/system/cpu/cpufreq/policy0/stats/time_in_state",
                512,
            ),
        )
        .with(
            "idle_rejections",
            named("/sys/module/idle/parameters/*", 128),
        )
        .with(
            "clock_blockers",
            named("/sys/module/clocks/parameters/*blockers", 128),
        )
        .with(
            "counter_reset_rule",
            "decreasing per-core counters are unknown, not negative rates",
        );
    result.set("collector_cpu_seconds", process_time() - started);
    result
}

/// `time.process_time()`.
fn process_time() -> f64 {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: a valid timespec and a clock every Linux has.
    unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut now) };
    now.tv_sec as f64 + now.tv_nsec as f64 * 1e-9
}

// ---- sleep_diagnostics (the parts the power sample drives) ----------------------------------------

const DIAGNOSTICS: &str = "/data/system/platform/sleep-diagnostics.jsonl";
const DIAGNOSTICS_LIMIT: u64 = 2097152;
const DIAGNOSTICS_ROW: usize = 65536;
const SPM: &str = "/sys/devices/platform/10006000.power-controller/";
const USB_SUMMARY: [&str; 21] = [
    "stage",
    "error",
    "configured",
    "detached",
    "monitor_parked",
    "cable_events",
    "runtime_status",
    "runtime_held",
    "pm_suspends",
    "pm_restores",
    "irqs",
    "dma_errors",
    "dma_aborts",
    "reconnect_failures",
    "monitor_failures",
    "dma_tx_programs",
    "dma_irqs",
    "dma_unmask_rearms",
    "dma_unmask_lost",
    "dma_intr_before",
    "dma_intr_after",
];
// Kernel messages kept in a row: USB, gadget, network and the sleep path.
const KERNEL_TOKENS: [&str; 18] = [
    "Y2USB",
    "Y2NET",
    "Y2SPM",
    "Y2PM",
    "Y2INFRA",
    "Y2VPROC",
    "Y2DVFS",
    "Y2_DPM",
    "y2-usb",
    "y2-spm",
    "y2-pmic",
    "apt32f-wheel",
    "musb",
    "udc",
    "configfs",
    "gadget",
    "usb0",
    "PM: ",
];
const PROCESSES: [&str; 2] = ["umtprd", "dropbear"];
pub(crate) const FOLLOW_UP_S: f64 = 30.0;
const MIN_INTERVAL_S: f64 = 10.0;

/// `ipv4(ctx, interface)`: local IPv4 addresses of one interface from /proc/net/fib_trie: no
/// subprocess, no ioctl. A route field that is not hex is `ValueError` (the row is dropped).
fn ipv4(root: &Root, interface: &str) -> Result<Vec<String>, Dropped> {
    let routes = root
        .read_limited("/proc/net/route", 65536)
        .unwrap_or_default();
    let mut networks = Vec::new();
    for line in routes.lines().skip(1) {
        let fields = python_split(line);
        if fields.len() >= 8 && fields[0] == interface {
            let network = python_hex(fields[1]).ok_or(Dropped)?;
            let mask = python_hex(fields[7]).ok_or(Dropped)?;
            networks.push((network, mask));
        }
    }
    let mut found: Vec<String> = Vec::new();
    let mut previous: Option<String> = None;
    for line in root
        .read_limited("/proc/net/fib_trie", 262144)
        .unwrap_or_default()
        .lines()
    {
        let text = y2p_core::root::python_strip(line);
        if text.contains("/32 host LOCAL") {
            if let Some(address) = previous.as_ref().filter(|p| !p.is_empty()) {
                // Python's ints are unbounded; an octet beyond 64 bits matches no route here.
                let octets: Option<Vec<i128>> = address
                    .split('.')
                    .map(|x| x.parse::<u64>().ok().map(i128::from))
                    .collect();
                if let Some(o) = octets.filter(|o| o.len() == 4) {
                    let value = o[0] | o[1] << 8 | o[2] << 16 | o[3] << 24;
                    if networks
                        .iter()
                        .any(|(network, mask)| value & mask == *network)
                        && !found.contains(address)
                    {
                        found.push(address.clone());
                    }
                }
            }
        }
        previous = match trie_address(text) {
            Some(address) => Some(address),
            None if text.contains('/') => previous,
            None => None,
        };
    }
    found.sort();
    Ok(found)
}

/// `processes(ctx)`: whether umtprd and dropbear run (their `comm` under /proc).
fn processes(root: &Root) -> Object {
    let mut running: Vec<(&str, bool)> = PROCESSES.iter().map(|p| (*p, false)).collect();
    if let Ok(entries) = fs::read_dir(root.path("/proc")) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.is_empty() && name.chars().all(|c| c.is_numeric()) {
                if let Some(comm) = read(&entry.path().join("comm"), 64) {
                    if let Some(slot) = running.iter_mut().find(|(p, _)| *p == comm) {
                        slot.1 = true;
                    }
                }
            }
        }
    }
    running
        .into_iter()
        .fold(Object::new(), |o, (name, on)| o.with(name, on))
}

/// `usb_net(ctx)`: compact USB gadget and USB network state (about 1 KiB).
pub fn usb_net(root: &Root) -> Result<Object, Dropped> {
    let net = "/sys/class/net/usb0/";
    let mut statistics = Object::new();
    for name in [
        "rx_packets",
        "tx_packets",
        "rx_bytes",
        "tx_bytes",
        "rx_errors",
        "tx_errors",
        "rx_dropped",
        "tx_dropped",
    ] {
        statistics.set(name, root.integer(&format!("{net}statistics/{name}")));
    }
    let mut udc = Object::new();
    for path in root.glob("/sys/class/udc/*") {
        udc.set(&glob_name(&path), read(&path.join("state"), 64));
    }
    let mut controller = Object::new();
    for path in root.glob("/sys/bus/platform/drivers/y2-usb/*/status") {
        let found = key_values(read(&path, 8192).as_deref());
        let mut summary = Object::new();
        for key in USB_SUMMARY {
            summary.set(
                key,
                found.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()),
            );
        }
        controller.set(&parent_name(&path), summary);
    }
    let mut out_path = Object::new();
    for path in root.glob("/sys/bus/platform/drivers/y2-usb/*/rxdiag") {
        let first: Vec<Value> = read(&path, 4096)
            .unwrap_or_default()
            .lines()
            .take(1)
            .map(Value::from)
            .collect();
        out_path.set(&parent_name(&path), first);
    }
    let usb0 = Object::new()
        .with(
            "operstate",
            root.read_limited(&format!("{net}operstate"), 64),
        )
        .with("carrier", root.read_limited(&format!("{net}carrier"), 8))
        .with("flags", root.read_limited(&format!("{net}flags"), 32))
        .with(
            "ipv4",
            ipv4(root, "usb0")?
                .into_iter()
                .map(Value::from)
                .collect::<Vec<_>>(),
        )
        .with("statistics", statistics);
    Ok(Object::new()
        .with("udc", udc)
        .with(
            "gadget_udc",
            root.read_limited("/sys/kernel/config/usb_gadget/y2/UDC", 128),
        )
        .with("controller", controller)
        .with("out_path", out_path)
        .with("usb0", usb0)
        .with("transfer", json(root, "/run/y2/usb-transfer.json"))
        .with("processes", processes(root)))
}

/// `features(ctx, golden)`: the 13 sleep-parity switches and counters, the INFRA and SPM state.
fn features(root: &Root, golden: bool) -> Object {
    let by_parent = |pattern: &str| {
        let mut out = Object::new();
        for path in root.glob(pattern) {
            out.set(&parent_name(&path), read(&path, 4096));
        }
        out
    };
    let r = |name: &str, limit: usize| root.read_limited(name, limit);
    let mut value = Object::new()
        .with(
            "vproc_sleep",
            r("/sys/module/pwrap/parameters/vproc_sleep", 16),
        )
        .with(
            "vproc_sleep_state",
            r("/sys/module/pwrap/parameters/vproc_sleep_state", 4096),
        )
        .with(
            "pmic_sleep_state",
            r("/sys/module/pwrap/parameters/pmic_sleep_state", 4096),
        )
        .with("lp_sel", r("/sys/firmware/y2_pmic/lp_sel", 4096))
        .with(
            "phy_savecurrent",
            by_parent("/sys/bus/platform/drivers/y2-usb/*/phy_savecurrent"),
        )
        .with(
            "wheel_sleep",
            by_parent("/sys/bus/i2c/devices/*-0051/sleep"),
        )
        .with("infra", r("/sys/firmware/y2_infra/state", 4096))
        .with("spm", r(&format!("{SPM}state"), 8192))
        // 27: stock panel power-down and the idle VGP2 rail (ledger F20/F22).
        .with(
            "panel_stock_suspend",
            r("/sys/module/panel/parameters/stock_suspend", 16),
        )
        .with(
            "panel_power_down",
            r("/sys/module/panel/parameters/power_down", 16),
        )
        .with(
            "vgp2_idle_off",
            r("/sys/module/y2_cs43131/parameters/vgp2_idle_off", 16),
        )
        // 32: deep idle during playback and how often it was admitted.
        .with(
            "deep_idle_audio",
            r("/sys/module/clocks/parameters/deep_idle_audio", 16),
        )
        .with(
            "deep_audio_entries",
            r("/sys/module/clocks/parameters/deep_audio_entries", 32),
        );
    if golden {
        // Read-only always-on/INFRA registers; the entry column is the last
        // suspend-entry capture, so read it once Linux has returned.
        value.set("golden", r(&format!("{SPM}golden"), 16384)).set(
            "pmic_golden",
            r("/sys/module/pwrap/parameters/pmic_golden", 16384),
        );
    }
    value
}

/// `kernel_tail(ctx)`: the last relevant kernel messages, read non-blocking from /dev/kmsg.
fn kernel_tail(root: &Root) -> Option<Vec<String>> {
    const LINES: usize = 80;
    const WIDTH: usize = 240;
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(root.path("/dev/kmsg"))
        .ok()?;
    let mut kept: Vec<String> = Vec::new();
    let mut block = vec![0u8; 8192];
    for _ in 0..16384 {
        let n = match file.read(&mut block) {
            Ok(n) => n,
            Err(e) if e.raw_os_error() == Some(libc::EPIPE) => continue,
            Err(_) => break,
        };
        if n == 0 {
            break;
        }
        for record in String::from_utf8_lossy(&block[..n]).lines() {
            if record.starts_with(' ') {
                continue;
            }
            let (header, message) = record.split_once(';').unwrap_or((record, ""));
            let fields: Vec<&str> = header.split(',').collect();
            let head: String = message.chars().take(96).collect();
            if KERNEL_TOKENS.iter().any(|t| head.contains(t)) {
                let stamp = fields.get(2).copied().unwrap_or("?");
                kept.push(format!("{stamp} {message}").chars().take(WIDTH).collect());
                if kept.len() > LINES {
                    kept.drain(..kept.len() - LINES);
                }
            }
        }
    }
    Some(kept)
}

/// `sleep_diagnostics.Diagnostics`: rows on suspend stages and around USB state changes.
#[derive(Debug, Default)]
pub struct Diagnostics {
    pub failures: u64,
    usb_signature: Option<Vec<(String, Option<String>)>>,
    follow_up: Option<f64>,
    last_row: Option<f64>,
}

impl Diagnostics {
    pub fn record(
        &mut self,
        root: &Root,
        stage: &str,
        now: f64,
        golden: bool,
        usb_reason: Option<&str>,
    ) -> bool {
        match self.row(root, stage, now, golden, usb_reason) {
            Ok(written) => {
                if written {
                    self.last_row = Some(now);
                }
                written
            }
            Err(Dropped) => {
                self.failures += 1;
                false
            }
        }
    }

    fn row(
        &self,
        root: &Root,
        stage: &str,
        now: f64,
        golden: bool,
        usb_reason: Option<&str>,
    ) -> Result<bool, Dropped> {
        if !data_ready(root) {
            return Ok(false);
        }
        let build_id = match json(root, "/etc/y2linux/versions.json") {
            Value::Object(o) => o.value("build_id"),
            _ => Value::Null,
        };
        let mut tail = kernel_tail(root);
        let mut row = Object::new()
            .with("schema", 1i64)
            .with("stage", stage)
            .with("usb_reason", usb_reason)
            .with("wall_time", wall())
            .with("monotonic_s", now)
            .with("boottime_s", root.clock().boottime_ns() as f64 / 1e9)
            .with("boot_id", root.boot_id())
            .with("build_id", build_id)
            .with("features", features(root, golden))
            .with("usb_net", usb_net(root)?)
            .with(
                "kernel_pm",
                root.read_limited("/sys/firmware/y2_pm/state", 8192),
            )
            .with(
                "kernel_tail",
                tail.clone()
                    .map(|t| t.into_iter().map(Value::from).collect::<Vec<_>>()),
            );
        let encode = |row: &Object| {
            dumps(row, Separators::Compact)
                .map(|t| t + "\n")
                .map_err(|_| Dropped)
        };
        let mut encoded = encode(&row)?;
        while encoded.len() > DIAGNOSTICS_ROW && tail.as_ref().is_some_and(|t| !t.is_empty()) {
            let lines = tail.as_mut().expect("tail");
            let cut = (lines.len() / 4).max(1);
            lines.drain(..cut);
            row.set(
                "kernel_tail",
                lines.iter().cloned().map(Value::from).collect::<Vec<_>>(),
            );
            encoded = encode(&row)?;
        }
        if encoded.len() > DIAGNOSTICS_ROW {
            return Err(Dropped); // sleep_diagnostics_row_too_large
        }
        append_bounded(
            root,
            DIAGNOSTICS,
            encoded.as_bytes(),
            DIAGNOSTICS_LIMIT,
            true,
        )
        .map_err(|_| Dropped)?;
        Ok(true)
    }

    /// Called on the existing one-second power sample; cheap when nothing changed.
    pub fn observe_usb(&mut self, root: &Root, now: f64) -> bool {
        let mut signature: Vec<(String, Option<String>)> = root
            .glob("/sys/class/udc/*")
            .iter()
            .map(|p| (glob_name(p), read(&p.join("state"), 64)))
            .collect();
        signature.sort();
        let mut reason = None;
        if self.usb_signature.as_ref().is_some_and(|s| *s != signature) {
            reason = Some("udc_change");
            self.follow_up = Some(now + FOLLOW_UP_S);
        } else if self.follow_up.is_some_and(|f| now >= f) {
            reason = Some("udc_follow_up");
            self.follow_up = None;
        }
        self.usb_signature = Some(signature);
        let Some(reason) = reason else {
            return false;
        };
        if let Some(last) = self.last_row.filter(|last| now - last < MIN_INTERVAL_S) {
            // Rate-limited: a change still gets its follow-up, a follow-up is postponed.
            self.follow_up = Some(if reason == "udc_change" {
                now + FOLLOW_UP_S
            } else {
                last + MIN_INTERVAL_S
            });
            return false;
        }
        self.record(root, "usb", now, false, Some(reason))
    }
}

// ---- sleep.status, as the journal row holds it ----------------------------------------------------

const SLEEP_SCHEMA: &str = "org.y2linux.sleep/v1";
const SLEEP_STATES: [&str; 8] = [
    "requested",
    "preparing",
    "refused",
    "sleeping",
    "restoring",
    "waking",
    "restored",
    "restore_failed",
];

/// `dict(value)` of a truthy non-dict `product`: the pairs, or `Err` for Python's TypeError/ValueError.
fn dict_of(value: &Value) -> Result<Vec<(Value, Value)>, Dropped> {
    let Value::Array(items) = value else {
        return Err(Dropped); // dict(str/int/bool): TypeError or ValueError
    };
    let mut pairs = Vec::new();
    for item in items {
        let (key, value) = match item {
            Value::Array(pair) if pair.len() == 2 => (pair[0].clone(), pair[1].clone()),
            Value::String(s) if s.chars().count() == 2 => {
                let mut chars = s.chars();
                (
                    Value::from(chars.next().expect("two").to_string()),
                    Value::from(chars.next().expect("two").to_string()),
                )
            }
            Value::Object(o) if o.len() == 2 => {
                let mut keys = o.iter().map(|(k, _)| Value::from(k.clone()));
                (keys.next().expect("two"), keys.next().expect("two"))
            }
            _ => return Err(Dropped),
        };
        if !key.hashable() {
            return Err(Dropped);
        }
        pairs.push((key, value));
    }
    Ok(pairs)
}

/// `sleep.status(ctx)` on the receipt: durable evidence interpreted without promoting a reset into a
/// resume.
fn sleep_status(receipt: &Object, boot_id: Option<&str>) -> Result<Object, Dropped> {
    let mut result = match receipt.value("product") {
        Value::Object(o) => o,
        other if other.truthy() => {
            // A list of pairs becomes a dict whose keys need not be strings; the row would then fail
            // `receipt.get('product').get` below (AttributeError: the daemon ends) unless its 'state'
            // is unhashable (TypeError here: dropped).
            let pairs = dict_of(&other)?;
            let state = pairs
                .iter()
                .rev()
                .find(|(k, _)| k.is_str("state"))
                .map(|(_, v)| v);
            if state.is_some_and(|s| !s.hashable()) {
                return Err(Dropped);
            }
            crash("AttributeError: 'list' object has no attribute 'get'");
        }
        _ => Object::new(),
    };
    let state = result.value("state");
    if !state.hashable() {
        return Err(Dropped); // `in STATES` (a set): TypeError
    }
    if !SLEEP_STATES.iter().any(|s| state.is_str(s)) {
        result = Object::new()
            .with("state", "idle")
            .with("reason", Value::Null)
            .with("request_boot_id", Value::Null)
            .with("kernel_completed", false)
            .with("wake_reason", "unknown");
    }
    result.set("schema", SLEEP_SCHEMA);
    result.set("boot_id", boot_id);
    let request = result.value("request_boot_id");
    let same = match boot_id {
        Some(boot) if request.truthy() && !boot.is_empty() => Value::from(request.is_str(boot)),
        _ => Value::Null,
    };
    result.set("same_boot", same.clone());
    let state = result.value("state");
    let state = state.as_str().unwrap_or_default();
    if matches!(state, "restored" | "refused") && same == Value::Bool(false) {
        result
            .set("state", "idle")
            .set("reason", Value::Null)
            .set("request_boot_id", Value::Null)
            .set("same_boot", Value::Null)
            .set("kernel_completed", false)
            .set("wake_reason", "unknown");
    }
    if matches!(
        state,
        "requested" | "preparing" | "sleeping" | "restoring" | "waking"
    ) && same == Value::Bool(false)
    {
        result
            .set("state", "restore_failed")
            .set("reason", "boot_changed_before_restoration")
            .set("kernel_completed", false);
    }
    Ok(result)
}

// ---- the journal ---------------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct PowerJournal {
    next_snapshot: f64,
    next_event: f64,
    signature: Option<(String, Value, Value)>,
    pub failures: u64,
    pub diagnostics: Diagnostics,
}

impl PowerJournal {
    pub fn new() -> Self {
        Self::default()
    }

    /// `observe(state, battery, shutdown, event)`: a row on an event, a safety state change, every
    /// 300 s, or a state change at most once a minute.
    pub fn observe(
        &mut self,
        root: &Root,
        state: &str,
        battery: &Object,
        shutdown: Option<&Object>,
        event: Option<&str>,
    ) -> bool {
        self.observe_at(
            root,
            state,
            battery,
            shutdown,
            event,
            root.clock().monotonic(),
        )
    }

    /// `observe(..., now=now)`: at this CLOCK_MONOTONIC time (seconds).
    pub fn observe_at(
        &mut self,
        root: &Root,
        state: &str,
        battery: &Object,
        shutdown: Option<&Object>,
        event: Option<&str>,
        now: f64,
    ) -> bool {
        // POWER-FINAL-14: persist USB state changes (and one follow-up) so a
        // USB failure after a wake leaves evidence across a restart.
        self.diagnostics.observe_usb(root, now);
        let signature = (
            state.to_owned(),
            battery.value("status"),
            battery.value("charging"),
        );
        let changed = self.signature.as_ref().is_none_or(|s| {
            s.0 != signature.0 || !s.1.py_eq(&signature.1) || !s.2.py_eq(&signature.2)
        });
        let safety = state == "ShutdownPending" && changed;
        if !(event.is_some_and(|e| !e.is_empty())
            || safety
            || now >= self.next_snapshot
            || (changed && now >= self.next_event))
        {
            return false;
        }
        self.next_snapshot = now + PERIOD;
        self.next_event = now + 60.0;
        self.signature = Some(signature);
        let durable = event.is_some_and(|e| !e.is_empty()) || safety;
        match self.row(root, state, battery, shutdown, event, safety, now) {
            Ok(Some(row)) => match self.append(root, &row, durable) {
                Ok(()) => true,
                Err(Dropped) => {
                    // A journal failure must never delay or disable battery protection.
                    self.failures += 1;
                    false
                }
            },
            Ok(None) => false,
            Err(Dropped) => {
                self.failures += 1;
                false
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn row(
        &self,
        root: &Root,
        state: &str,
        battery: &Object,
        shutdown: Option<&Object>,
        event: Option<&str>,
        safety: bool,
        now: f64,
    ) -> Result<Option<Object>, Dropped> {
        // Do not create persistent state on an unverified/missing data mount.
        if !data_ready(root) {
            return Ok(None);
        }
        let receipt = match root.json("/data/system/platform/suspend-last.json") {
            Some(Value::Object(o)) => o,
            _ => Object::new(),
        };
        let mut usb = Object::new();
        for path in root.glob("/sys/class/udc/*") {
            usb.set(
                &glob_name(&path),
                read(&path.join("state"), y2p_core::root::READ_LIMIT),
            );
        }
        let usb_net = usb_net(root)?;
        let mut backlight = Object::new();
        for path in root.glob("/sys/class/backlight/*/bl_power") {
            backlight.set(&parent_name(&path), read(&path, y2p_core::root::READ_LIMIT));
        }
        let mut radios = Object::new();
        for path in root.glob("/sys/class/rfkill/*") {
            radios.set(
                &glob_name(&path),
                Object::new()
                    .with("type", read(&path.join("type"), y2p_core::root::READ_LIMIT))
                    .with(
                        "blocked",
                        read(&path.join("soft"), y2p_core::root::READ_LIMIT),
                    ),
            );
        }
        let boot_id = root.boot_id();
        let idle = idle_counters(root);
        let sleep = sleep_status(&receipt, boot_id.as_deref())?;
        let mut suspend = Object::new();
        for key in [
            "mode",
            "stage",
            "first_entry",
            "resume",
            "cycles",
            "suspended_seconds",
            "maintenance_wakes",
            "error",
        ] {
            suspend.set(key, receipt.value(key));
        }
        // POWER-FINAL-18: 17 lost why two requests were refused.
        suspend.set(
            "reason",
            match receipt.value("product") {
                Value::Object(p) => p.value("reason"),
                _ => Value::Null, // a falsy product (`or {}`); a truthy non-dict ended above
            },
        );
        let row = Object::new()
            .with("schema", 1i64)
            .with(
                "event",
                event.filter(|e| !e.is_empty()).unwrap_or(if safety {
                    "state_change"
                } else {
                    "snapshot"
                }),
            )
            .with("wall_time", wall())
            .with("monotonic_s", now)
            .with("boottime_s", root.clock().boottime_ns() as f64 / 1e9)
            .with("boot_id", boot_id)
            .with("build", json(root, "/etc/y2linux/versions.json"))
            .with("state", state)
            .with("battery", battery.clone())
            .with("shutdown", shutdown.cloned())
            .with("usb", usb)
            .with("usb_net", usb_net)
            .with(
                "charger",
                root.read("/sys/class/power_supply/BAT0/charging_state"),
            )
            .with("backlight", backlight)
            .with("radios", radios)
            .with("idle_policy", json(root, "/run/y2/cpu-idle-policy.json"))
            .with("idle_counters", idle)
            .with("sleep", sleep)
            // Preserve completed intervals when a later refusal replaces
            // suspend-last.json. This shares the existing event cadence.
            .with("suspend", suspend)
            .with(
                "kernel_pm",
                root.read_limited("/sys/firmware/y2_pm/state", 8192),
            );
        Ok(Some(row))
    }

    fn append(&self, root: &Root, row: &Object, durable: bool) -> Result<(), Dropped> {
        let encoded = dumps(row, Separators::Compact).map_err(|_| Dropped)? + "\n";
        if encoded.len() > ROW {
            return Err(Dropped); // power_journal_row_too_large
        }
        append_bounded(root, PATH, encoded.as_bytes(), LIMIT, durable).map_err(|_| Dropped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// POWER-FINAL-14: a UDC state change gives a durable row, then one follow-up 30 s later; rows
    /// closer than 10 s are postponed.
    #[test]
    fn usb_changes_leave_rows_and_one_follow_up() {
        let dir = std::env::temp_dir().join(format!("y2-usb-rows-{}", std::process::id()));
        let put = |name: &str, text: &str| {
            let path = dir.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        };
        put(
            "proc/self/mountinfo",
            "30 22 179:9 / /data rw - ext4 /dev/mmcblk0p9 rw\n",
        );
        put("run/y2-data-device", "/dev/mmcblk0p9\n");
        put("data/.y2data-schema", "1\n");
        put("sys/class/udc/11200000.usb/state", "configured\n");
        let root = Root::new(&dir);
        let rows = || {
            fs::read_to_string(dir.join("data/system/platform/sleep-diagnostics.jsonl"))
                .unwrap_or_default()
                .lines()
                .map(
                    |l| match serde_json::from_slice::<Value>(l.as_bytes()).unwrap() {
                        Value::Object(o) => o.value("usb_reason").py_str(),
                        _ => unreachable!(),
                    },
                )
                .collect::<Vec<_>>()
        };
        let mut diagnostics = Diagnostics::default();
        assert!(!diagnostics.observe_usb(&root, 100.0));
        put("sys/class/udc/11200000.usb/state", "not attached\n");
        assert!(diagnostics.observe_usb(&root, 101.0));
        put("sys/class/udc/11200000.usb/state", "configured\n");
        assert!(!diagnostics.observe_usb(&root, 105.0), "rate-limited");
        assert!(!diagnostics.observe_usb(&root, 120.0));
        assert!(
            diagnostics.observe_usb(&root, 135.0),
            "the follow-up of the postponed change"
        );
        assert!(!diagnostics.observe_usb(&root, 200.0));
        assert_eq!(rows(), ["udc_change", "udc_follow_up"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sleep_status_follows_sleep_py() {
        let parse = |t: &str| match serde_json::from_slice::<Value>(t.as_bytes()).unwrap() {
            Value::Object(o) => o,
            _ => unreachable!(),
        };
        let text = |o: &Object| dumps(o, Separators::Spaced).unwrap();
        assert_eq!(
            text(&sleep_status(&parse("{}"), Some("b")).unwrap()),
            "{\"state\": \"idle\", \"reason\": null, \"request_boot_id\": null, \"kernel_completed\": false, \
             \"wake_reason\": \"unknown\", \"schema\": \"org.y2linux.sleep/v1\", \"boot_id\": \"b\", \"same_boot\": null}"
        );
        let other = parse(r#"{"product": {"state": "sleeping", "request_boot_id": "a"}}"#);
        let status = sleep_status(&other, Some("b")).unwrap();
        assert!(status.value("state").is_str("restore_failed"));
        assert!(sleep_status(&parse(r#"{"product": {"state": []}}"#), Some("b")).is_err());
        assert!(sleep_status(&parse(r#"{"product": 5}"#), Some("b")).is_err());
        assert!(sleep_status(&parse(r#"{"product": [1]}"#), Some("b")).is_err());
    }
}

#[cfg(test)]
#[path = "journal_tests.rs"]
mod journal_tests;
