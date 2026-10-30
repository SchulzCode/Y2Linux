//! The `system` section: versions, boot history and stages, the previous boot's evidence and reset
//! cause, watchdogs, RTC, time, SSH, USB (link, charging, network, MTP), the update journal and the
//! radio firmware.
// SPDX-License-Identifier: GPL-2.0-only
use super::util::{
    self, findall2, findall_map, get_eq, name, number, read, read_limited, record_fields,
    splitlines, text,
};
use crate::commands::devices::ssh;
use crate::{regex, reply::Failure};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use y2p_core::{
    command,
    json::{Object, ObjectExt},
    py::{truthy, PyValue},
    Root,
};

pub const RESET_STATUS: &str = "/sys/firmware/y2_pm/reset_status";

/// The reset cause: the MT6582 RGU WDT_STATUS the watchdog driver read at
/// probe, before its first write. Zero, unknown bits or an absent file never prove a power-on reset.
pub fn reset_cause(text: Option<&str>, missing: &str) -> Map<String, Value> {
    let fields = findall_map(regex!(r"(\w+)=(\S+)"), text.unwrap_or_default());
    let mut result = Map::new();
    if fields.get("valid").map(String::as_str) != Some("1") {
        result.insert("reset_cause".into(), Value::Null);
        result.insert("reset_cause_reason".into(), json!(missing));
        result.insert("reset_status_raw".into(), Value::Null);
        return result;
    }
    let cause = fields.get("cause").map(String::as_str).unwrap_or_default();
    let known = [
        "watchdog_timeout",
        "software_reset",
        "watchdog_irq",
        "debug_reset",
        "spm_watchdog",
    ]
    .contains(&cause);
    result.insert(
        "reset_cause".into(),
        if known { json!(cause) } else { Value::Null },
    );
    result.insert(
        "reset_cause_reason".into(),
        json!(if known {
            "rgu_wdt_status_at_probe".to_owned()
        } else {
            format!(
                "rgu_status_{}",
                if cause.is_empty() {
                    "unreadable"
                } else {
                    cause
                }
            )
        }),
    );
    result.insert("reset_status_raw".into(), json!(fields.get("raw")));
    result
}

/// The previous boot's evidence (boot-evidence's record) as the snapshot shows it: its findings
/// and the log tails' digests, never the private lines.
pub(crate) fn evidence_status(root: &Root) -> Result<Value, Failure> {
    let value = root.json_object("/data/system/platform/previous-boot-evidence.json");
    if value.is_empty() {
        return Ok(json!({"state": "Unavailable", "reason": "no_previous_boot_record"}));
    }
    let get = |k: &str| value.get(k).cloned().unwrap_or(Value::Null);
    let mut result = Map::new();
    for key in [
        "captured_on_boot_id",
        "previous_boot_id",
        "previous_last_stage",
        "previous_orderly_shutdown",
        "reset_cause",
        "reset_cause_reason",
        "reset_status_raw",
        "kernel_panic_retention_guaranteed",
    ] {
        result.insert(key.into(), get(key));
    }
    result.insert(
        "private_record".into(),
        json!("/data/system/platform/previous-boot-evidence.json"),
    );
    result.insert("retained".into(), get("retained"));
    result.insert("pmic".into(), get("pmic"));
    result.insert("loader".into(), get("loader"));
    // The logs are a list of objects (boot-evidence writes them; an empty object or text holds none
    // either); anything else is a broken record.
    let invalid = || Failure::value("invalid_previous_boot_evidence");
    let logs = match value.get("logs") {
        None => Vec::new(),
        Some(other) => y2p_core::py::iterate(other).ok_or_else(invalid)?,
    };
    let mut tails = Vec::new();
    for log in logs {
        let Value::Object(log) = log else {
            return Err(invalid());
        };
        tails.push(
            json!({"name": log.get("name"), "tail_sha256": log.get("tail_sha256"),
                          "source_bytes_at_capture": log.get("source_bytes_at_capture")}),
        );
    }
    result.insert("log_tails".into(), Value::Array(tails));
    Ok(Value::Object(result))
}

