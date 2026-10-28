//! `observe.snapshot`: read-only platform snapshots. Null means unobserved, never zero or success.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    bluetooth, system,
    util::{
        self, counters, eq, findall_map, fresh_seconds, get_eq, int, is_int, key_values, name,
        number, parent_name, read, read_limited, reads_by_name, realpath, split_ws, splitlines,
        text,
    },
};
use crate::{regex, reply::Failure};
use serde_json::{json, Map, Value};
use std::path::Path;
use y2p_core::{
    clock, command,
    json::{dumps_sorted, Separators},
    observation_support::sha2::{Digest, Sha256},
    py::truthy,
    Root,
};

pub type Snapshot = Map<String, Value>;

/// `observe.mountinfo`: every field, as the snapshot shows it.
pub fn mountinfo(text: Option<&str>) -> Vec<Map<String, Value>> {
    let mut result = Vec::new();
    for line in splitlines(text.unwrap_or_default()) {
        let (left, right) = line.split_once(" - ").unwrap_or((line, ""));
        let (a, b) = (split_ws(left), split_ws(right));
        if !line.contains(" - ")
            || a.len() < 6
            || b.len() < 3
            || !a[0].chars().all(|c| c.is_ascii_digit())
        {
            continue;
        }
        let Some(mount_id) = int(a[0]) else { continue };
        let list = |s: &str| Value::from(s.split(',').collect::<Vec<_>>());
        let mut mount = Map::new();
        mount.insert("mount_id".into(), json!(mount_id));
        mount.insert("device_id".into(), json!(a[2]));
        mount.insert("root".into(), json!(unescape(a[3])));
        mount.insert("path".into(), json!(unescape(a[4])));
        mount.insert("options".into(), list(a[5]));
        mount.insert("filesystem".into(), json!(b[0]));
        mount.insert("source".into(), json!(unescape(b[1])));
        mount.insert("super_options".into(), list(b[2]));
        result.push(mount);
    }
    result
}

fn unescape(value: &str) -> String {
    regex!(r"\\(040|011|012|134)")
        .replace_all(
            value,
            |c: &y2p_core::observation_support::regex::Captures| {
                char::from(u8::from_str_radix(&c[1], 8).expect("octal")).to_string()
            },
        )
        .into_owned()
}

pub fn mounts_at<'a>(mounts: &'a [Map<String, Value>], path: &str) -> Vec<&'a Map<String, Value>> {
    mounts
        .iter()
        .filter(|m| m.get("path").and_then(Value::as_str) == Some(path))
        .collect()
}

pub fn field<'a>(mount: &'a Map<String, Value>, key: &str) -> &'a str {
    mount.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn has_option(mount: &Map<String, Value>, key: &str, option: &str) -> bool {
    mount
        .get(key)
        .and_then(Value::as_array)
        .is_some_and(|list| list.iter().any(|o| o == option))
}

/// `observe.device_instance`: one gendisk lifetime (dev_t plus diskseq), or None.
pub fn device_instance(root: &Root, device_id: &str) -> Option<String> {
    let path = std::fs::canonicalize(root.path(&format!("/sys/dev/block/{device_id}"))).ok()?;
    let disk = if path.join("partition").is_file() {
        path.parent()?.to_path_buf()
    } else {
        path.clone()
    };
    let dev = read(&path.join("dev"));
    let seq = read(&disk.join("diskseq"));
    let dev = dev.filter(|d| d == device_id && regex!(r"\A[0-9]+:[0-9]+\z").is_match(d))?;
    let seq = seq.filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))?;
    (int(&seq)? > 0).then(|| format!("block:{dev}:diskseq:{seq}"))
}

// ---- cpu ---------------------------------------------------------------------------------------

fn timer_runtime(root: &Root) -> Result<Map<String, Value>, Failure> {
    let text = if root.is_real() {
        let answer =
            command::run_with(root, &["/bin/cat", "/proc/timer_list"], 0.2, 262_144, false);
        if answer.ok {
            answer.output
        } else {
            None
        }
    } else {
        root.read_limited("/proc/timer_list", 262_144)
    };
    let text = text.unwrap_or_default();
    let mut cpus: Vec<Map<String, Value>> = Vec::new();
    for block in regex!(r"(?m)^cpu:\s*").split(&text).skip(1) {
        let lines = splitlines(block);
        let Some(first) = lines.first() else {
            return Err(Failure::Fallback); // IndexError
        };
        if first.is_empty() || !first.chars().all(char::is_numeric) {
            continue;
        }
        if !first.is_ascii() {
            return Err(Failure::Fallback); // a non-ASCII digit int() would read
        }
        let mut core = Map::new();
        core.insert("cpu".into(), json!(int(first).ok_or(Failure::Fallback)?));
        let fields = findall_map(
            regex!(r"(?m)^\s*\.(hres_active|nohz|tick_stopped|nr_hangs)\s*:\s*(\d+)\s*$"),
            block,
        );
        for (key, value) in fields {
            core.insert(key, json!(int(&value).ok_or(Failure::Fallback)?));
        }
        cpus.push(core);
    }
    let observed = |key: &str| -> Value {
        let values: Vec<Option<i64>> = cpus
            .iter()
            .map(|c| c.get(key).and_then(Value::as_i64))
            .collect();
        if values.is_empty() || values.iter().any(Option::is_none) {
            return Value::Null;
        }
        json!(values
            .iter()
            .flatten()
            .all(|v| if key == "nohz" { *v > 0 } else { *v == 1 }))
    };
    let mut result = Map::new();
    result.insert("highres_active".into(), observed("hres_active"));
    result.insert("no_hz_active".into(), observed("nohz"));
    result.insert("source".into(), json!("/proc/timer_list"));
    result.insert(
        "reason".into(),
        if cpus.is_empty() {
            json!("runtime_timer_state_unavailable")
        } else {
            Value::Null
        },
    );
    result.insert(
        "per_cpu".into(),
        Value::Array(cpus.into_iter().map(Value::Object).collect()),
    );
    Ok(result)
}

