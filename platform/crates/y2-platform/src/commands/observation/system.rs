//! The `system` section: versions, boot history and stages, the previous boot's evidence and reset
//! cause, watchdogs, RTC, time, SSH, USB (link, charging, network, MTP), the update journal and the
//! radio firmware.
// SPDX-License-Identifier: GPL-2.0-only
use super::util::{
    self, findall2, findall_map, get_eq, int, name, read, read_limited, record_fields, splitlines,
    Counted,
};
use crate::commands::devices::ssh;
use crate::{regex, reply::Failure};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use y2p_contract::BootStageLine;
use y2p_core::{
    command,
    json::{Object, ObjectExt},
    Root,
};

pub const RESET_STATUS: &str = "/sys/firmware/y2_pm/reset_status";

pub(crate) use crate::commands::boot::{reset_cause_or as reset_cause, ResetCause};

/// The previous boot's evidence (boot-evidence's record) as the snapshot shows it: its findings
/// and the log tails' digests, never the private lines.
pub(crate) fn evidence_status(root: &Root) -> EvidenceStatus {
    let path = "/data/system/platform/previous-boot-evidence.json";
    let Some(record) = super::evidence::read_record(root) else {
        let reason = if root.path(path).exists() {
            "invalid_previous_boot_evidence"
        } else {
            "no_previous_boot_record"
        };
        return EvidenceStatus::Unavailable {
            state: "Unavailable",
            reason,
        };
    };
    EvidenceStatus::Observed(Box::new(EvidenceSummary {
        log_tails: record
            .logs
            .iter()
            .map(|log| LogDigest {
                name: log.name.clone(),
                tail_sha256: log.tail_sha256.clone(),
                source_bytes_at_capture: log.source_bytes_at_capture,
            })
            .collect(),
        private_record: path,
        captured_on_boot_id: record.captured_on_boot_id,
        previous_boot_id: record.previous_boot_id,
        previous_last_stage: record.previous_last_stage,
        previous_orderly_shutdown: record.previous_orderly_shutdown,
        reset_cause: record.reset_cause,
        reset_cause_reason: record.reset_cause_reason,
        reset_status_raw: record.reset_status_raw,
        kernel_panic_retention_guaranteed: record.kernel_panic_retention_guaranteed,
        retained: record.retained,
        pmic: record.pmic,
        loader: record.loader,
    }))
}

/// The previous boot's evidence as the snapshot shows it, or why there is none.
#[derive(Serialize)]
#[serde(untagged)]
pub enum EvidenceStatus {
    Observed(Box<EvidenceSummary>),
    Unavailable {
        state: &'static str,
        reason: &'static str,
    },
}

/// The evidence record without the private log lines.
#[derive(Serialize)]
pub struct EvidenceSummary {
    captured_on_boot_id: String,
    previous_boot_id: Option<String>,
    previous_last_stage: Option<String>,
    previous_orderly_shutdown: Option<bool>,
    reset_cause: Option<String>,
    reset_cause_reason: Option<String>,
    reset_status_raw: Option<String>,
    kernel_panic_retention_guaranteed: bool,
    private_record: &'static str,
    retained: super::evidence::Retained,
    pmic: super::evidence::Pmic,
    loader: super::evidence::Loader,
    log_tails: Vec<LogDigest>,
}

/// A log tail's digest (never its lines).
#[derive(Serialize)]
struct LogDigest {
    name: String,
    tail_sha256: String,
    source_bytes_at_capture: u64,
}

/// Whether Y2Media is mounted as recorded: the claim's mount is still the only, live mount at
/// /media/internal.
pub fn internal_ready(root: &Root) -> bool {
    crate::commands::storage::internal::ready(root)
}

/// `/run/y2/radio-firmware.json`: the last firmware install's answer (service group).
#[derive(Deserialize, Default)]
#[serde(default)]
struct FirmwareInstall {
    state: Value,
    reason: Value,
}

/// The radio firmware: Ready (every file there at its size), Installing or Failed (the install's
/// answer), or Missing (and where system.img goes).
fn radiofw_state(root: &Root) -> Value {
    let install: FirmwareInstall = record_fields(root, "/run/y2/radio-firmware.json");
    let target = root.path("/data/firmware/mt6582");
    // Cheap check for status polling: every file there with its size (the service's own list).
    use crate::commands::service::radiofw::{DEFAULTS, FILES};
    let present = FILES.iter().chain(&DEFAULTS).all(|(n, size, _)| {
        std::fs::symlink_metadata(target.join(n))
            .is_ok_and(|m| m.len() == *size && m.file_type().is_file())
    });
    if present {
        return json!({"state": "Ready"});
    }
    if matches!(install.state.as_str(), Some("Installing" | "Failed")) {
        return json!({"state": install.state, "reason": install.reason});
    }
    let mut missing = json!({"state": "Missing", "folder": "Firmware", "file": "system.img",
                             "location": if internal_ready(root) { "internal" } else { "drive" }});
    // The responder refuses a copy that does not fit before writing anything (mtp-safe space()), so
    // nothing is left behind; only say why (#39).
    // ENOSPC
    if get_eq(
        &root.json_object("/run/y2/mtp-events.json"),
        "error",
        &json!(28),
    ) {
        missing["reason"] = json!("no_space");
    }
    missing
}

