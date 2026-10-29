//! The sleep-parity switches and counters and the filtered kernel-log tail that both writers of the
//! sleep diagnostics rows carry: the power journal's USB rows and the suspend group's stage rows.
// SPDX-License-Identifier: GPL-2.0-only
use serde::Serialize;
use serde_json::Value;
use std::{fs::File, io::Read, os::unix::fs::OpenOptionsExt, path::Path};
use y2p_core::{json::Object, root::read, text::splitlines, Root};

/// The SPM's sysfs directory.
pub(crate) const SPM: &str = "/sys/devices/platform/10006000.power-controller/";

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

fn parent_name(path: &Path) -> String {
    path.parent()
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The 13 sleep-parity switches and counters, the INFRA and SPM state; after a return also the
/// always-on/INFRA golden registers.
#[derive(Serialize)]
pub struct Features {
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

pub fn features(root: &Root, golden: bool) -> Features {
    let text = |name: &str, limit| root.read_limited(name, limit);
    let each = |pattern: &str| {
        root.glob(pattern)
            .iter()
            .map(|p| {
                (
                    parent_name(p),
                    read(p, 4096).map_or(Value::Null, Value::String),
                )
            })
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
pub fn kernel_tail(root: &Root, lines: usize, width: usize) -> Option<Vec<String>> {
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