/// `int(text, 0)`: a Python integer literal (prefix, underscores), or None.
fn int_base0(text: &str) -> Option<i128> {
    let text = y2p_core::root::python_strip(text);
    let (negative, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let lower = body.to_ascii_lowercase();
    let (radix, digits) = if let Some(rest) = lower.strip_prefix("0x") {
        (16, rest.strip_prefix('_').unwrap_or(rest))
    } else if let Some(rest) = lower.strip_prefix("0o") {
        (8, rest.strip_prefix('_').unwrap_or(rest))
    } else if let Some(rest) = lower.strip_prefix("0b") {
        (2, rest.strip_prefix('_').unwrap_or(rest))
    } else {
        // A decimal literal: no leading zeros unless it is all zeros.
        if lower.starts_with('0') && lower.chars().any(|c| c != '0' && c != '_') {
            return None;
        }
        (10, lower.as_str())
    };
    if digits.is_empty()
        || digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
        || !digits
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    let value = i128::from_str_radix(&digits.replace('_', ""), radix).ok()?;
    Some(if negative { -value } else { value })
}

const PERIPHERAL: [(u32, &str); 22] = [
    (0, "NFI"),
    (2, "PWM1"),
    (3, "PWM2"),
    (4, "PWM3"),
    (5, "PWM4"),
    (6, "PWM5"),
    (7, "PWM6"),
    (8, "PWM7"),
    (9, "PWM"),
    (10, "USB0"),
    (11, "APDMA"),
    (12, "MSDC0"),
    (13, "MSDC1"),
    (14, "MSDC2"),
    (15, "NLI"),
    (17, "UART1"),
    (18, "UART2"),
    (19, "UART3"),
    (20, "BTIF"),
    (21, "I2C0"),
    (22, "I2C1"),
    (23, "I2C2"),
];
const DISPLAY: [&str; 19] = [
    "SMI_COMMON",
    "SMI_LARB0",
    "CMDQ",
    "MUTEX",
    "COLOR",
    "BLS",
    "DISP_WDMA",
    "DISP_RDMA",
    "OVL",
    "MDP_TDSHP",
    "MDP_WROT",
    "MDP_WDMA",
    "MDP_RSZ1",
    "MDP_RSZ0",
    "MDP_RDMA",
    "BLS_26M",
    "CAM_MDP",
    "FAKE_ENG",
    "MUTEX_32K",
];

fn runtime_status(root: &Root, node: &str) -> Value {
    text(root.read(&format!(
        "/sys/bus/platform/devices/{node}/power/runtime_status"
    )))
}

/// Counters come from cpuidle's returned state; absent is never a PASS.
fn idle_completion(root: &Root, states: &[Map<String, Value>]) -> Map<String, Value> {
    let mut result = Map::new();
    for (label, state) in [("C1", "WFI"), ("C2", "SLIDLE"), ("C3", "DORMANT")] {
        let rows: Vec<&Map<String, Value>> = states
            .iter()
            .filter(|s| s.get("name").and_then(Value::as_str) == Some(state))
            .filter(|s| label == "C1" || s.get("cpu").and_then(Value::as_str) == Some("cpu0"))
            .collect();
        let total = |field: &str| -> Value {
            let values: Vec<Option<i64>> = rows
                .iter()
                .map(|r| r.get(field).and_then(Value::as_i64))
                .collect();
            if values.is_empty() || values.iter().any(Option::is_none) {
                Value::Null
            } else {
                json!(values
                    .iter()
                    .flatten()
                    .map(|v| i128::from(*v))
                    .sum::<i128>() as i64)
            }
        };
        let mut item = Map::new();
        item.insert("registered".into(), json!(!rows.is_empty()));
        item.insert(
            "enabled".into(),
            if rows.is_empty() {
                Value::Null
            } else {
                json!(rows.iter().all(|r| r.get("disabled") == Some(&json!(0))))
            },
        );
        item.insert("entries".into(), total("usage"));
        item.insert("residency_us".into(), total("time_us"));
        item.insert("failures".into(), total("rejected"));
        item.insert(
            "per_cpu".into(),
            Value::Array(rows.into_iter().cloned().map(Value::Object).collect()),
        );
        result.insert(label.into(), Value::Object(item));
    }
    let mut c2 = Map::new();
    c2.insert(
        "attribution".into(),
        reads_by_name(root, "/sys/module/idle/parameters/slow_*", false),
    );
    c2.insert(
        "last_blocker".into(),
        text(root.read("/sys/module/clocks/parameters/slow_blocker_names")),
    );
    let mask = root.integer("/sys/module/clocks/parameters/slow_blockers");
    let mut owners = Vec::new();
    for (bit, clock, owner, node) in [
        (
            11,
            "APDMA",
            "connectivity / I2C DMA",
            Some("18070000.connectivity"),
        ),
        (12, "MSDC0", "eMMC", Some("11230000.mmc")),
        (13, "MSDC1", "SD", Some("11240000.mmc")),
        (14, "MSDC2", "unused controller", None),
        (20, "BTIF", "connectivity", Some("18070000.connectivity")),
        (21, "I2C0", "wheel", Some("11007000.i2c")),
        (22, "I2C1", "DAC", Some("11008000.i2c")),
        (23, "I2C2", "unused controller", None),
    ] {
        if mask.is_some_and(|m| m & (1 << bit) != 0) {
            owners.push(json!({"bit": bit, "clock": clock, "owner": owner,
                               "runtime_status": node.map_or(Value::Null, |n| runtime_status(root, n))}));
        }
    }
    c2.insert(
        "blocker_owners".into(),
        if mask.is_some() {
            Value::Array(owners)
        } else {
            Value::Null
        },
    );
    c2.insert(
        "shared_dma_owners".into(),
        Value::Array(
            ["18070000.connectivity", "11007000.i2c", "11008000.i2c"]
                .iter()
                .map(|n| json!({"name": n, "runtime_status": runtime_status(root, n)}))
                .collect(),
        ),
    );
    let preflight_text =
        root.read("/sys/devices/platform/10006000.power-controller/dormant_preflight");
    let context = root.read("/sys/devices/platform/10006000.power-controller/state");
    let mut c3 = Map::new();
    c3.insert("preflight".into(), text(preflight_text.clone()));
    c3.insert(
        "boot_policy".into(),
        Value::Object(root.json_object("/run/y2/cpu-idle-policy.json")),
    );
    c3.insert(
        "unused_clock_handoff".into(),
        text(root.read("/sys/module/clocks/parameters/unused_handoff")),
    );
    c3.insert("context".into(), text(context.clone()));
    c3.insert(
        "uart_clock_handoff".into(),
        text(root.read("/sys/module/clocks/parameters/uart_sleep_handoff")),
    );
    let uart = findall_map(regex!(r"(\w+)=([^\s]+)"), context.as_deref().unwrap_or(""));
    c3.insert(
        "uart_sleep".into(),
        Value::Object(
            uart.into_iter()
                .filter(|(k, _)| {
                    [
                        "uart_sleep_",
                        "uart_power_",
                        "uart_r13_",
                        "uart_request_",
                        "uart_ack_",
                    ]
                    .iter()
                    .any(|p| k.starts_with(p))
                })
                .map(|(k, v)| (k, json!(v)))
                .collect(),
        ),
    );
    c3.insert(
        "entry_budget".into(),
        json!(root.integer("/sys/module/spm/parameters/dormant_budget")),
    );
    c3.insert(
        "display_idle".into(),
        reads_by_name(root, "/sys/module/mm_clocks/parameters/*", false),
    );
    let preflight = findall_map(
        regex!(r"(\w+)=([^\s]+)"),
        preflight_text.as_deref().unwrap_or(""),
    );
    c3.insert(
        "prerequisites".into(),
        Value::Object(
            preflight
                .iter()
                .filter_map(|(k, v)| {
                    k.strip_prefix("prerequisite_")
                        .map(|n| (n.to_owned(), json!(v == "1")))
                })
                .collect(),
        ),
    );
    c3.insert(
        "unmet".into(),
        match preflight.get("unmet").filter(|u| !u.is_empty()) {
            Some(unmet) => json!(unmet.trim_matches(',').split(',').collect::<Vec<_>>()),
            None => json!([]),
        },
    );
    let mut blockers = Vec::new();
    let infra: &[(u32, &str)] = &[(5, "AFE"), (7, "L2C_SRAM"), (13, "TRNG"), (15, "CPUM")];
    let disp1: &[(u32, &str)] = &[
        (0, "DSI_ENGINE"),
        (1, "DSI_DIGITAL"),
        (2, "DPI_DIGITAL"),
        (3, "DPI_ENGINE"),
    ];
    let display: Vec<(u32, &str)> = DISPLAY
        .iter()
        .enumerate()
        .map(|(i, n)| (i as u32, *n))
        .collect();
    for (group, names, owner) in [
        ("peri_blockers", &PERIPHERAL[..], "peripheral controller"),
        ("infra_blockers", infra, "infrastructure clock owner"),
        ("disp0", &display[..], "DRM / inherited engine"),
        ("disp1", disp1, "DRM / inherited engine"),
    ] {
        let Some(mask) = preflight.get(group).and_then(|v| int_base0(v)) else {
            continue;
        };
        for bit in 0..32u32 {
            if mask & (1 << bit) == 0 {
                continue;
            }
            let clock = names
                .iter()
                .find(|(b, _)| *b == bit)
                .map_or("unknown", |(_, n)| n);
            let node = match clock {
                "USB0" => Some("11200000.usb"),
                "APDMA" | "BTIF" => Some("18070000.connectivity"),
                "MSDC0" => Some("11230000.mmc"),
                "MSDC1" => Some("11240000.mmc"),
                "I2C0" => Some("11007000.i2c"),
                "I2C1" => Some("11008000.i2c"),
                _ => None,
            };
            blockers.push(json!({"group": group, "bit": bit, "clock": clock, "owner": owner,
                                 "runtime_status": node.map_or(Value::Null, |n| runtime_status(root, n))}));
        }
    }
    c3.insert(
        "blocker_owners".into(),
        if preflight_text.is_some() {
            Value::Array(blockers)
        } else {
            Value::Null
        },
    );
    for (label, extra) in [("C2", c2), ("C3", c3)] {
        if let Some(Value::Object(item)) = result.get_mut(label) {
            item.extend(extra);
        }
    }
    result
}

/// `observe.cpu`.
pub fn cpu(root: &Root) -> Result<Map<String, Value>, Failure> {
    let stat = root.read("/proc/stat");
    let mut ticks = Map::new();
    for line in splitlines(stat.as_deref().unwrap_or_default()) {
        let fields = split_ws(line);
        if fields
            .first()
            .is_some_and(|f| regex!(r"\Acpu[0-9]*\z").is_match(f))
        {
            let values: Vec<Value> = fields
                .iter()
                .skip(1)
                .take(8)
                .map(|v| number(Some(v)))
                .collect();
            ticks.insert(fields[0].to_owned(), Value::Array(values));
        }
    }
    let ceiling = root.integer("/sys/module/cpu_dvfs/parameters/qualification_max_khz");
    let mut policies = Vec::new();
    for path in root.glob("/sys/devices/system/cpu/cpufreq/policy*") {
        let mut fields = Map::new();
        for field in [
            "affected_cpus",
            "scaling_governor",
            "scaling_cur_freq",
            "scaling_min_freq",
            "scaling_max_freq",
            "scaling_available_frequencies",
            "cpuinfo_min_freq",
            "cpuinfo_max_freq",
            "cpuinfo_cur_freq",
            "scaling_driver",
        ] {
            fields.insert(field.into(), text(read(&path.join(field))));
        }
        let maximum = fields
            .get("scaling_max_freq")
            .and_then(Value::as_str)
            .and_then(int);
        let admitted = ceiling.zip(maximum).map(|(c, m)| c.min(m));
        let available = fields
            .get("scaling_available_frequencies")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let opp: Vec<Value> = split_ws(&available)
            .into_iter()
            .filter(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()))
            .filter_map(|v| int(v))
            .filter(|v| admitted.is_some_and(|a| *v <= a))
            .map(Value::from)
            .collect();
        fields.insert("admitted_opp_khz".into(), Value::Array(opp));
        fields.insert("name".into(), json!(name(&path)));
        let states = counters(read(&path.join("stats/time_in_state")).as_deref());
        fields.insert(
            "time_in_state".into(),
            if states.is_empty() {
                Value::Null
            } else {
                Value::Object(states)
            },
        );
        fields.insert("time_in_state_unit".into(), json!("USER_HZ_ticks"));
        fields.insert("frequency_unit".into(), json!("kHz"));
        policies.push(Value::Object(fields));
    }
    let mut idle = Vec::new();
    for path in root.glob("/sys/devices/system/cpu/cpu[0-9]*/cpuidle/state*") {
        let cpu_name = path.parent().map(parent_name).unwrap_or_default();
        let value = |f: &str| number(read(&path.join(f)).as_deref());
        let mut state = Map::new();
        state.insert("cpu".into(), json!(cpu_name));
        state.insert("state".into(), json!(name(&path)));
        state.insert("name".into(), text(read(&path.join("name"))));
        state.insert("time_us".into(), value("time"));
        state.insert("usage".into(), value("usage"));
        state.insert("exit_latency_us".into(), value("latency"));
        state.insert("target_residency_us".into(), value("residency"));
        state.insert("rejected".into(), value("rejected"));
        state.insert("disabled".into(), value("disable"));
        idle.push(state);
    }
    let totals = counters(stat.as_deref());
    // This sysfs read is a suspend handshake: it blocks while a wakeup source is active, even with
    // O_NONBLOCK. Keep it outside the observer process and use the existing bounded runner to
    // kill/reap an interruptible stalled read.
    let wakeup_path = root.path("/sys/power/wakeup_count");
    let wakeup = command::run_with(
        root,
        &["/bin/cat", &wakeup_path.to_string_lossy()],
        0.1,
        64,
        false,
    );
    let wakeup_count = if wakeup.ok {
        wakeup.output.as_deref().and_then(int).filter(|v| *v >= 0)
    } else {
        None
    };
    let completion = idle_completion(root, &idle);
    let mut result = Map::new();
    result.insert(
        "online".into(),
        text(root.read("/sys/devices/system/cpu/online")),
    );
    result.insert("load_average".into(), text(root.read("/proc/loadavg")));
    result.insert(
        "ticks".into(),
        if ticks.is_empty() {
            Value::Null
        } else {
            Value::Object(ticks)
        },
    );
    result.insert("ticks_unit".into(), json!("USER_HZ_ticks"));
    result.insert("utilization_percent".into(), Value::Null);
    result.insert("policies".into(), Value::Array(policies));
    result.insert(
        "idle".into(),
        Value::Array(idle.into_iter().map(Value::Object).collect()),
    );
    result.insert("idle_completion".into(), Value::Object(completion));
    let read_text = |p: &str| text(root.read(p));
    result.insert(
        "voltage_uv".into(),
        json!(root.integer("/sys/module/pwrap/parameters/cpu_voltage_uv")),
    );
    result.insert(
        "stock_bin".into(),
        read_text("/sys/module/cpu_dvfs/parameters/bin_supported"),
    );
    result.insert("dvfs_ceiling_khz".into(), json!(ceiling));
    result.insert(
        "dvfs_fault".into(),
        read_text("/sys/module/cpu_dvfs/parameters/voltage_fault"),
    );
    result.insert(
        "spm".into(),
        read_text("/sys/devices/platform/10006000.power-controller/state"),
    );
    result.insert(
        "dormant_preflight".into(),
        read_text("/sys/devices/platform/10006000.power-controller/dormant_preflight"),
    );
    result.insert(
        "cirq".into(),
        read_text("/sys/devices/platform/10204000.interrupt-latch/state"),
    );
    result.insert("suspend_stage".into(), read_text("/run/y2/suspend-stage"));
    let mut persistent = Map::new();
    persistent.insert("current".into(), read_text("/sys/firmware/y2_pm/state"));
    persistent.insert(
        "previous_boot".into(),
        read_text("/sys/firmware/y2_pm/previous"),
    );
    persistent.insert(
        "durable".into(),
        root.json("/data/system/platform/suspend-last.json")
            .unwrap_or(Value::Null),
    );
    let mut usb = Map::new();
    for path in root.glob("/sys/bus/platform/drivers/y2-usb/*/status") {
        usb.insert(parent_name(&path), text(read_limited(&path, 8192)));
    }
    persistent.insert("usb_restore".into(), Value::Object(usb));
    persistent.insert(
        "radio_restore".into(),
        read_text("/sys/devices/platform/18070000.connectivity/status"),
    );
    // Fix02: awake SRAM proof, retention and callback trail.
    persistent.insert(
        "retention".into(),
        read_text("/sys/firmware/y2_pm/retention"),
    );
    persistent.insert("selftest".into(), read_text("/sys/firmware/y2_pm/selftest"));
    persistent.insert(
        "backstop".into(),
        read_text("/sys/firmware/y2_pm/backstop_s"),
    );
    persistent.insert(
        "device_callbacks".into(),
        text(root.read_limited("/sys/firmware/y2_pm/devices", 16384)),
    );
    persistent.insert(
        "device_callbacks_previous_boot".into(),
        text(root.read_limited("/sys/firmware/y2_pm/devices_previous", 16384)),
    );
    // Fix03: decoded RGU cause of the reset that started this boot.
    persistent.insert(
        "reset_status".into(),
        read_text("/sys/firmware/y2_pm/reset_status"),
    );
    result.insert("suspend_persistent".into(), Value::Object(persistent));
    result.insert(
        "dvfs_diagnostics".into(),
        reads_by_name(root, "/sys/module/cpu_dvfs/parameters/*", false),
    );
    result.insert(
        "voltage_ownership".into(),
        read_text("/sys/module/pwrap/parameters/cpu_voltage_state"),
    );
    result.insert(
        "slidle_clock_owners".into(),
        read_text("/sys/module/clocks/parameters/slow_blocker_names"),
    );
    result.insert(
        "pwrap_readiness".into(),
        read_text("/sys/module/pwrap/parameters/pwrap_readiness"),
    );
    result.insert(
        "slidle_attribution".into(),
        reads_by_name(root, "/sys/module/clocks/parameters/slow_*", false),
    );
    result.insert(
        "mmc_runtime_pm".into(),
        reads_by_name(root, "/sys/bus/platform/devices/*.mmc/y2_runtime_pm", true),
    );
    result.insert(
        "system_idle".into(),
        reads_by_name(root, "/sys/module/system_idle/parameters/*", false),
    );
    result.insert(
        "radio_boot_retries".into(),
        json!(root.integer("/sys/devices/platform/18070000.connectivity/boot_retries")),
    );
    result.insert(
        "clock_blockers".into(),
        reads_by_name(root, "/sys/module/clocks/parameters/*blockers", false),
    );
    result.insert(
        "idle_diagnostics".into(),
        reads_by_name(root, "/sys/module/idle/parameters/*", false),
    );
    result.insert(
        "recovery_options".into(),
        reads_by_name(root, "/sys/module/cpu_options/parameters/*", false),
    );
    let mut picked = Map::new();
    for key in [
        "ctxt",
        "intr",
        "softirq",
        "processes",
        "procs_running",
        "procs_blocked",
    ] {
        picked.insert(key.into(), totals.get(key).cloned().unwrap_or(Value::Null));
    }
    result.insert("counters".into(), Value::Object(picked));
    result.insert("wakeup_count".into(), json!(wakeup_count));
    result.insert(
        "wakeup_count_reason".into(),
        match wakeup_count {
            Some(_) => Value::Null,
            None => json!(wakeup
                .reason
                .filter(|r| !r.is_empty())
                .unwrap_or_else(|| "counter_unavailable".into())),
        },
    );
    result.insert("throttling_reason".into(), Value::Null);
    result.insert(
        "workload_qos".into(),
        json!({"implemented": root.path("/dev/y2-workload").exists(), "lease_max_ms": 30000,
               "interactive_max_ms": 500,
               "leases": text(root.read("/sys/module/workload/parameters/leases"))}),
    );
    let mut timer = timer_runtime(root)?;
    timer.insert(
        "admission".into(),
        reads_by_name(root, "/sys/module/local_timer/parameters/*", false),
    );
    timer.insert(
        "clocksource".into(),
        read_text("/sys/devices/system/clocksource/clocksource0/current_clocksource"),
    );
    timer.insert(
        "broadcast_clockevent".into(),
        read_text("/sys/devices/system/clockevents/broadcast/current_device"),
    );
    timer.insert(
        "clockevents".into(),
        reads_by_name(
            root,
            "/sys/devices/system/clockevents/clockevent*/current_device",
            true,
        ),
    );
    result.insert("timer".into(), Value::Object(timer));
    Ok(result)
}