/// The kernel marks the detected BC1.1 class in brackets: 'Unknown [SDP] DCP CDP'.
fn source_type(text: Option<&str>) -> Option<String> {
    regex!(r"\[(\w+)\]")
        .captures(text.unwrap_or_default())
        .map(|c| c[1].to_owned())
}

/// The charger's `charging_state` fields the USB diagnostics show, each with its pattern (compiled
/// once).
fn charge_field(state: &str, name: &str) -> Result<Option<i64>, Failure> {
    let pattern = match name {
        "stock_request_ua" => regex!(r"\bstock_request_ua=(-?[0-9]+)"),
        "class_ceiling_ua" => regex!(r"\bclass_ceiling_ua=(-?[0-9]+)"),
        "thermal_level" => regex!(r"\bthermal_level=(-?[0-9]+)"),
        "sense_current_est_ua" => regex!(r"\bsense_current_est_ua=(-?[0-9]+)"),
        _ => return Ok(None),
    };
    match pattern.captures(state) {
        Some(c) => Ok(Some(util::int_or_raise(&c[1])?)),
        None => Ok(None),
    }
}

/// Read-only USB platform diagnostics: negotiated speed, link lifecycle, charging source/ceiling,
/// USB network, MTP storage roots and OUT-path counters.
#[derive(Serialize)]
pub struct UsbDiagnostics {
    negotiated_speed: Option<String>,
    functions: Vec<String>,
    link: Counted,
    charging: Charging,
    network: UsbNetwork,
    mtp_roots: Object,
    out_path: Counted,
}

/// The charger as the USB diagnostics show it. Three different currents: the stock request for the
/// detected source, what the PMIC selector is programmed to, and an unvalidated charger-path estimate.
#[derive(Serialize)]
struct Charging {
    source_type: Option<String>,
    host_allowance_ua: Option<i64>,
    programmed_charge_ua: Option<i64>,
    owner_charge_ceiling_ua: Option<i64>,
    stock_request_ua: Option<i64>,
    class_ceiling_ua: Option<i64>,
    thermal_level: Option<i64>,
    sense_current_est_ua: Option<i64>,
    sense_current_basis: &'static str,
    state: Option<String>,
    usb_online: Option<String>,
}

/// The USB network interface (usb0).
#[derive(Serialize)]
struct UsbNetwork {
    operstate: Option<String>,
    address: Option<String>,
    /// The first IPv4 address with its prefix; null when not read.
    ipv4: Option<String>,
}

