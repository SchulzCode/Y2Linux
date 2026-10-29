//! The compact USB gadget and USB network state (about 1 KiB) that both journals carry: the power
//! journal's rows (`power-journal.jsonl`) and the sleep diagnostics rows (`sleep-diagnostics.jsonl`,
//! also written by the suspend group at its stages). One implementation for both writers.
// SPDX-License-Identifier: GPL-2.0-only
use serde::Serialize;
use serde_json::Value;
use y2p_core::paths::{name, parent_name};
use y2p_core::{
    json::Object,
    root::read,
    suspend_text::{int16, split, splitlines},
    Root,
};

/// The controller status counters a row keeps (null when the driver does not report one).
pub(crate) const USB_SUMMARY: [&str; 21] = [
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

/// Why a row has no USB state: a route field that is not hex (Python's `int(text, 16)` ValueError).
#[derive(Debug)]
pub struct Unreadable(pub String);

/// Which of the USB services run (`/proc/*/comm`).
#[derive(Serialize)]
struct Processes {
    umtprd: bool,
    dropbear: bool,
}

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

/// `key=value` pairs of a status text, as the `keys` in this order (null where missing).
pub(crate) fn key_values(text: Option<&str>, keys: &[&str]) -> Value {
    let found = y2p_core::power_text::key_values(text);
    Value::Object(
        keys.iter()
            .map(|k| {
                let value = found.iter().find(|(key, _)| key == k);
                (
                    (*k).to_owned(),
                    value.map_or(Value::Null, |(_, v)| Value::String(v.clone())),
                )
            })
            .collect(),
    )
}

/// Local IPv4 addresses of one interface from /proc/net/fib_trie: no subprocess, no ioctl.
pub(crate) fn ipv4(root: &Root, interface: &str) -> Result<Vec<String>, Unreadable> {
    let routes = root
        .read_limited("/proc/net/route", 65_536)
        .unwrap_or_default();
    let mut networks = Vec::new();
    for line in splitlines(&routes).into_iter().skip(1) {
        let fields = split(line);
        if fields.len() >= 8 && fields[0] == interface {
            let hex = |text: &str| {
                int16(text).ok_or_else(|| {
                    Unreadable(format!(
                        "invalid literal for int() with base 16: {}",
                        y2p_core::py::repr(text)
                    ))
                })
            };
            networks.push((hex(fields[1])?, hex(fields[7])?));
        }
    }
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
                    let octet: i64 = octet.parse().map_err(|_| Unreadable("octet".into()))?;
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
        previous = match y2p_core::power_text::trie_address(text) {
            Some(address) => Some(address),
            None if text.contains('/') => previous,
            None => None,
        };
    }
    found.sort();
    Ok(found)
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

/// The USB state now; `Unreadable` when the routing table cannot be read as Python read it.
pub fn usb_net(root: &Root) -> Result<UsbNet, Unreadable> {
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
        .map(|p| {
            (
                name(p),
                read(&p.join("state"), 64).map_or(Value::Null, Value::String),
            )
        })
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
        // As the file holds it (the platform writes it sorted).
        transfer: root
            .json("/run/y2/usb-transfer.json")
            .unwrap_or(Value::Null),
        processes: processes(root),
    })
}