/// `observe.utilization(before, after)`.
pub fn utilization(before: Option<&Value>, after: Option<&Value>) -> Value {
    let mut result = Map::new();
    let empty = Map::new();
    let before = before.and_then(Value::as_object).unwrap_or(&empty);
    for (core, new) in after.and_then(Value::as_object).unwrap_or(&empty) {
        let (Some(old), Some(new)) = (before.get(core).and_then(Value::as_array), new.as_array())
        else {
            continue;
        };
        if old.len() != new.len() || new.len() < 5 {
            continue;
        }
        let (Some(old), Some(new)): (Option<Vec<i64>>, Option<Vec<i64>>) = (
            old.iter().map(Value::as_i64).collect(),
            new.iter().map(Value::as_i64).collect(),
        ) else {
            continue;
        };
        let diff: Vec<i128> = old
            .iter()
            .zip(&new)
            .map(|(a, b)| i128::from(*b) - i128::from(*a))
            .collect();
        let total: i128 = diff.iter().sum();
        if diff.iter().min().is_some_and(|m| *m >= 0) && total > 0 {
            let busy = 100.0 * (total - diff[3] - diff[4]) as f64 / total as f64;
            result.insert(core.clone(), json!(round3(busy)));
        }
    }
    if result.is_empty() {
        Value::Null
    } else {
        Value::Object(result)
    }
}