/// This boot's internal storage setup (state, files, bytes, …), else Unknown.
pub fn setup_status(root: &Root) -> Value {
    match root.json("/run/y2/internal-media-setup.json") {
        Some(Value::Object(value)) if get_eq(&value, "boot_id", &Value::from(root.boot_id())) => {
            let picked: Map<String, Value> =
                ["state", "reason", "files", "bytes", "serial", "kept"]
                    .iter()
                    .filter_map(|k| value.get(*k).map(|v| (k.to_string(), v.clone())))
                    .collect();
            Value::Object(picked)
        }
        _ => json!({"state": "Unknown"}),
    }
}

/// `/run/y2/internal-media.json`: the Y2Media mount the platform made (storage group).
#[derive(Deserialize, Default)]
#[serde(default)]
struct InternalClaim {
    state: Value,
    boot_id: Value,
    mount_id: Value,
    device_id: Value,
}

/// Whether Y2Media is mounted as recorded: the claim's mount is still the only, live mount at
/// /media/internal.
pub fn internal_ready(root: &Root) -> bool {
    let claim: InternalClaim = record_fields(root, "/run/y2/internal-media.json");
    let current = y2p_core::mounts::mounts_at(root, "/media/internal");
    let [mount] = &current[..] else {
        return false;
    };
    claim.state.as_str() == Some("Ready")
        && claim.boot_id.py_eq(&Value::from(root.boot_id()))
        && claim.mount_id.py_eq(&json!(mount.mount_id))
        && claim.device_id.py_eq(&json!(mount.device_id))
}

/// The update download's record (download.json), with a download or check whose process is gone shown as
/// interrupted. A user's player (8.17) said "Downloading Update… Keep Wi-Fi on." for a day: the
/// record a download writes first stays `Staging` when the download dies without reaching its own
/// handler (power loss, a kill), and that page offered neither Cancel nor Check for Updates; a
/// restart kept it (it is on /data).
fn download_record(root: &Root) -> Value {
    let record = root.json("/data/updates/download.json");
    let in_progress = record
        .as_ref()
        .and_then(|r| r.get("state"))
        .and_then(Value::as_str)
        .is_some_and(|state| matches!(state, "Staging" | "Checking"));
    // Only a download in progress needs its process: the lock's live holders.
    if in_progress
        && !std::fs::metadata(root.path("/data/updates/.stage.lock"))
            .is_ok_and(|meta| !y2p_core::lock::flock_pids(root, &meta).is_empty())
    {
        return json!({"schema": 1, "state": "Failed", "failure": "download_interrupted"});
    }
    record.unwrap_or(Value::Null)
}