fn usb_diagnostics(root: &Root, udcs: &[Udc], link: Counted) -> Result<UsbDiagnostics, Failure> {
    let base = "/sys/class/power_supply/";
    let negotiated_speed = udcs
        .iter()
        .filter_map(|u| u.speed.clone())
        .find(|speed| !speed.is_empty());
    let mut functions: Vec<String> = root
        .glob("/sys/kernel/config/usb_gadget/y2/configs/c.1/*")
        .iter()
        .filter(|p| {
            p.symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink())
        })
        .map(|p| name(p))
        .collect();
    functions.sort();
    let mut out_path = Counted::new();
    // One y2-usb device: its OUT-path counters (the first line).
    if let [source] = &root.glob("/sys/bus/platform/drivers/y2-usb/*/rxdiag")[..] {
        let all = read_limited(source, 4096).unwrap_or_default();
        let first = splitlines(&all).first().copied().unwrap_or("");
        for (key, value) in findall2(regex!(r"\b([a-z_]+)=([0-9]+)\b"), first) {
            out_path.insert(key, int(&value));
        }
    }
    // `ip -4 -o addr show dev usb0`'s first `inet <address>/<prefix>`; on the player read from
    // rtnetlink (connectivity::direct), below a fixture root the recorded command as before.
    let ipv4 = if root.is_real() {
        let answer = crate::commands::connectivity::direct::ipv4_addresses(root, "usb0", 0.5);
        answer.ok.then(|| {
            let parsed: Value =
                serde_json::from_str(answer.output.as_deref().unwrap_or("[]")).unwrap_or_default();
            let first = &parsed[0]["addr_info"][0];
            match (first["local"].as_str(), first["prefixlen"].as_u64()) {
                (Some(local), Some(prefix)) => Some(format!("{local}/{prefix}")),
                _ => None,
            }
        })
    } else {
        let answer = command::run(
            root,
            &["/sbin/ip", "-4", "-o", "addr", "show", "dev", "usb0"],
            0.5,
        );
        answer.ok.then(|| {
            regex!(r"inet (\S+)")
                .captures(answer.output.as_deref().unwrap_or_default())
                .map(|c| c[1].to_owned())
        })
    }
    .flatten();
    let charging_state = root
        .read(&format!("{base}BAT0/charging_state"))
        .unwrap_or_default();
    let charging = Charging {
        source_type: source_type(
            read(&root.path(&format!("{base}y2-usb-presence/usb_type"))).as_deref(),
        ),
        host_allowance_ua: root.integer(&format!("{base}y2-usb-input/current_max")),
        programmed_charge_ua: root.integer(&format!("{base}BAT0/constant_charge_current")),
        owner_charge_ceiling_ua: root.integer(&format!("{base}BAT0/constant_charge_current_max")),
        stock_request_ua: charge_field(&charging_state, "stock_request_ua")?,
        class_ceiling_ua: charge_field(&charging_state, "class_ceiling_ua")?,
        thermal_level: charge_field(&charging_state, "thermal_level")?,
        sense_current_est_ua: charge_field(&charging_state, "sense_current_est_ua")?,
        sense_current_basis: "bsp-200mohm-unvalidated-charger-path-estimate",
        state: read(&root.path(&format!("{base}BAT0/status"))),
        usb_online: read(&root.path(&format!("{base}y2-usb-presence/online"))),
    };
    Ok(UsbDiagnostics {
        negotiated_speed,
        functions,
        link,
        charging,
        network: UsbNetwork {
            operstate: read(&root.path("/sys/class/net/usb0/operstate")),
            address: read(&root.path("/sys/class/net/usb0/address")),
            ipv4,
        },
        mtp_roots: root.json_object("/run/y2/mtp-roots.json"),
        out_path,
    })
}

/// The time section: `y2-platform time`'s record.
fn time_status(root: &Root) -> Result<Value, Failure> {
    Ok(Value::Object(super::super::time::record(root)))
}