/// Python `round(x, 3)` (correctly rounded through the shortest decimal repr).
pub fn round3(x: f64) -> f64 {
    let text = format!("{x:.3}");
    let candidate: f64 = text.parse().unwrap_or(x);
    // format! rounds the exact binary value half-to-even, as CPython's round() does.
    candidate
}

// ---- memory, thermal, power --------------------------------------------------------------------

pub fn memory(root: &Root, pids: &[i64], pss: bool) -> Result<Map<String, Value>, Failure> {
    let mut values = Map::new();
    for (k, v) in key_values(root.read("/proc/meminfo").as_deref(), ":") {
        if !v.is_empty() {
            values.insert(k, number(split_ws(&v).first().copied()));
        }
    }
    let mut processes = Vec::new();
    for &pid in pids {
        if !(1..=4_194_304).contains(&pid) {
            return Err(Failure::value("invalid pid"));
        }
        let status = key_values(root.read(&format!("/proc/{pid}/status")).as_deref(), ":");
        let stat = root.read(&format!("/proc/{pid}/stat")).unwrap_or_default();
        let after = stat.rsplit_once(") ").map_or(stat.as_str(), |(_, r)| r);
        let fields = split_ws(after);
        let smaps = if pss {
            key_values(
                root.read(&format!("/proc/{pid}/smaps_rollup")).as_deref(),
                ":",
            )
        } else {
            Default::default()
        };
        let first_word = |map: &std::collections::BTreeMap<String, String>, key: &str| {
            number(
                map.get(key)
                    .map(|v| v.split(' ').next().unwrap_or_default()),
            )
            .clone()
        };
        let lookup = |key: &str| number(status.get(key).map(String::as_str));
        processes.push(json!({
            "pid": pid,
            "name": status.get("Name"),
            "start_ticks": if fields.len() > 19 { number(Some(fields[19])) } else { Value::Null },
            "rss_kib": first_word(&status, "VmRSS"),
            "pss_kib": first_word(&smaps, "Pss"),
            "threads": lookup("Threads"),
            "voluntary_context_switches": lookup("voluntary_ctxt_switches"),
            "involuntary_context_switches": lookup("nonvoluntary_ctxt_switches"),
        }));
    }
    let vmstat = counters(root.read("/proc/vmstat").as_deref());
    let mut result = Map::new();
    result.insert("unit".into(), json!("KiB"));
    result.insert(
        "meminfo".into(),
        if values.is_empty() {
            Value::Null
        } else {
            Value::Object(values)
        },
    );
    result.insert(
        "vmstat".into(),
        if vmstat.is_empty() {
            Value::Null
        } else {
            Value::Object(vmstat)
        },
    );
    result.insert("pressure".into(), text(root.read("/proc/pressure/memory")));
    result.insert("processes".into(), Value::Array(processes));
    Ok(result)
}

/// `PurePath.stem` (Python 3.14: a trailing dot is a suffix too; a leading one is not).
fn stem(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    }
}

pub fn thermal(root: &Root) -> Value {
    let mut zones = Vec::new();
    for path in root.glob("/sys/class/thermal/thermal_zone*") {
        let mut trips = Vec::new();
        for trip in util::glob_dir(&path, "trip_point_*_temp") {
            let trip_name = name(&trip);
            trips.push(json!({"name": stem(&trip_name), "temperature_millicelsius": number(read(&trip).as_deref()),
                              "type": read(&trip.with_file_name(trip_name.replace("_temp", "_type")))}));
        }
        zones.push(
            json!({"name": name(&path), "type": read(&path.join("type")),
                          "temperature_millicelsius": number(read(&path.join("temp")).as_deref()),
                          "trips": trips}),
        );
    }
    let cooling: Vec<Value> = root
        .glob("/sys/class/thermal/cooling_device*")
        .iter()
        .map(|p| {
            json!({"name": name(p), "type": read(&p.join("type")),
                   "state": number(read(&p.join("cur_state")).as_deref()),
                   "max_state": number(read(&p.join("max_state")).as_deref())})
        })
        .collect();
    json!({"zones": zones, "cooling": cooling, "battery_temperature": null,
           "battery_temperature_reason": "no_qualified_pack_sensor"})
}