/// The update journal (state.json) with the download, the last check and the channel; Idle on a
/// data volume that has none yet.
pub fn update_status(root: &Root) -> Value {
    let path = root.path("/data/updates/state.json");
    let mut value = root.json("/data/updates/state.json");
    if value.is_none() && !path.exists() && path.symlink_metadata().is_err() {
        let ready = root.read("/data/.y2data-schema").as_deref() == Some("1")
            && root.path("/data/updates").is_dir();
        value = Some(
            json!({"schema": 1, "state": if ready { "Idle" } else { "Unavailable" },
                            "failure": if ready { Value::Null } else { json!("data_state_unavailable") }}),
        );
    }
    // The native core writes journal schema 2 since OTA-35 (BOOTIMG + root).
    let mut value = match value {
        Some(Value::Object(map))
            if get_eq(&map, "schema", &json!(1)) || get_eq(&map, "schema", &json!(2)) =>
        {
            map
        }
        _ => return json!({"schema": 1, "state": "Failed", "failure": "journal_unreadable"}),
    };
    value.insert("download".into(), download_record(root));
    value.insert(
        "last_check".into(),
        root.json("/data/updates/last-check.json")
            .unwrap_or(Value::Null),
    );
    // The owner's channel on /data, or the shipped one.
    let name = root
        .json_object("/data/updates/channel.json")
        .shift_remove("name")
        .or_else(|| {
            root.json_object("/etc/y2linux/update-channel.json")
                .shift_remove("name")
        })
        .unwrap_or(Value::Null);
    value.insert("channel".into(), name);
    value.insert("full_system".into(), json!(true));
    value.insert("physical_qualification".into(), json!(false));
    Value::Object(value)
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
fn source_type(text: Option<&str>) -> Value {
    regex!(r"\[(\w+)\]")
        .captures(text.unwrap_or_default())
        .map_or(Value::Null, |c| json!(&c[1]))
}

/// The charger's `charging_state` fields the USB diagnostics show, each with its pattern (compiled
/// once).
fn charge_field(state: &str, name: &str) -> Result<Value, Failure> {
    let pattern = match name {
        "stock_request_ua" => regex!(r"\bstock_request_ua=(-?\d+)"),
        "class_ceiling_ua" => regex!(r"\bclass_ceiling_ua=(-?\d+)"),
        "thermal_level" => regex!(r"\bthermal_level=(-?\d+)"),
        "sense_current_est_ua" => regex!(r"\bsense_current_est_ua=(-?\d+)"),
        _ => return Ok(Value::Null),
    };
    match pattern.captures(state) {
        Some(c) => Ok(json!(util::int_or_raise(&c[1])?)),
        None => Ok(Value::Null),
    }
}

/// Read-only USB platform diagnostics: negotiated speed, link lifecycle, charging source/ceiling,
/// USB network, MTP storage roots and OUT-path counters.
fn usb_diagnostics(
    root: &Root,
    udcs: &[Value],
    link: Value,
) -> Result<Map<String, Value>, Failure> {
    let base = "/sys/class/power_supply/";
    let speed = udcs
        .iter()
        .map(|u| u.get("speed").cloned().unwrap_or(Value::Null))
        .find(truthy)
        .unwrap_or(Value::Null);
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
    let mut rx = Map::new();
    // One y2-usb device: its OUT-path counters (the first line).
    if let [source] = &root.glob("/sys/bus/platform/drivers/y2-usb/*/rxdiag")[..] {
        let all = read_limited(source, 4096).unwrap_or_default();
        let first = splitlines(&all).first().copied().unwrap_or("");
        for (key, value) in findall2(regex!(r"\b([a-z_]+)=(\d+)\b"), first) {
            rx.insert(key, number(Some(&value)));
        }
    }
    let mut network = Map::new();
    network.insert(
        "operstate".into(),
        text(read(&root.path("/sys/class/net/usb0/operstate"))),
    );
    network.insert(
        "address".into(),
        text(read(&root.path("/sys/class/net/usb0/address"))),
    );
    // `ip -4 -o addr show dev usb0`'s first `inet <address>/<prefix>`; on the player read from
    // rtnetlink (connectivity::direct), below a fixture root the recorded command as before.
    if root.is_real() {
        let answer = crate::commands::connectivity::direct::ipv4_addresses(root, "usb0", 0.5);
        if answer.ok {
            let parsed: Value =
                serde_json::from_str(answer.output.as_deref().unwrap_or("[]")).unwrap_or_default();
            let first = parsed[0]["addr_info"][0].clone();
            network.insert(
                "ipv4".into(),
                match (first["local"].as_str(), first["prefixlen"].as_u64()) {
                    (Some(local), Some(prefix)) => json!(format!("{local}/{prefix}")),
                    _ => Value::Null,
                },
            );
        }
    } else {
        let answer = command::run(
            root,
            &["/sbin/ip", "-4", "-o", "addr", "show", "dev", "usb0"],
            0.5,
        );
        if answer.ok {
            network.insert(
                "ipv4".into(),
                regex!(r"inet (\S+)")
                    .captures(answer.output.as_deref().unwrap_or_default())
                    .map_or(Value::Null, |c| json!(&c[1])),
            );
        }
    }
    let charging_state = root
        .read(&format!("{base}BAT0/charging_state"))
        .unwrap_or_default();
    let mut charging = Map::new();
    charging.insert(
        "source_type".into(),
        source_type(read(&root.path(&format!("{base}y2-usb-presence/usb_type"))).as_deref()),
    );
    charging.insert(
        "host_allowance_ua".into(),
        json!(root.integer(&format!("{base}y2-usb-input/current_max"))),
    );
    // Three different quantities: the stock request for the detected source, what the PMIC selector
    // is programmed to, and an unvalidated charger-path estimate.
    charging.insert(
        "programmed_charge_ua".into(),
        json!(root.integer(&format!("{base}BAT0/constant_charge_current"))),
    );
    charging.insert(
        "owner_charge_ceiling_ua".into(),
        json!(root.integer(&format!("{base}BAT0/constant_charge_current_max"))),
    );
    for key in [
        "stock_request_ua",
        "class_ceiling_ua",
        "thermal_level",
        "sense_current_est_ua",
    ] {
        charging.insert(key.into(), charge_field(&charging_state, key)?);
    }
    charging.insert(
        "sense_current_basis".into(),
        json!("bsp-200mohm-unvalidated-charger-path-estimate"),
    );
    charging.insert(
        "state".into(),
        text(read(&root.path(&format!("{base}BAT0/status")))),
    );
    charging.insert(
        "usb_online".into(),
        text(read(&root.path(&format!("{base}y2-usb-presence/online")))),
    );
    let mut result = Map::new();
    result.insert("negotiated_speed".into(), speed);
    result.insert("functions".into(), json!(functions));
    result.insert("link".into(), link);
    result.insert("charging".into(), Value::Object(charging));
    result.insert("network".into(), Value::Object(network));
    result.insert(
        "mtp_roots".into(),
        Value::Object(root.json_object("/run/y2/mtp-roots.json")),
    );
    result.insert("out_path".into(), Value::Object(rx));
    Ok(result)
}

/// The time section: `y2-platform time`'s record.
fn time_status(root: &Root) -> Result<Value, Failure> {
    Ok(Value::Object(super::super::time::record(root)))
}

/// The `system` section.
pub fn system(root: &Root) -> Result<Value, Failure> {
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
    let mut stages = Vec::new();
    for line in &lines[lines.len().saturating_sub(32)..] {
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(line) {
            stages.push(Value::Object(map));
        }
    }
    let udcs: Vec<Value> = root
        .glob("/sys/class/udc/*")
        .iter()
        .map(|p| json!({"name": name(p), "state": read(&p.join("state")), "speed": read(&p.join("current_speed"))}))
        .collect();
    let rtcs: Vec<Value> = root
        .glob("/sys/class/rtc/rtc*")
        .iter()
        .map(|p| {
            let fields: Map<String, Value> = ["name", "date", "time", "since_epoch", "wakealarm"]
                .iter()
                .map(|f| (f.to_string(), text(read(&p.join(f)))))
                .collect();
            Value::Object(fields)
        })
        .collect();
    let sources = root.glob("/sys/bus/platform/drivers/y2-usb/*/status");
    let (dma, link) = if let [source] = &sources[..] {
        let status = read_limited(source, 8192).unwrap_or_default();
        let mut dma = Map::new();
        for (key, value) in findall_map(
            regex!(r"\b(transfer|dma_[a-z_]+|pio_[a-z_]+|irq_[a-z_]+)=([a-z_]+|-?\d+)\b"),
            &status,
        ) {
            let typed = if key == "transfer" {
                json!(value)
            } else {
                number(Some(&value))
            };
            dma.insert(key, typed);
        }
        let mut link = Map::new();
        for (key, value) in findall_map(
            regex!(
                r"\b(detached|reconnect_retries|reconnect_failures|monitor_failures|monitor_transients|runtime_held)=(\d+)\b"
            ),
            &status,
        ) {
            link.insert(key, number(Some(&value)));
        }
        (Value::Object(dma), Value::Object(link))
    } else {
        (Value::Null, json!({}))
    };
    let usb_extra = usb_diagnostics(root, &udcs, link)?;
    let mut usb = Map::new();
    usb.insert("mode".into(), json!("peripheral"));
    usb.insert("udcs".into(), Value::Array(udcs));
    usb.insert("dma".into(), dma);
    usb.insert(
        "file_transfer".into(),
        Value::Object(root.json_object("/run/y2/usb-transfer.json")),
    );
    usb.extend(usb_extra);
    let uptime = root
        .read("/proc/uptime")
        .and_then(|u| u.split(' ').next().map(str::to_owned))
        .filter(|u| !u.is_empty());
    let watchdogs: Vec<Value> = root
        .glob("/sys/class/watchdog/watchdog*")
        .iter()
        .map(|p| {
            let fields: Map<String, Value> =
                ["identity", "state", "bootstatus", "status", "timeout"]
                    .iter()
                    .map(|f| (f.to_string(), text(read(&p.join(f)))))
                    .collect();
            Value::Object(fields)
        })
        .collect();
    let pstore: Vec<String> = root
        .glob("/sys/fs/pstore/*")
        .iter()
        .map(|p| name(p))
        .collect();
    let mut result = Map::new();
    result.insert("uptime_seconds".into(), text(uptime));
    result.insert(
        "versions".into(),
        root.json("/etc/y2linux/versions.json")
            .unwrap_or(Value::Null),
    );
    result.insert(
        "boot_history".into(),
        root.json("/data/system/platform/boot.json")
            .unwrap_or(Value::Null),
    );
    result.insert("previous_boot_evidence".into(), evidence_status(root)?);
    result.insert(
        "application_readiness".into(),
        root.json("/run/y2/application-ready.json")
            .unwrap_or(Value::Null),
    );
    result.insert("boot_stages".into(), Value::Array(stages));
    result.extend(reset_cause(
        root.read(RESET_STATUS).as_deref(),
        "no_qualified_retained_register",
    ));
    result.insert(
        "kernel_taint".into(),
        json!(root.integer("/proc/sys/kernel/tainted")),
    );
    result.insert("kernel_warning_count".into(), Value::Null);
    result.insert(
        "kernel_warning_reason".into(),
        json!("no_dedicated_counter; retain_bounded_owner_dmesg_capture"),
    );
    result.insert(
        "radio_core".into(),
        text(root.read("/sys/devices/platform/18070000.connectivity/status")),
    );
    result.insert("watchdogs".into(), Value::Array(watchdogs));
    result.insert("pstore_files".into(), json!(pstore));
    result.insert("rtc".into(), Value::Array(rtcs));
    let crng = clock.get("entropy_ready").cloned().unwrap_or(Value::Null);
    result.insert("time".into(), clock);
    result.insert("ssh".into(), Value::Object(ssh));
    result.insert(
        "entropy_available_bits".into(),
        json!(root.integer("/proc/sys/kernel/random/entropy_avail")),
    );
    result.insert("crng_ready".into(), crng);
    result.insert("usb".into(), Value::Object(usb));
    result.insert("update".into(), update_status(root));
    result.insert("radio_firmware".into(), radiofw_state(root));
    result.insert(
        "reborn_supervisor".into(),
        root.json("/data/reborn/logs/supervisor-last.json")
            .unwrap_or(Value::Null),
    );
    Ok(Value::Object(result))
}
