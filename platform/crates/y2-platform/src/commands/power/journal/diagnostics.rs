//! The parts of `sleep_diagnostics` the power sample drives: a row around each USB state change
//! (POWER-FINAL-14), with the USB, network and sleep-parity state; `sleep-diagnostics.jsonl`.
// SPDX-License-Identifier: GPL-2.0-only
use super::{glob_name, json, parent_name, Dropped};
use crate::commands::time::wall;
use serde_json::Value;
use std::{fs, io::Read, os::unix::fs::OpenOptionsExt};
use y2p_core::{
    json::{dumps, Separators},
    json::{Object, ObjectExt},
    mounts::data_ready,
    power_journal::append_bounded,
    power_text::{key_values, python_hex, python_split, trie_address},
    root::read,
    Root,
};

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
        while encoded.len() > DIAGNOSTICS_ROW {
            // Too large: drop the oldest quarter of the kernel lines, as long as there are any.
            let Some(lines) = tail.as_mut().filter(|t| !t.is_empty()) else {
                break;
            };
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

#[cfg(test)]
mod tests {
    use super::*;
    use y2p_core::py::PyValue;

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
}