pub fn power(root: &Root) -> Result<Value, Failure> {
    let fields = [
        "type",
        "status",
        "health",
        "online",
        "present",
        "usb_type",
        "voltage_now",
        "voltage_min_design",
        "voltage_max_design",
        "constant_charge_current",
        "constant_charge_current_max",
        "constant_charge_voltage",
        "constant_charge_voltage_max",
        "input_current_limit",
    ];
    let mut supplies = Vec::new();
    for path in root.glob("/sys/class/power_supply/*") {
        let mut supply = Map::new();
        for f in fields {
            supply.insert(f.into(), text(read(&path.join(f))));
        }
        supply.insert("name".into(), json!(name(&path)));
        supplies.push(Value::Object(supply));
    }
    let mut policy = root.json_object("/run/y2/power.json");
    let boot = util::boot_id(root);
    let fresh = is_int(policy.get("monotonic_ns"))
        .is_some_and(|t| (0..5_000_000_000).contains(&(i128::from(clock::monotonic_ns()) - t)));
    if !get_eq(&policy, "boot_id", &boot) || !fresh {
        policy = serde_json::from_value(
            json!({"state": "Unavailable", "reason": "policy_not_running_or_stale"}),
        )
        .expect("literal");
    }
    let battery = match policy.get("battery") {
        None => Map::new(),
        Some(Value::Object(map)) => map.clone(),
        Some(_) => return Err(Failure::Fallback), // AttributeError on .get
    };
    let get = |k: &str| battery.get(k).cloned().unwrap_or(Value::Null);
    let sleep = super::super::sleep::product(
        &root.json_object("/data/system/platform/suspend-last.json"),
        root.read("/proc/sys/kernel/random/boot_id").as_deref(),
    )?;
    Ok(json!({
        "supplies": supplies, "units": {"voltage": "uV", "configured_current": "uA"},
        "battery": battery.clone(),
        "measured_current_ua": get("current_ua"), "soc_percent": get("percent"),
        "soc_source": get("source"), "soc_confidence": get("confidence"),
        "pack_temperature": get("temperature_millicelsius"),
        "unavailable_reason": "not_exposed_by_qualified_y2_battery_driver",
        "normal_boot": root.integer("/sys/firmware/y2_boot/normal_boot"),
        "low_battery": policy, "sleep": sleep,
    }))
}

// ---- storage -----------------------------------------------------------------------------------

/// Product capacity budget, not electrical thresholds. Never delete user data.
fn space_state(available: i128, total: i128, readonly: bool, error: bool) -> &'static str {
    if error {
        "Failed"
    } else if readonly {
        "ReadOnlyRisk"
    } else if available < (32 * 1024 * 1024).max(total / 20) {
        "CriticalSpace"
    } else if available < (96 * 1024 * 1024).max(total / 10) {
        "LowSpace"
    } else {
        "Normal"
    }
}

/// The driver's negotiated-mode line as typed fields; absent on older kernels.
pub fn storage_mode(text: Option<&str>) -> Value {
    let Some(text) = text.filter(|t| !t.is_empty()) else {
        return Value::Null;
    };
    let mut mode = Map::new();
    for (key, value) in util::findall2(regex!(r"([a-z0-9_]+)=(\S+)"), text) {
        let typed = if regex!(r"\A-?\d+\z").is_match(&value) && value.is_ascii() {
            int(&value).map_or(Value::Null, Value::from)
        } else if regex!(r"\A0x[0-9a-f]+\z").is_match(&value) {
            i64::from_str_radix(&value[2..], 16).map_or(Value::Null, Value::from)
        } else {
            json!(value)
        };
        mode.insert(key, typed);
    }
    Value::Object(mode)
}

/// The card still inserted that the last mount refused (its check failed), so the player can say so
/// instead of "no card" (issue #44); {} otherwise. A removed or reinserted card has another disk
/// sequence number.
fn refused_card(root: &Root) -> Map<String, Value> {
    let Some(Value::Object(last)) = root.json("/run/y2/media.json") else {
        return Map::new();
    };
    if last.get("reason").and_then(Value::as_str) != Some("filesystem_check_failed") {
        return Map::new();
    }
    let instance = last
        .get("source_instance")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let parts: Vec<&str> = if last.get("source_instance").is_some_and(Value::is_string) {
        instance.split(':').collect()
    } else {
        Vec::new()
    };
    if parts.len() != 5
        || device_instance(root, &parts[1..3].join(":")).as_deref() != Some(instance)
    {
        return Map::new();
    }
    let findings = match last.get("filesystem_check") {
        Some(Value::Object(check)) => check.get("findings").cloned().unwrap_or(json!([])),
        _ => json!([]),
    };
    // ['timeout'] for a card too slow to check in 60 s, not one that needs repair (F12).
    serde_json::from_value(json!({"state": "Degraded", "reason": "filesystem_check_failed",
        "filesystem": last.get("filesystem").cloned().unwrap_or(Value::Null), "findings": findings}))
    .expect("literal")
}

/// `media.media_identity`: CID/partition geometry supplies identity when a legal FAT UUID is absent.
fn media_identity(entry: &Path, filesystem: &Value, uuid: &Value) -> Value {
    let mut device = realpath(entry);
    let partition = read(&entry.join("partition"));
    if partition.is_some() {
        device = device.parent().map(Path::to_path_buf).unwrap_or(device);
    }
    let cid = read(&device.join("device/cid"));
    let start = if partition.is_some() {
        read(&entry.join("start"))
    } else {
        Some("0".into())
    };
    let size = read(&entry.join("size"));
    let digits = |v: &Option<String>| {
        v.as_ref()
            .is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
    };
    if !truthy(uuid)
        && (cid.as_deref().is_none_or(str::is_empty) || !digits(&start) || !digits(&size))
    {
        return Value::Null;
    }
    let mut identity = json!({"cid": cid, "geometry": {"partition": partition, "start": start, "size": size},
                              "filesystem": filesystem, "uuid": uuid});
    let encoded = dumps_sorted(&identity, Separators::Spaced).expect("identity");
    let digest: String = Sha256::digest(encoded.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    identity["id"] = json!(digest);
    identity
}

fn statvfs(path: &Path) -> Option<libc::statvfs> {
    let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).ok()?;
    let mut vfs = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: a NUL-terminated path and a buffer for the result.
    (unsafe { libc::statvfs(name.as_ptr(), vfs.as_mut_ptr()) } == 0)
        // SAFETY: statvfs filled it.
        .then(|| unsafe { vfs.assume_init() })
}

