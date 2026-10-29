//! Idle completion: whether C1 (WFI), C2 (SLIDLE) and C3 (DORMANT) are registered, enabled and
//! entered, with what blocks C2 (the clocks' slow-idle blockers) and C3 (the SPM's dormant preflight:
//! peripheral, infrastructure and display clocks, the UART sleep handshake) and the owner of each.
// SPDX-License-Identifier: GPL-2.0-only
use crate::commands::observation::util::{findall_map, reads_by_name, text};
use crate::regex;
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

fn runtime_status(root: &Root, node: &str) -> Value {
    text(root.read(&format!(
        "/sys/bus/platform/devices/{node}/power/runtime_status"
    )))
}

/// Counters come from cpuidle's returned state; absent is never a PASS.
pub(super) fn idle_completion(root: &Root, states: &[Map<String, Value>]) -> Map<String, Value> {
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
