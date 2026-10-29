//! `sleep_diagnostics.py`: persistent sleep and USB diagnostics that survive a hold-Power restart.
//!
//! POWER-FINAL-14. 13's counters (VPROC sleep, PHY save-current, wheel sleep, INFRA power-down, the
//! golden capture at sleep entry) live only in kernel memory, and the USB-after-wake defect is only
//! recoverable by a restart, which erased them twice. Each row here is read-only sysfs/procfs plus a
//! filtered kernel-log tail, appended to a bounded private file on Y2DATA. A failure is counted and
//! never blocks, delays or changes sleep.
// SPDX-License-Identifier: GPL-2.0-only
use super::{journal::plain, text_value, E};
use serde::Serialize;
use serde_json::Value;
use std::{fs::File, io::Read, os::unix::fs::OpenOptionsExt, path::Path};
use y2p_core::mounts::data_ready;
use y2p_core::{
    json::{dumps, Object, Separators},
    power_journal::append_bounded,
    root::read,
    suspend_text::{split, splitlines, Regex},
    Root,
};

pub const PATH: &str = "/data/system/platform/sleep-diagnostics.jsonl";
const LIMIT: u64 = 2_097_152;
const ROW_LIMIT: usize = 65_536;
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

fn name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn parent_name(path: &Path) -> String {
    path.parent().map(name).unwrap_or_default()
}

/// `key_values(text, keys)`: `\b([a-z0-9_]+)=(\S+)`, one entry per key (None when absent).
fn key_values(text: Option<&str>, keys: &[&str]) -> Value {
    let pattern = Regex::new(r"\b([a-z0-9_]+)=(\S+)").expect("pattern");
    let mut found = serde_json::Map::new();
    for capture in pattern.captures_iter(text.unwrap_or_default()) {
        found.insert(capture[1].to_owned(), Value::String(capture[2].to_owned()));
    }
    let values: Object = keys
        .iter()
        .map(|k| {
            (
                (*k).to_owned(),
                found.get(*k).cloned().unwrap_or(Value::Null),
            )
        })
        .collect();
    Value::Object(values)
}

/// Local IPv4 addresses of one interface from /proc/net/fib_trie: no subprocess, no ioctl.
fn ipv4(root: &Root, interface: &str) -> Result<Vec<String>, E> {
    let routes = root
        .read_limited("/proc/net/route", 65_536)
        .unwrap_or_default();
    let mut networks = Vec::new();
    for line in splitlines(&routes).into_iter().skip(1) {
        let fields = split(line);
        if fields.len() >= 8 && fields[0] == interface {
            let hex = |text: &str| {
                y2p_core::suspend_text::int16(text).ok_or_else(|| {
                    E::value(format!(
                        "invalid literal for int() with base 16: {}",
                        y2p_core::py::repr(text)
                    ))
                })
            };
            networks.push((hex(fields[1])?, hex(fields[7])?));
        }
    }
    let address = Regex::new(r"^[|+`-]*\s*([0-9]+(?:\.[0-9]+){3})$").expect("pattern");
    let (mut found, mut previous): (Vec<String>, Option<String>) = (Vec::new(), None);
    let trie = root
        .read_limited("/proc/net/fib_trie", 262_144)
        .unwrap_or_default();
    for line in splitlines(&trie) {
        let text = y2p_core::root::python_strip(line);
        if text.contains("/32 host LOCAL") {
            if let Some(previous) = previous.as_deref().filter(|p| !p.is_empty()) {
                let mut value: i64 = 0;
                for (index, octet) in previous.split('.').enumerate() {
                    let octet: i64 = octet.parse().map_err(|_| E::value("octet"))?;
                    value |= octet << (8 * index);
                }
                if networks
                    .iter()
                    .any(|(network, mask)| value & mask == *network)
                    && !found.iter().any(|f| f == previous)
                {
                    found.push(previous.to_owned());
                }
            }
        }
        previous = match address.captures(text) {
            Some(capture) => Some(capture[1].to_owned()),
            None if text.contains('/') => previous,
            None => None,
        };
    }
    found.sort();
    Ok(found)
}