pub fn storage(root: &Root) -> Result<Value, Failure> {
    let mounts = mountinfo(root.read("/proc/self/mountinfo").as_deref());
    let boot = root.read("/proc/sys/kernel/random/boot_id");
    let boot_value = text(boot.clone());
    let mut volumes = Vec::new();
    // Index order is a consumer contract: root, Y2DATA, SD, then Y2Media (internal user media).
    for target in ["/", "/data", "/media/sd", "/media/internal"] {
        let found = mounts_at(&mounts, target);
        // Stacked mounts are ambiguous to consumers; fail closed.
        if found.len() != 1 {
            let mut volume: Map<String, Value> = serde_json::from_value(json!({"path": target,
                "state": "Unavailable", "reason": "not_uniquely_mounted", "generation": null,
                "space_state": "Unavailable"}))
            .expect("literal");
            if target == "/media/sd" && found.is_empty() {
                volume.extend(refused_card(root));
            }
            volumes.push(Value::Object(volume));
            continue;
        }
        let mut item = found[0].clone();
        let device_id = field(&item, "device_id").to_owned();
        let mount_id = item.get("mount_id").cloned().unwrap_or(Value::Null);
        item.insert(
            "generation".into(),
            match boot.as_deref().filter(|b| !b.is_empty()) {
                Some(b) => json!(format!("{b}:{mount_id}:{device_id}")),
                None => Value::Null,
            },
        );
        let device = root.path(&format!("/sys/dev/block/{device_id}"));
        let real = realpath(&device).to_string_lossy().into_owned();
        let expected = if target == "/media/sd" {
            "11240000.mmc"
        } else {
            "11230000.mmc"
        };
        let instance = device_instance(root, &device_id);
        let controller_valid = real.contains(expected) && device.exists();
        item.insert("source_instance".into(), text(instance.clone()));
        item.insert("controller_valid".into(), json!(controller_valid));
        let mut state = if controller_valid { "Ready" } else { "Failed" };
        let mut reason: Option<&str> =
            (!controller_valid).then_some("source_missing_or_wrong_controller");
        let mut uuid = Value::Null;
        let claim = root.json_object("/run/y2/media-mount.json");
        if controller_valid {
            let source = field(&item, "source").to_owned();
            let answer = command::run(
                root,
                &["blkid", "-p", "-s", "UUID", "-o", "value", &source],
                command::TIMEOUT,
            );
            if answer.ok {
                uuid = answer
                    .output
                    .filter(|o| !o.is_empty())
                    .map_or(Value::Null, Value::String);
            }
            let expected_uuid = match target {
                "/" => Some("79324c69-6e75-4801-8000-000000000101"),
                "/data" => Some("79324c69-6e75-4801-8000-000000000102"),
                "/media/internal" => Some("79324c69-6e75-4801-8000-000000000103"),
                _ => None,
            };
            if let Some(expected) = expected_uuid {
                if uuid.as_str() != Some(expected) {
                    (state, reason) = ("Failed", Some("filesystem_identity_invalid"));
                }
            }
            if target == "/media/sd" {
                let filesystem = item.get("filesystem").cloned().unwrap_or(Value::Null);
                let identity = media_identity(&device, &filesystem, &uuid);
                item.insert("identity".into(), identity.clone());
                let matches = [
                    ("boot_id", boot_value.clone()),
                    ("mount_id", mount_id.clone()),
                    ("device_id", json!(device_id)),
                    ("uuid", uuid.clone()),
                ]
                .iter()
                .all(|(k, v)| get_eq(&claim, k, v));
                if !matches {
                    (state, reason) = ("Failed", Some("mount_claim_changed_or_missing"));
                }
                let claimed = claim.get("identity").filter(|v| !v.is_null());
                if state == "Ready"
                    && (identity.is_null() || claimed.is_some_and(|c| !eq(c, &identity)))
                {
                    (state, reason) = ("Failed", Some("media_identity_changed_or_missing"));
                }
                if instance.is_none() || !get_eq(&claim, "source_instance", &text(instance.clone()))
                {
                    (state, reason) = ("Failed", Some("block_instance_changed"));
                }
            }
        }
        item.insert("uuid".into(), uuid);
        let readonly =
            has_option(&item, "options", "ro") || has_option(&item, "super_options", "ro");
        let mut protected = false;
        if target == "/media/sd" && readonly {
            // media.py mounts a healthy card read-only between writes: it is Ready; so is one that
            // plays while its check finishes in the background (F49).
            protected = matches!(
                claim.get("read_only").and_then(Value::as_str),
                Some("protected" | "checking")
            );
        }
        if target == "/media/sd" {
            item.insert("writable".into(), json!(!readonly));
        }
        match statvfs(&root.path(target)) {
            Some(vfs) => {
                let frsize = i128::from(vfs.f_frsize);
                let total = i128::from(vfs.f_blocks) * frsize;
                let available = i128::from(vfs.f_bavail) * frsize;
                item.insert("total_bytes".into(), json!(total as u64));
                item.insert("available_bytes".into(), json!(available as u64));
                item.insert(
                    "free_bytes".into(),
                    json!((i128::from(vfs.f_bfree) * frsize) as u64),
                );
                item.insert("available_inodes".into(), json!(vfs.f_favail));
                let mut space =
                    space_state(available, total, readonly && !protected, state == "Failed");
                if vfs.f_files > 0 && vfs.f_favail == 0 {
                    space = "CriticalSpace";
                }
                item.insert("space_state".into(), json!(space));
                if target == "/media/sd" {
                    // A filesystem far smaller than its partition (a copied image, an old partition
                    // layout) hides most of the card; report it, never auto-fix it.
                    if let Some(sectors) = read(&device.join("size"))
                        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
                    {
                        let partition = int(&sectors).ok_or(Failure::Fallback)? as i128 * 512;
                        item.insert("partition_bytes".into(), json!(partition as u64));
                        item.insert(
                            "capacity_state".into(),
                            json!(if (total as f64) < 0.9 * partition as f64 {
                                "Undersized"
                            } else {
                                "Full"
                            }),
                        );
                    }
                }
            }
            None => {
                item.insert("space_state".into(), json!("Failed"));
                (state, reason) = ("Failed", Some("statvfs_failed"));
            }
        }
        if state == "Ready" && readonly && !protected {
            (state, reason) = ("Degraded", Some("read_only"));
            if target == "/media/sd" {
                // Why the platform mounted the card read-only (media.py): its check found errors or
                // did not finish; Repair can make it writable.
                let cause = claim.get("read_only").and_then(Value::as_str);
                item.insert(
                    "read_only_reason".into(),
                    json!(match cause {
                        Some(c @ ("filesystem_errors" | "check_timed_out")) => c,
                        _ => "mounted_read_only",
                    }),
                );
            }
        }
        item.insert("state".into(), json!(state));
        item.insert("reason".into(), json!(reason));
        volumes.push(Value::Object(item));
    }
    let mut blocks = Vec::new();
    for path in root.glob("/sys/class/block/*") {
        let real = realpath(&path).to_string_lossy().into_owned();
        if !["11230000.mmc", "11240000.mmc"]
            .iter()
            .any(|c| real.contains(c))
        {
            continue;
        }
        blocks.push(
            json!({"name": name(&path), "stat": read(&path.join("stat")),
            "stat_unit": "Linux_block_stat_fields_sectors_512_bytes",
            "size_sectors": number(read(&path.join("size")).as_deref()),
            "type": read(&path.join("device/type")), "name_id": read(&path.join("device/name")),
            "cid": read(&path.join("device/cid")), "serial": read(&path.join("device/serial")),
            "io_error_count": number(read(&path.join("device/ioerr_cnt")).as_deref())}),
        );
    }
    let mut controllers = Vec::new();
    for path in root.glob("/sys/bus/platform/devices/*.mmc/y2_performance") {
        let parent = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut controller = Map::new();
        controller.insert("name".into(), json!(name(&parent)));
        for (key, value) in findall_map(
            regex!(r"(cap_hz|actual_hz|transport_errors|fallbacks|clock_error)=(-?\d+)"),
            read(&path).as_deref().unwrap_or(""),
        ) {
            controller.insert(key, json!(int(&value).ok_or(Failure::Fallback)?));
        }
        controller.insert(
            "mode".into(),
            storage_mode(read_limited(&parent.join("y2_storage"), 4096).as_deref()),
        );
        controller.insert(
            "runtime_pm".into(),
            storage_mode(read_limited(&parent.join("y2_runtime_pm"), 4096).as_deref()),
        );
        controller.insert(
            "runtime_status".into(),
            text(read(&parent.join("power/runtime_status"))),
        );
        controller.insert(
            "tuning".into(),
            text(read_limited(&parent.join("y2_tune_diag"), 4096)),
        );
        controllers.push(Value::Object(controller));
    }
    Ok(json!({
        "volumes": volumes, "blocks": blocks, "controllers": controllers,
        "internal_setup": system::setup_status(root),
        "media": {"last_operation": root.json("/run/y2/media.json"),
                  "lifecycle": root.json("/run/y2/media-lifecycle.json")},
        "bus_parameters": root.read("/sys/kernel/debug/mmc0/ios"),
        "sd_bus_parameters": root.read("/sys/kernel/debug/mmc1/ios"),
    }))
}

// ---- wifi --------------------------------------------------------------------------------------

/// A confirmed lease is re-probed every 300 s (network.DnsProbeSchedule); this keeps Online across
/// one slow or skipped re-check.
const DNS_FRESH_S: f64 = 420.0;

