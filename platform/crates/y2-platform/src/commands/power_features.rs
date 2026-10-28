//! `power_features.apply`: the POWER-FINAL-16/17/18/19/27/32 runtime power features, armed once the UI
//! has drawn its first frame (`boot-stage application_ready`), never earlier: the display domain must
//! not be cut while the loader-lit panel is still being taken over. Each feature is a kernel parameter
//! whose kernel default is the safe behaviour. The owner keeps one off for an A/B night with
//! /data/system/platform/power-features.json, e.g. {"dis_pdn": false, "golden_apply": 3}. A failure is
//! recorded and never blocks boot, sleep or the UI.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::{atomic_json, mkdir_parents, write_text, Failure};
use serde_json::{json, Map, Value};
use y2p_core::{py::exact_int, Root};

const OVERRIDE: &str = "/data/system/platform/power-features.json";
const STATE: &str = "/run/y2/power-features.json";

#[derive(Clone, Copy)]
enum Default {
    Flag,
    Mask(i64),
}

const FEATURES: [(&str, &str, Default); 9] = [
    // G1: display (DIS) power domain off at screen-off and in suspend.
    (
        "dis_pdn",
        "/sys/module/spm/parameters/dis_pdn",
        Default::Flag,
    ),
    // G3b: golden sleep entry, bitmask: 1 PLLs, 2 mux power-down, 4 PERI gates, 8 INFRA gates
    // (golden-apply-policy.h).
    (
        "golden_apply",
        "/sys/module/spm/parameters/golden_apply",
        Default::Mask(0xf),
    ),
    // 17: gate the never-used modem's peripheral clock requests (ledger F11); re-checked at every sleep
    // entry.
    (
        "md1_gate",
        "/sys/module/spm/parameters/md1_gate",
        Default::Flag,
    ),
    // 18: stock PMIC suspend fields, bitmask: 1 audio clocks, 2 unused PMIC clocks, 4 VREF18, 8 VSYS
    // buck sleep state (27) (pmic-sleep-policy.h).
    (
        "pmic_sleep",
        "/sys/module/pwrap/parameters/pmic_sleep",
        Default::Mask(0xf),
    ),
    // 19: gate the AP's PMIC_WRAP clock (INFRA bit 23) for the PCM sleep only.
    (
        "pwrap_gate",
        "/sys/module/spm/parameters/pwrap_gate",
        Default::Flag,
    ),
    // 27 (ledger F22): screen-off sends the stock GC9503V power-down sequence and leaves reset high
    // (panel.c); N keeps 26's reset-low sequence.
    (
        "panel_stock_suspend",
        "/sys/module/panel/parameters/stock_suspend",
        Default::Flag,
    ),
    // 27 (ledger F20): the CS43131's VGP2 rail turns off while the DAC is idle, as in stock; N keeps
    // 26's always-on rail (y2-cs43131.c).
    (
        "vgp2_idle_off",
        "/sys/module/y2_cs43131/parameters/vgp2_idle_off",
        Default::Flag,
    ),
    // 32 (ledger F34/F36): deep idle (SLIDLE/DORMANT) while the audio clock is on, i.e. during
    // screen-off playback, as stock dpidle; N keeps 31's rule.
    (
        "deep_idle_audio",
        "/sys/module/clocks/parameters/deep_idle_audio",
        Default::Flag,
    ),
    // #45: screen-off powers the panel down with its stock lcm_suspend and screen-on replays the stock
    // lcm_init (panel.c power_down); N keeps the LK-lit panel awake as before. On by default only for a
    // panel it was confirmed on (CONFIRMED_PANELS); others turn it on with the override.
    (
        "panel_power_down",
        "/sys/module/panel/parameters/power_down",
        Default::Flag,
    ),
];
/// The ST7701S was confirmed on the owner's player (2026-10-18, 6456), the GC9503V on samanz's player
/// (#45, Beta 8.7: normal picture through screen-off cycles and deep sleep; #42: overnight drain
/// 30-37 mA -> 1.9 mA).
const CONFIRMED_PANELS: [&str; 2] = ["st7701s", "gc9503v"];