/// Which of the USB services run (`/proc/*/comm`).
#[derive(Serialize)]
struct Processes {
    umtprd: bool,
    dropbear: bool,
}

fn processes(root: &Root) -> Processes {
    let mut running = Processes {
        umtprd: false,
        dropbear: false,
    };
    if let Ok(entries) = std::fs::read_dir(root.path("/proc")) {
        for entry in entries.flatten() {
            let file = entry.file_name();
            let Some(text) = file.to_str() else { continue };
            if !text.is_empty() && text.chars().all(|c| c.is_ascii_digit()) {
                match read(&entry.path().join("comm"), 64).as_deref() {
                    Some("umtprd") => running.umtprd = true,
                    Some("dropbear") => running.dropbear = true,
                    _ => {}
                }
            }
        }
    }
    running
}

/// Compact USB gadget and USB network state (about 1 KiB), in sleep diagnostics and power journal
/// rows.
#[derive(Serialize)]
pub struct UsbNet {
    /// Each UDC's state.
    udc: Object,
    gadget_udc: Option<String>,
    /// Each controller's status counters (`USB_SUMMARY`, null when absent).
    controller: Object,
    /// Each controller's first `rxdiag` line.
    out_path: Object,
    usb0: Usb0,
    transfer: Value,
    processes: Processes,
}

#[derive(Serialize)]
struct Usb0 {
    operstate: Option<String>,
    carrier: Option<String>,
    flags: Option<String>,
    ipv4: Vec<String>,
    statistics: Object,
}

pub fn usb_net(root: &Root) -> Result<UsbNet, E> {
    let net = "/sys/class/net/usb0/";
    let statistics = [
        "rx_packets",
        "tx_packets",
        "rx_bytes",
        "tx_bytes",
        "rx_errors",
        "tx_errors",
        "rx_dropped",
        "tx_dropped",
    ]
    .iter()
    .map(|n| {
        let value = root.integer(&format!("{net}statistics/{n}"));
        ((*n).to_owned(), value.map_or(Value::Null, Value::from))
    })
    .collect();
    let udc = root
        .glob("/sys/class/udc/*")
        .iter()
        .map(|p| (name(p), text_value(read(&p.join("state"), 64))))
        .collect();
    let controller = root
        .glob("/sys/bus/platform/drivers/y2-usb/*/status")
        .iter()
        .map(|p| {
            (
                parent_name(p),
                key_values(read(p, 8192).as_deref(), &USB_SUMMARY),
            )
        })
        .collect();
    let out_path = root
        .glob("/sys/bus/platform/drivers/y2-usb/*/rxdiag")
        .iter()
        .map(|p| {
            let text = read(p, 4096).unwrap_or_default();
            let first: Vec<Value> = splitlines(&text)
                .into_iter()
                .take(1)
                .map(|l| Value::String(l.to_owned()))
                .collect();
            (parent_name(p), Value::Array(first))
        })
        .collect();
    let field = |n: &str, limit| root.read_limited(&format!("{net}{n}"), limit);
    Ok(UsbNet {
        udc,
        gadget_udc: root.read_limited("/sys/kernel/config/usb_gadget/y2/UDC", 128),
        controller,
        out_path,
        usb0: Usb0 {
            operstate: field("operstate", 64),
            carrier: field("carrier", 8),
            flags: field("flags", 32),
            ipv4: ipv4(root, "usb0")?,
            statistics,
        },
        transfer: plain(
            root.json("/run/y2/usb-transfer.json")
                .unwrap_or(Value::Null),
        ),
        processes: processes(root),
    })
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

/// One `os.read(fd, 8192)` (EINTR retried, as Python does).
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
        assert_eq!(ipv4(&root, "usb0").unwrap(), ["192.168.2.1"]);
        assert!(ipv4(&root, "wlan0").unwrap().is_empty());
        assert_eq!(
            key_values(Some("stage=a error=0 x=1"), &["stage", "irqs"]),
            serde_json::json!({"stage": "a", "irqs": null})
        );
    }
}

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod diagnostics_tests;
