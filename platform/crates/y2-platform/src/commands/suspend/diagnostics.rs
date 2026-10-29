//! Persistent sleep and USB diagnostics (`sleep-diagnostics.jsonl`) that survive a hold-Power restart.
//!
//! POWER-FINAL-14. 13's counters (VPROC sleep, PHY save-current, wheel sleep, INFRA power-down, the
//! golden capture at sleep entry) live only in kernel memory, and the USB-after-wake defect is only
//! recoverable by a restart, which erased them twice. Each row here is read-only sysfs/procfs plus a
//! filtered kernel-log tail, appended to a bounded private file on Y2DATA. A failure is counted and
//! never blocks, delays or changes sleep.
// SPDX-License-Identifier: GPL-2.0-only
use super::{journal::plain, E};
use crate::commands::power::journal::state::{features, kernel_tail, Features};
use crate::commands::power::journal::usb;
pub(crate) use crate::commands::power::journal::usb::UsbNet;
use serde::Serialize;
use serde_json::Value;
use y2p_core::mounts::data_ready;
use y2p_core::{
    json::{dumps, Separators},
    power_journal::append_bounded,
    Root,
};

pub const PATH: &str = "/data/system/platform/sleep-diagnostics.jsonl";
const LIMIT: u64 = 2_097_152;
const ROW_LIMIT: usize = 65_536;
/// The USB state (power::journal::usb, shared with the power journal's rows).
pub fn usb_net(root: &Root) -> Result<UsbNet, E> {
    usb::usb_net(root).map_err(|error| E::value(error.0))
}

/// `Diagnostics(ctx).record(stage, golden=stage == 'kernel_resumed')`: one row for a suspend
/// stage. `Ok(false)` when the data mount is not verified or a failure was counted.
pub fn record(root: &Root, stage: &str) -> Result<bool, E> {
    match row(root, stage, stage == "kernel_resumed") {
        Ok(written) => Ok(written),
        Err(error) if error.is_recorded_failure() => Ok(false),
        Err(error) => Err(error),
    }
}

/// One `sleep-diagnostics.jsonl` row (compact JSON in this order).
#[derive(Serialize)]
struct Row<'a> {
    schema: u8,
    stage: &'a str,
    usb_reason: Option<&'a str>,
    wall_time: f64,
    monotonic_s: f64,
    boottime_s: f64,
    boot_id: Option<String>,
    build_id: Value,
    features: Features,
    usb_net: UsbNet,
    kernel_pm: Option<String>,
    kernel_tail: Option<Vec<String>>,
}

fn row(root: &Root, stage: &str, golden: bool) -> Result<bool, E> {
    let now = super::monotonic();
    if !data_ready(root) {
        return Ok(false);
    }
    let build_id = match root.json("/etc/y2linux/versions.json") {
        Some(Value::Object(map)) => map.get("build_id").cloned().unwrap_or(Value::Null),
        _ => Value::Null,
    };
    let mut row = Row {
        schema: 1,
        stage,
        usb_reason: None,
        wall_time: super::wall(),
        monotonic_s: now,
        boottime_s: super::boottime(),
        boot_id: root.boot_id(),
        build_id: plain(build_id),
        features: features(root, golden),
        usb_net: usb_net(root)?,
        kernel_pm: root.read_limited("/sys/firmware/y2_pm/state", 8192),
        kernel_tail: kernel_tail(root, 80, 240),
    };
    let encode = |row: &Row| -> Result<Vec<u8>, E> {
        let text = dumps(row, Separators::Compact).map_err(|e| E::value(e.to_string()))?;
        Ok((text + "\n").into_bytes())
    };
    let mut encoded = encode(&row)?;
    // An oversized row loses the oldest quarter of its kernel tail at a time, never the row.
    while encoded.len() > ROW_LIMIT {
        let Some(tail) = row.kernel_tail.as_mut().filter(|t| !t.is_empty()) else {
            return Err(E::value("sleep_diagnostics_row_too_large"));
        };
        let drop = (tail.len() / 4).max(1);
        tail.drain(..drop);
        encoded = encode(&row)?;
    }
    append_bounded(root, PATH, &encoded, LIMIT, true).map_err(E::from)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::suspend::tests::Dir;
    use std::fs;

    fn put(dir: &Dir, name: &str, text: &str) {
        let path = dir.path().join(name.trim_start_matches('/'));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn addresses_come_from_the_fib_trie() {
        let dir = Dir::new();
        put(
            &dir,
            "/proc/net/route",
            "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\n\
             usb0\t0002A8C0\t00000000\t0001\t0\t0\t0\t00FFFFFF\n",
        );
        put(
            &dir,
            "/proc/net/fib_trie",
            "Main:\n  +-- 0.0.0.0/0 3 0 5\n     |-- 192.168.2.0\n        /24 link UNICAST\n     \
             |-- 192.168.2.1\n        /32 host LOCAL\n     |-- 10.0.0.1\n        /32 host LOCAL\n",
        );
        let root = Root::new(dir.path());
        assert_eq!(usb::ipv4(&root, "usb0").unwrap(), ["192.168.2.1"]);
        assert!(usb::ipv4(&root, "wlan0").unwrap().is_empty());
        assert_eq!(
            usb::key_values(Some("stage=a error=0 x=1"), &["stage", "irqs"]),
            serde_json::json!({"stage": "a", "irqs": null})
        );
    }
}

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod diagnostics_tests;
