//! Idle completion: whether C1 (WFI), C2 (SLIDLE) and C3 (DORMANT) are registered, enabled and
//! entered, with what blocks C2 (the clocks' slow-idle blockers) and C3 (the SPM's dormant preflight:
//! peripheral, infrastructure and display clocks, the UART sleep handshake) and the owner of each.
// SPDX-License-Identifier: GPL-2.0-only
use super::cpu::IdleState;
use crate::commands::observation::util::{findall_map, reads_by_name};
use crate::regex;
use serde::Serialize;
use serde_json::{json, Map, Value};
use y2p_core::Root;

/// An integer literal with an optional `0x`/`0o`/`0b` prefix and `_` separators (the preflight's
/// masks), or None.
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

fn runtime_status(root: &Root, node: &str) -> Option<String> {
    root.read(&format!(
        "/sys/bus/platform/devices/{node}/power/runtime_status"
    ))
}

/// `cpu.idle_completion`: C1, C2 and C3 in this order.
#[derive(Serialize)]
#[allow(non_snake_case)]
pub(super) struct IdleCompletion {
    C1: Entered,
    C2: Level<SlowIdle>,
    C3: Level<Dormant>,
}

/// One idle level: how it was entered, then what blocks it.
#[derive(Serialize)]
struct Level<T> {
    #[serde(flatten)]
    entered: Entered,
    #[serde(flatten)]
    blocked: T,
}

/// Whether an idle level is registered and enabled, and its counters summed over its rows (null
/// when a row cannot tell). Counters come from cpuidle's returned state; absent is never a PASS.
#[derive(Serialize)]
struct Entered {
    registered: bool,
    enabled: Option<bool>,
    entries: Option<i64>,
    residency_us: Option<i64>,
    failures: Option<i64>,
    per_cpu: Vec<IdleState>,
}

impl Entered {
    /// The rows named `state`, of every CPU or of cpu0 only.
    fn of(states: &[IdleState], state: &str, every_cpu: bool) -> Self {
        let rows: Vec<IdleState> = states
            .iter()
            .filter(|s| s.name.as_deref() == Some(state) && (every_cpu || s.cpu == "cpu0"))
            .cloned()
            .collect();
        let total = |field: fn(&IdleState) -> Option<i64>| -> Option<i64> {
            let values: Vec<i64> = rows.iter().map(field).collect::<Option<_>>()?;
            (!values.is_empty()).then(|| values.iter().map(|v| i128::from(*v)).sum::<i128>() as i64)
        };
        Entered {
            registered: !rows.is_empty(),
            enabled: (!rows.is_empty()).then(|| rows.iter().all(|r| r.disabled == Some(0))),
            entries: total(|r| r.usage),
            residency_us: total(|r| r.time_us),
            failures: total(|r| r.rejected),
            per_cpu: rows,
        }
    }
}

/// A clock that blocks an idle level, with the device that owns it.
#[derive(Serialize)]
struct Blocker {
    #[serde(skip_serializing_if = "Option::is_none")]
    group: Option<&'static str>,
    bit: u32,
    clock: &'static str,
    owner: &'static str,
    runtime_status: Option<String>,
}

#[derive(Serialize)]
struct DmaOwner {
    name: &'static str,
    runtime_status: Option<String>,
}

/// What blocks C2 (SLIDLE): the clocks' slow-idle blocker mask.
#[derive(Serialize)]
struct SlowIdle {
    attribution: crate::commands::observation::util::Readings,
    last_blocker: Option<String>,
    blocker_owners: Option<Vec<Blocker>>,
    shared_dma_owners: Vec<DmaOwner>,
}

/// The slow-idle blocker mask's bits: (bit, clock, owner, device).
const SLOW_IDLE_BLOCKERS: [(u32, &str, &str, Option<&str>); 8] = [
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
];

impl SlowIdle {
    fn read(root: &Root) -> Self {
        let mask = root.integer("/sys/module/clocks/parameters/slow_blockers");
        let blocker_owners = mask.map(|mask| {
            SLOW_IDLE_BLOCKERS
                .iter()
                .filter(|(bit, ..)| mask & (1 << bit) != 0)
                .map(|&(bit, clock, owner, node)| Blocker {
                    group: None,
                    bit,
                    clock,
                    owner,
                    runtime_status: node.and_then(|n| runtime_status(root, n)),
                })
                .collect()
        });
        SlowIdle {
            attribution: reads_by_name(root, "/sys/module/idle/parameters/slow_*", false),
            last_blocker: root.read("/sys/module/clocks/parameters/slow_blocker_names"),
            blocker_owners,
            shared_dma_owners: ["18070000.connectivity", "11007000.i2c", "11008000.i2c"]
                .into_iter()
                .map(|name| DmaOwner {
                    name,
                    runtime_status: runtime_status(root, name),
                })
                .collect(),
        }
    }
}