const PANEL_RECORD: &str = "/data/system/platform/panel.json";
const PANEL_HINT: &str = "/sys/module/panel/parameters/panel_hint";
const PANEL_HINTS: [(&str, i64); 2] = [("gc9503v", 1), ("st7701s", 2)];
const PANEL_KIND: &str = "/sys/module/panel/parameters/panel_kind";

fn hint(name: &str) -> Option<i64> {
    PANEL_HINTS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, h)| *h)
}

/// This player's panel: LK's identification (y2_boot/metadata panel=: a gc9503v name, or st7701 with
/// lcm=1-; boot-policy.h), recorded on Y2DATA; on boots where LK did not identify it (lcm=0-st7701 can
/// be a fallback), the record. The kernel gets the record as panel_hint; it never overrides LK.
fn panel(root: &Root) -> Option<String> {
    let mut lk = None;
    for field in root
        .read("/sys/firmware/y2_boot/metadata")
        .unwrap_or_default()
        .split_whitespace()
    {
        if let Some(name) = field.strip_prefix("panel=") {
            lk = Some(name.to_owned());
        }
    }
    if !lk.as_deref().is_some_and(|n| hint(n).is_some()) {
        // The driver's own ID read at the first screen-on (stock compare_id registers).
        let kind = root.read(PANEL_KIND);
        lk = PANEL_HINTS
            .iter()
            .find(|(_, h)| Some(h.to_string()) == kind)
            .map(|(n, _)| (*n).to_owned());
    }
    let record = root.json_object(PANEL_RECORD);
    if let Some(lk) = lk {
        if record.get("panel").and_then(Value::as_str) != Some(lk.as_str()) {
            let path = root.path(PANEL_RECORD);
            let value =
                json!({"schema": 1, "panel": lk, "lcm": root.read("/sys/firmware/y2_boot/lcm")});
            let _ = mkdir_parents(path.parent().expect("parent"), 0o700)
                .and_then(|()| atomic_json(&path, &value, true));
        }
        return Some(lk);
    }
    let remembered = record.get("panel").and_then(Value::as_str)?;
    let value = hint(remembered)?;
    write_text(&root.path(PANEL_HINT), &value.to_string()).ok()?;
    Some(remembered.to_owned())
}

/// `encode(default, value)`.
fn encode(default: Default, value: &Value) -> Result<String, Failure> {
    match default {
        Default::Flag => Ok(if *value == Value::Bool(false) {
            "N"
        } else {
            "Y"
        }
        .into()),
        Default::Mask(maximum) => {
            let number = match value {
                Value::Bool(true) => Some(maximum),
                Value::Bool(false) => Some(0),
                other => exact_int(Some(other)),
            };
            match number {
                Some(n) if (0..=maximum).contains(&n) => Ok(n.to_string()),
                _ => Err(Failure::value("invalid_power_feature")),
            }
        }
    }
}

pub fn apply(root: &Root) -> Map<String, Value> {
    let override_ = root.json_object(OVERRIDE);
    let mut result = Map::new();
    for (name, path, default) in FEATURES {
        let wanted = if name == "panel_power_down" && !override_.contains_key(name) {
            json!(panel(root).is_some_and(|p| CONFIRMED_PANELS.contains(&p.as_str())))
        } else {
            override_.get(name).cloned().unwrap_or(match default {
                Default::Flag => json!(true),
                Default::Mask(m) => json!(m),
            })
        };
        let entry =
            match encode(default, &wanted).and_then(|value| write_text(&root.path(path), &value)) {
                Ok(()) => json!({"wanted": wanted, "value": root.read(path)}),
                Err(error) => json!({"wanted": wanted, "error": error.strerror()}),
            };
        result.insert(name.into(), entry);
    }
    let runtime = root.path(STATE);
    let _ = mkdir_parents(runtime.parent().expect("parent"), 0o700)
        .and_then(|()| atomic_json(&runtime, &json!({"schema": 1, "features": result}), false));
    result
}