fn wifi_state(
    radio: bool,
    wpa: &Map<String, Value>,
    address: bool,
    route: bool,
    dns: bool,
    dhcp_error: &Value,
) -> (Value, Value) {
    let s = |v: &str| json!(v);
    if !radio {
        return (s("Off"), s("radio_off"));
    }
    if wpa.is_empty() {
        return (s("Starting"), s("supplicant_unavailable"));
    }
    let state = wpa.get("wpa_state").and_then(Value::as_str);
    // Y2Reborn #20: wrong password. The supplicant goes on scanning and trying; without this the
    // player showed "Connecting…" and "Saved", never why.
    if state != Some("COMPLETED") && wpa.get("credential_failure").is_some_and(truthy) {
        return (s("Failed"), wpa["credential_failure"].clone());
    }
    match state {
        Some("SCANNING") => return (s("Scanning"), Value::Null),
        Some("ASSOCIATING" | "ASSOCIATED" | "4WAY_HANDSHAKE" | "GROUP_HANDSHAKE") => {
            return (s("Associating"), Value::Null)
        }
        Some("COMPLETED") => {}
        _ => {
            return (
                s("Failed"),
                wpa.get("failure_reason")
                    .cloned()
                    .unwrap_or(s("not_authenticated")),
            )
        }
    }
    if !address {
        return if truthy(dhcp_error) {
            (s("Failed"), dhcp_error.clone())
        } else {
            (s("AcquiringIP"), s("no_address"))
        };
    }
    if !route {
        return (s("Authenticated"), s("no_default_route"));
    }
    if !dns {
        return (s("Authenticated"), s("dns_unavailable"));
    }
    (s("Online"), Value::Null)
}

/// The global addresses of `ip -j -4 addr`, as Python's comprehension reads them: Err(Fallback)
/// where it raises an AttributeError, Ok(None) where it raises what `wifi` catches.
fn addresses(output: Option<&str>) -> Result<Option<Vec<Value>>, Failure> {
    let text = output.filter(|o| !o.is_empty()).unwrap_or("[]");
    let Ok(parsed) = serde_json::from_str::<Value>(text) else {
        return Ok(None);
    };
    let links = match parsed {
        Value::Array(items) => items,
        Value::Object(map) if map.is_empty() => Vec::new(),
        Value::String(s) if s.is_empty() => Vec::new(),
        Value::Object(_) | Value::String(_) => return Err(Failure::Fallback),
        _ => return Ok(None), // not iterable: TypeError
    };
    let mut found = Vec::new();
    for link in links {
        let Value::Object(link) = link else {
            return Err(Failure::Fallback);
        };
        let infos = match link.get("addr_info") {
            None => Vec::new(),
            Some(Value::Array(items)) => items.clone(),
            Some(Value::Object(m)) if m.is_empty() => Vec::new(),
            Some(Value::String(s)) if s.is_empty() => Vec::new(),
            Some(Value::Object(_) | Value::String(_)) => return Err(Failure::Fallback),
            Some(_) => return Ok(None),
        };
        for info in infos {
            let Value::Object(info) = info else {
                return Err(Failure::Fallback);
            };
            if info.get("scope").and_then(Value::as_str) == Some("global") {
                match info.get("local") {
                    Some(local) => found.push(local.clone()),
                    None => return Ok(None), // KeyError
                }
            }
        }
    }
    Ok(Some(found))
}

