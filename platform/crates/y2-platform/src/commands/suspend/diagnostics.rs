//! Persistent sleep and USB diagnostics (`sleep-diagnostics.jsonl`) that survive a hold-Power restart.
//!
//! POWER-FINAL-14. 13's counters (VPROC sleep, PHY save-current, wheel sleep, INFRA power-down, the
//! golden capture at sleep entry) live only in kernel memory, and the USB-after-wake defect is only
//! recoverable by a restart, which erased them twice. Each row here is read-only sysfs/procfs plus a
//! filtered kernel-log tail, appended to a bounded private file on Y2DATA. A failure is counted and
//! never blocks, delays or changes sleep.
// SPDX-License-Identifier: GPL-2.0-only
use super::{journal::plain, parent_name, text_value, E};
use crate::commands::power::journal::usb;
pub(crate) use crate::commands::power::journal::usb::UsbNet;
use serde::Serialize;
use serde_json::Value;
use std::{fs::File, io::Read, os::unix::fs::OpenOptionsExt};
use y2p_core::mounts::data_ready;
use y2p_core::{
    json::{dumps, Object, Separators},
    power_journal::append_bounded,
    root::read,
    suspend_text::splitlines,
    Root,
};

pub const PATH: &str = "/data/system/platform/sleep-diagnostics.jsonl";
const LIMIT: u64 = 2_097_152;
const ROW_LIMIT: usize = 65_536;
const SPM: &str = "/sys/devices/platform/10006000.power-controller/";
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

/// The USB state (power::journal::usb, shared with the power journal's rows).
pub fn usb_net(root: &Root) -> Result<UsbNet, E> {
    usb::usb_net(root).map_err(|error| E::value(error.0))
}

/// The 13 sleep-parity switches and counters, the INFRA and SPM state; after a return also the
/// always-on/INFRA golden registers.
#[derive(Serialize)]
struct Features {
    vproc_sleep: Option<String>,
    vproc_sleep_state: Option<String>,
    pmic_sleep_state: Option<String>,
    lp_sel: Option<String>,
    phy_savecurrent: Object,
    wheel_sleep: Object,
    infra: Option<String>,
    spm: Option<String>,
    // 27: stock panel power-down and the idle VGP2 rail (ledger F20/F22).
    panel_stock_suspend: Option<String>,
    panel_power_down: Option<String>,
    vgp2_idle_off: Option<String>,
    // 32: deep idle during playback and how often it was admitted.
    deep_idle_audio: Option<String>,
    deep_audio_entries: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    golden: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pmic_golden: Option<Option<String>>,
}

fn features(root: &Root, golden: bool) -> Features {
    let text = |name: &str, limit| root.read_limited(name, limit);
    let each = |pattern: &str| {
        root.glob(pattern)
            .iter()
            .map(|p| (parent_name(p), text_value(read(p, 4096))))
            .collect()
    };
    Features {
        vproc_sleep: text("/sys/module/pwrap/parameters/vproc_sleep", 16),
        vproc_sleep_state: text("/sys/module/pwrap/parameters/vproc_sleep_state", 4096),
        pmic_sleep_state: text("/sys/module/pwrap/parameters/pmic_sleep_state", 4096),
        lp_sel: text("/sys/firmware/y2_pmic/lp_sel", 4096),
        phy_savecurrent: each("/sys/bus/platform/drivers/y2-usb/*/phy_savecurrent"),
        wheel_sleep: each("/sys/bus/i2c/devices/*-0051/sleep"),
        infra: text("/sys/firmware/y2_infra/state", 4096),
        spm: text(&format!("{SPM}state"), 8192),
        panel_stock_suspend: text("/sys/module/panel/parameters/stock_suspend", 16),
        panel_power_down: text("/sys/module/panel/parameters/power_down", 16),
        vgp2_idle_off: text("/sys/module/y2_cs43131/parameters/vgp2_idle_off", 16),
        deep_idle_audio: text("/sys/module/clocks/parameters/deep_idle_audio", 16),
        deep_audio_entries: text("/sys/module/clocks/parameters/deep_audio_entries", 32),
        // Read-only always-on/INFRA registers; the entry column is the last suspend-entry capture,
        // so they are read once Linux has returned.
        golden: golden.then(|| text(&format!("{SPM}golden"), 16384)),
        pmic_golden: golden.then(|| text("/sys/module/pwrap/parameters/pmic_golden", 16384)),
    }
}

/// The last relevant kernel messages, read non-blocking from /dev/kmsg.
fn kernel_tail(root: &Root, lines: usize, width: usize) -> Option<Vec<String>> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(root.path("/dev/kmsg"))
        .ok()?;
    let mut kept: Vec<String> = Vec::new();
    let mut block = vec![0u8; 8192];
    for _ in 0..16384 {
        let n = match read_once(&mut file, &mut block) {
            Ok(n) => n,
            Err(error) if error.raw_os_error() == Some(libc::EPIPE) => continue,
            Err(_) => break,
        };
        if n == 0 {
            break;
        }
        let text = String::from_utf8_lossy(&block[..n]).into_owned();
        for record in splitlines(&text) {
            if record.starts_with(' ') {
                continue;
            }
            let (header, message) = record.split_once(';').unwrap_or((record, ""));
            let fields: Vec<&str> = header.split(',').collect();
            let head: String = message.chars().take(96).collect();
            if KERNEL_TOKENS.iter().any(|t| head.contains(t)) {
                let stamp = fields.get(2).copied().unwrap_or("?");
                kept.push(format!("{stamp} {message}").chars().take(width).collect());
                if kept.len() > lines {
                    kept.drain(..kept.len() - lines);
                }
            }
        }
    }
    Some(kept)
}

/// One read of at most a block (an interrupted read is retried: a signal is no end of the log).
fn read_once(file: &mut File, block: &mut [u8]) -> std::io::Result<usize> {
    loop {
        match file.read(block) {
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            other => return other,
        }
    }
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