/// The `system` section.
pub fn system(root: &Root) -> Result<System, Failure> {
    let clock = time_status(root)?;
    // The SSH supervisor's record while it is renewed (every few seconds) this boot.
    let mut ssh = root.json_object("/run/y2/ssh.json");
    let published = ssh::Published::of(&ssh);
    if !published.is_this_boot(root) || !published.fresh(root, 10.0) {
        ssh = Object::new()
            .with("state", "Unavailable")
            .with("reason", "ssh_record_stale_or_absent");
    }
    let trail = root.read("/run/y2/boot-stages.jsonl").unwrap_or_default();
    let lines = splitlines(&trail);
    let stages: Vec<BootStageLine> = lines[lines.len().saturating_sub(32)..]
        .iter()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let udcs: Vec<Udc> = root
        .glob("/sys/class/udc/*")
        .iter()
        .map(|p| Udc {
            name: name(p),
            state: read(&p.join("state")),
            speed: read(&p.join("current_speed")),
        })
        .collect();
    let rtcs = root
        .glob("/sys/class/rtc/rtc*")
        .iter()
        .map(|p| Rtc {
            name: read(&p.join("name")),
            date: read(&p.join("date")),
            time: read(&p.join("time")),
            since_epoch: read(&p.join("since_epoch")),
            wakealarm: read(&p.join("wakealarm")),
        })
        .collect();
    let sources = root.glob("/sys/bus/platform/drivers/y2-usb/*/status");
    let (dma, link) = if let [source] = &sources[..] {
        let status = read_limited(source, 8192).unwrap_or_default();
        let mut dma = Dma {
            transfer: None,
            counters: Counted::new(),
        };
        for (key, value) in findall_map(
            regex!(r"\b(transfer|dma_[a-z_]+|pio_[a-z_]+|irq_[a-z_]+)=([a-z_]+|-?[0-9]+)\b"),
            &status,
        ) {
            if key == "transfer" {
                dma.transfer = Some(value);
            } else {
                dma.counters.insert(key, int(&value));
            }
        }
        let mut link = Counted::new();
        for (key, value) in findall_map(
            regex!(
                r"\b(detached|reconnect_retries|reconnect_failures|monitor_failures|monitor_transients|runtime_held)=([0-9]+)\b"
            ),
            &status,
        ) {
            link.insert(key, int(&value));
        }
        (Some(dma), link)
    } else {
        (None, Counted::new())
    };
    let diagnostics = usb_diagnostics(root, &udcs, link)?;
    let usb = Usb {
        mode: "peripheral",
        udcs,
        dma,
        file_transfer: root.json_object("/run/y2/usb-transfer.json"),
        diagnostics,
    };
    let uptime = root
        .read("/proc/uptime")
        .and_then(|u| u.split(' ').next().map(str::to_owned))
        .filter(|u| !u.is_empty());
    let watchdogs = root
        .glob("/sys/class/watchdog/watchdog*")
        .iter()
        .map(|p| Watchdog {
            identity: read(&p.join("identity")),
            state: read(&p.join("state")),
            bootstatus: read(&p.join("bootstatus")),
            status: read(&p.join("status")),
            timeout: read(&p.join("timeout")),
        })
        .collect();
    let pstore: Vec<String> = root
        .glob("/sys/fs/pstore/*")
        .iter()
        .map(|p| name(p))
        .collect();
    let crng = clock.get("entropy_ready").cloned().unwrap_or(Value::Null);
    Ok(System {
        uptime_seconds: uptime,
        versions: root.json("/etc/y2linux/versions.json"),
        boot_history: root.json("/data/system/platform/boot.json"),
        previous_boot_evidence: evidence_status(root),
        application_readiness: root.json("/run/y2/application-ready.json"),
        boot_stages: stages,
        reset: reset_cause(
            root.read(RESET_STATUS).as_deref(),
            "no_qualified_retained_register",
        ),
        kernel_taint: root.integer("/proc/sys/kernel/tainted"),
        kernel_warning_count: None,
        kernel_warning_reason: "no_dedicated_counter; retain_bounded_owner_dmesg_capture",
        radio_core: root.read("/sys/devices/platform/18070000.connectivity/status"),
        watchdogs,
        pstore_files: pstore,
        rtc: rtcs,
        time: clock,
        ssh,
        entropy_available_bits: root.integer("/proc/sys/kernel/random/entropy_avail"),
        crng_ready: crng,
        usb,
        update: Value::Object(crate::commands::service::update::journal_status(root)),
        radio_firmware: radiofw_state(root),
        reborn_supervisor: root.json("/data/reborn/logs/supervisor-last.json"),
    })
}

/// The `system` section, in this key order (the reset cause's fields where it is flattened).
#[derive(Serialize)]
pub struct System {
    uptime_seconds: Option<String>,
    versions: Option<Value>,
    boot_history: Option<Value>,
    previous_boot_evidence: EvidenceStatus,
    application_readiness: Option<Value>,
    boot_stages: Vec<BootStageLine>,
    #[serde(flatten)]
    reset: ResetCause,
    kernel_taint: Option<i64>,
    kernel_warning_count: Option<i64>,
    kernel_warning_reason: &'static str,
    radio_core: Option<String>,
    watchdogs: Vec<Watchdog>,
    pstore_files: Vec<String>,
    pub rtc: Vec<Rtc>,
    time: Value,
    ssh: Object,
    entropy_available_bits: Option<i64>,
    crng_ready: Value,
    pub usb: Usb,
    pub update: Value,
    radio_firmware: Value,
    reborn_supervisor: Option<Value>,
}

#[derive(Serialize)]
pub struct Watchdog {
    identity: Option<String>,
    state: Option<String>,
    bootstatus: Option<String>,
    status: Option<String>,
    timeout: Option<String>,
}

#[derive(Serialize)]
pub struct Rtc {
    name: Option<String>,
    date: Option<String>,
    time: Option<String>,
    since_epoch: Option<String>,
    wakealarm: Option<String>,
}

/// The USB controller: its UDCs, DMA counters and file-transfer record, then the diagnostics.
#[derive(Serialize)]
pub struct Usb {
    mode: &'static str,
    pub udcs: Vec<Udc>,
    dma: Option<Dma>,
    file_transfer: Map<String, Value>,
    #[serde(flatten)]
    diagnostics: UsbDiagnostics,
}

/// One USB device controller.
#[derive(Serialize)]
pub struct Udc {
    name: String,
    state: Option<String>,
    speed: Option<String>,
}

/// The y2-usb driver's transfer mode and DMA/PIO/IRQ counters.
#[derive(Serialize)]
struct Dma {
    #[serde(skip_serializing_if = "Option::is_none")]
    transfer: Option<String>,
    #[serde(flatten)]
    counters: Counted,
}