/// What blocks C3 (DORMANT): the SPM's dormant preflight (peripheral, infrastructure and display
/// clocks) and the UART sleep handshake.
#[derive(Serialize)]
struct Dormant {
    preflight: Option<String>,
    boot_policy: Map<String, Value>,
    unused_clock_handoff: Option<String>,
    context: Option<String>,
    uart_clock_handoff: Option<String>,
    uart_sleep: Map<String, Value>,
    entry_budget: Option<i64>,
    display_idle: crate::commands::observation::util::Readings,
    prerequisites: Map<String, Value>,
    unmet: Vec<String>,
    blocker_owners: Option<Vec<Blocker>>,
}

/// The device behind a preflight clock, if the snapshot knows one.
fn clock_device(clock: &str) -> Option<&'static str> {
    match clock {
        "USB0" => Some("11200000.usb"),
        "APDMA" | "BTIF" => Some("18070000.connectivity"),
        "MSDC0" => Some("11230000.mmc"),
        "MSDC1" => Some("11240000.mmc"),
        "I2C0" => Some("11007000.i2c"),
        "I2C1" => Some("11008000.i2c"),
        _ => None,
    }
}

impl Dormant {
    fn read(root: &Root) -> Self {
        let preflight_text =
            root.read("/sys/devices/platform/10006000.power-controller/dormant_preflight");
        let context = root.read("/sys/devices/platform/10006000.power-controller/state");
        let uart = findall_map(regex!(r"(\w+)=([^\s]+)"), context.as_deref().unwrap_or(""));
        let uart_sleep = uart
            .into_iter()
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
            .collect();
        let preflight = findall_map(
            regex!(r"(\w+)=([^\s]+)"),
            preflight_text.as_deref().unwrap_or(""),
        );
        let prerequisites = preflight
            .iter()
            .filter_map(|(k, v)| {
                k.strip_prefix("prerequisite_")
                    .map(|n| (n.to_owned(), json!(v == "1")))
            })
            .collect();
        let unmet = match preflight.get("unmet").filter(|u| !u.is_empty()) {
            Some(unmet) => unmet
                .trim_matches(',')
                .split(',')
                .map(String::from)
                .collect(),
            None => Vec::new(),
        };
        let blocker_owners = preflight_text
            .is_some()
            .then(|| preflight_blockers(root, &preflight));
        Dormant {
            preflight: preflight_text,
            boot_policy: root.json_object("/run/y2/cpu-idle-policy.json"),
            unused_clock_handoff: root.read("/sys/module/clocks/parameters/unused_handoff"),
            context,
            uart_clock_handoff: root.read("/sys/module/clocks/parameters/uart_sleep_handoff"),
            uart_sleep,
            entry_budget: root.integer("/sys/module/spm/parameters/dormant_budget"),
            display_idle: reads_by_name(root, "/sys/module/mm_clocks/parameters/*", false),
            prerequisites,
            unmet,
            blocker_owners,
        }
    }
}

/// Each set bit of the preflight's four blocker masks, by group, as a named clock with its owner.
fn preflight_blockers(
    root: &Root,
    preflight: &std::collections::BTreeMap<String, String>,
) -> Vec<Blocker> {
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
    let mut blockers = Vec::new();
    for (group, names, owner) in [
        ("peri_blockers", &PERIPHERAL[..], "peripheral controller"),
        ("infra_blockers", infra, "infrastructure clock owner"),
        ("disp0", &display[..], "DRM / inherited engine"),
        ("disp1", disp1, "DRM / inherited engine"),
    ] {
        let Some(mask) = preflight.get(group).and_then(|v| int_base0(v)) else {
            continue;
        };
        for bit in (0..32u32).filter(|bit| mask & (1 << bit) != 0) {
            let clock = names
                .iter()
                .find(|(b, _)| *b == bit)
                .map_or("unknown", |(_, n)| n);
            blockers.push(Blocker {
                group: Some(group),
                bit,
                clock,
                owner,
                runtime_status: clock_device(clock).and_then(|n| runtime_status(root, n)),
            });
        }
    }
    blockers
}

/// Counters come from cpuidle's returned state; absent is never a PASS.
pub(super) fn idle_completion(root: &Root, states: &[IdleState]) -> IdleCompletion {
    IdleCompletion {
        C1: Entered::of(states, "WFI", true),
        C2: Level {
            entered: Entered::of(states, "SLIDLE", false),
            blocked: SlowIdle::read(root),
        },
        C3: Level {
            entered: Entered::of(states, "DORMANT", false),
            blocked: Dormant::read(root),
        },
    }
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
}