pub fn wifi(root: &Root) -> Result<Value, Failure> {
    let radio = root.path("/sys/class/net/wlan0").exists();
    let started = clock::monotonic_ns();
    // Station statistics can hold the control loop for about 2.1 s on this firmware. Bound the
    // observation above that measured delay.
    let wpa = radio.then(|| command::run(root, &["wpa_cli", "-i", "wlan0", "status"], 3.0));
    let duration = radio.then(|| clock::monotonic_ns() - started);
    let mut properties: Map<String, Value> =
        key_values(wpa.as_ref().and_then(|w| w.output.as_deref()), "=")
            .into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect();
    let boot = util::boot_id(root);
    let failure = root.json_object("/run/y2/wifi-failure.json");
    if get_eq(&failure, "boot_id", &boot)
        && fresh_seconds(failure.get("monotonic_s"), 120.0)
        && failure.get("error").is_some_and(truthy)
    {
        properties.insert("failure_reason".into(), failure["error"].clone());
    }
    let mut failed_network = Value::Null;
    if let Some(Value::Object(credential)) = failure.get("credential") {
        if credential.get("error").is_some_and(truthy) && get_eq(credential, "boot_id", &boot) {
            // Until the next connect (network.py drops it there).
            properties.insert("credential_failure".into(), credential["error"].clone());
            failed_network = credential.get("network_id").cloned().unwrap_or(Value::Null);
        }
    }
    let (ip, routes) = if radio {
        (
            command::run(
                root,
                &["ip", "-j", "-4", "addr", "show", "dev", "wlan0"],
                command::TIMEOUT,
            )
            .output,
            command::run(
                root,
                &["ip", "-j", "-4", "route", "show", "default", "dev", "wlan0"],
                command::TIMEOUT,
            )
            .output,
        )
    } else {
        (None, None)
    };
    let (addresses, default_route) = match addresses(ip.as_deref())? {
        Some(found) => match serde_json::from_str::<Value>(
            routes.as_deref().filter(|r| !r.is_empty()).unwrap_or("[]"),
        ) {
            Ok(route) => (found, route),
            Err(_) => (Vec::new(), json!([])),
        },
        None => (Vec::new(), json!([])),
    };
    let dns = root.json_object("/run/y2/dns.json");
    // Configuration alone never constitutes DNS readiness. Match network epoch.
    let epoch = text(root.read("/run/y2/wifi-epoch"));
    let bssid = properties.get("bssid").cloned().unwrap_or(Value::Null);
    let dns_ready = util::is_number(dns.get("monotonic_s")).is_some()
        && y2p_core::py::is_true(dns.get("ok"))
        && get_eq(&dns, "boot_id", &boot)
        && truthy(&epoch)
        && get_eq(&dns, "epoch", &epoch)
        && util::contains(&addresses, dns.get("address").unwrap_or(&Value::Null))
        && get_eq(&dns, "bssid", &bssid)
        && fresh_seconds(dns.get("monotonic_s"), DNS_FRESH_S);
    let dhcp = root.json_object("/run/y2/dhcp.json");
    let dhcp_error = if get_eq(&dhcp, "boot_id", &boot)
        && get_eq(&dhcp, "epoch", &text(root.read("/run/y2/wifi-epoch")))
    {
        dhcp.get("error").cloned().unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let (state, reason) = wifi_state(
        radio,
        &properties,
        !addresses.is_empty(),
        truthy(&default_route),
        dns_ready,
        &dhcp_error,
    );
    let mut stats = Map::new();
    for path in root.glob("/sys/class/net/wlan0/statistics/*") {
        stats.insert(name(&path), number(read(&path).as_deref()));
    }
    let previous = root.json_object("/run/y2/network.json");
    let mut signal = match previous.get("signal_observation") {
        Some(Value::Object(map)) => Value::Object(map.clone()),
        Some(other) if truthy(other) => return Err(Failure::Fallback), // .get on a non-dict
        _ => json!({}),
    };
    let fresh = {
        let s = signal.as_object().expect("object");
        get_eq(s, "boot_id", &boot)
            && get_eq(s, "bssid", &bssid)
            && fresh_seconds(s.get("monotonic_s"), 30.0)
    };
    let mut rssi = if fresh {
        previous.get("rssi_dbm").cloned().unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let association = properties.get("wpa_state").cloned().unwrap_or(Value::Null);
    if association.as_str() == Some("COMPLETED") && !fresh {
        let poll = command::run(root, &["wpa_cli", "-i", "wlan0", "signal_poll"], 3.0);
        let values = key_values(poll.output.as_deref(), "=");
        rssi = number(values.get("RSSI").map(String::as_str));
        signal = json!({"boot_id": boot, "bssid": bssid, "monotonic_s": clock::monotonic(),
                        "ok": poll.ok, "reason": poll.reason});
    } else if association.as_str() != Some("COMPLETED") {
        rssi = Value::Null;
        signal = Value::Null;
    }
    let credential = properties
        .get("credential_failure")
        .cloned()
        .unwrap_or(Value::Null);
    Ok(json!({
        "state": state, "reason": reason, "radio_present": radio,
        // The saved network whose password failed (wpa_supplicant's id).
        "failed_network_id": if state == "Failed" && eq(&reason, &credential) { failed_network } else { Value::Null },
        // A missed control query does not prove a radio disconnect. Preserve the actual failure and
        // latency separately from event counters.
        "supplicant_observation": {"ok": wpa.as_ref().map(|w| w.ok),
                                   "reason": match &wpa { Some(w) => json!(w.reason), None => json!("radio_off") },
                                   "duration_ns": duration},
        "association": association, "association_id": bssid,
        "ip_addresses": addresses, "default_route": default_route, "dns_ready": dns_ready,
        "rssi_dbm": rssi, "signal_observation": signal, "traffic_counters": stats,
        "events": root.json("/run/y2/wifi-events.json"), "reconnect_owner": "wpa_supplicant",
        "power_save": root.json("/run/y2/wifi-power.json"),
        "coexistence": root.json("/run/y2/coexistence.json"),
    }))
}

// ---- readiness and the snapshot ----------------------------------------------------------------

fn readiness(status: &Snapshot) -> Result<Value, Failure> {
    let item = |state: &str, reason: Value| json!({"state": state, "reason": reason});
    let empty = Vec::new();
    let volumes = status["storage"]["volumes"].as_array().unwrap_or(&empty);
    let first: Vec<&Value> = volumes.iter().take(2).collect();
    let all_ready = first.iter().all(|v| v["state"] == "Ready");
    let storage_state = if all_ready {
        if first.iter().all(|v| v["space_state"] == "Normal") {
            "Ready"
        } else {
            "Degraded"
        }
    } else {
        "Failed"
    };
    let storage_reason = first
        .iter()
        .find(|v| v["state"] != "Ready")
        .map_or(Value::Null, |v| {
            v.get("reason").cloned().unwrap_or(Value::Null)
        });
    let wifi = &status["wifi"];
    let wifi_state = match wifi["state"].as_str() {
        Some("Online") => "Ready",
        Some("Off") => "Unavailable",
        Some("Failed") => "Failed",
        Some("Authenticated") => "Degraded",
        _ => "Starting",
    };
    let profile = &status["audio"]["qualified_profile"];
    let valid = profile.as_object().is_some_and(|p| {
        p.get("schema").and_then(Value::as_str) == Some("org.y2linux.audio-qualification/v1")
            && p.get("card").and_then(Value::as_str) == Some("Y2Audio")
            && get_eq(p, "channels", &json!(2))
            && p.get("qualified_formats")
                .and_then(Value::as_array)
                .is_some_and(|f| util::contains(f, &json!("S16_LE")))
            && p.get("qualified_rates")
                .and_then(Value::as_array)
                .is_some_and(|r| {
                    r.iter()
                        .any(|v| y2p_core::py::exact_int(Some(v)) == Some(44100))
                })
    });
    let cards = status["audio"]["cards"].as_str().unwrap_or_default();
    let audio = regex!(r"(?m)^\s*\d+\s+\[Y2Audio\s*\]").is_match(cards) && valid;
    let update = &status["system"]["update"];
    let update_state = match update.get("state") {
        Some(Value::Array(_) | Value::Object(_)) => return Err(Failure::Fallback), // unhashable
        Some(Value::String(s)) => match s.as_str() {
            "Idle" | "Acknowledged" => "Ready",
            "RolledBack" | "RollbackPending" => "Degraded",
            "Queued" | "PendingHealth" => "Starting",
            "Failed" | "RescueRequired" => "Failed",
            _ => "Unavailable",
        },
        _ => "Unavailable",
    };
    let bluetooth = &status["bluetooth"];
    Ok(json!({
        "storage": item(storage_state, storage_reason),
        "wifi": item(wifi_state, wifi["reason"].clone()),
        "bluetooth": {"state": bluetooth["state"], "reason": bluetooth.get("reason").cloned().unwrap_or(Value::Null)},
        "audio": item(if audio { "Ready" } else { "Unavailable" }, json!("profile_does_not_imply_current_physical_acceptance")),
        "update": item(update_state, update.get("failure").cloned().unwrap_or(Value::Null)),
    }))
}

/// `observe.snapshot(ctx, pids, pss, interval)`.
pub fn snapshot(root: &Root, pids: &[i64], pss: bool, interval: f64) -> Result<Snapshot, Failure> {
    let start = clock::monotonic_ns();
    let before = cpu(root)?;
    let mut after = before.clone();
    if interval > 0.0 {
        std::thread::sleep(std::time::Duration::from_secs_f64(interval));
        after = cpu(root)?;
        let busy = utilization(before.get("ticks"), after.get("ticks"));
        after.insert("utilization_percent".into(), busy);
    }
    let mut result = Map::new();
    result.insert("schema".into(), json!(y2p_contract::schema::STATUS));
    result.insert(
        "record".into(),
        Value::Object(util::record(root, "observation", Map::new())),
    );
    result.insert("cpu".into(), Value::Object(after));
    result.insert("memory".into(), Value::Object(memory(root, pids, pss)?));
    result.insert("thermal".into(), thermal(root));
    result.insert("power".into(), power(root)?);
    result.insert("storage".into(), storage(root)?);
    result.insert("wifi".into(), wifi(root)?);
    result.insert("bluetooth".into(), bluetooth::bluetooth(root)?);
    result.insert("system".into(), system::system(root)?);
    let mut hw = Map::new();
    let base = root.path("/");
    for path in root.glob("/proc/asound/card*/pcm*/sub*/hw_params") {
        let relative = path
            .strip_prefix(&base)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        hw.insert(relative, text(read(&path)));
    }
    result.insert("audio".into(), json!({
        "cards": root.read("/proc/asound/cards"), "pcm": root.read("/proc/asound/pcm"),
        "headphone_jack_present": null, "headphone_jack_reason": "codec_irq_not_qualified",
        "qualified_profile": root.json("/etc/y2linux/audio-qualified.json"),
        "enabled_profile": root.json("/etc/y2linux/audio-enabled.json"),
        "pcm_contract": root.json("/sys/devices/platform/11220000.audio-controller/pcm_contract"),
        "hw_params": hw,
    }));
    let ready = readiness(&result)?;
    result.insert("readiness".into(), ready);
    let record = result["record"].as_object_mut().expect("record");
    record.insert("result".into(), json!("observed"));
    record.insert("units".into(), json!({"collection_duration": "ns"}));
    record.insert(
        "collection_duration_ns".into(),
        json!(clock::monotonic_ns() - start),
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_literals_follow_int_base_zero() {
        assert_eq!(int_base0("0x1f"), Some(31));
        assert_eq!(int_base0("0X_1"), Some(1));
        assert_eq!(int_base0("12"), Some(12));
        assert_eq!(int_base0("0"), Some(0));
        assert_eq!(int_base0("00"), Some(0));
        assert_eq!(int_base0("010"), None);
        assert_eq!(int_base0("-0b11"), Some(-3));
        assert_eq!(int_base0("1__0"), None);
        assert_eq!(int_base0("zz"), None);
    }

    #[test]
    fn rounding_matches_python() {
        // round(x, 3) in CPython
        assert_eq!(round3(2.0005), 2.0);
        assert_eq!(round3(13.0434782608), 13.043);
        assert_eq!(round3(0.0015), 0.002);
        assert_eq!(round3(100.0), 100.0);
    }

    #[test]
    fn mount_lines_carry_every_field() {
        let mounts = mountinfo(Some(
            "30 22 179:9 / /data rw,noatime - ext4 /dev/mmcblk0p9 rw\nx\n31 1 0:1 / /my\\040dir rw - tmpfs t rw",
        ));
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0]["options"], json!(["rw", "noatime"]));
        assert_eq!(mounts[1]["path"], "/my dir");
    }
}
