//! The `bluetooth` section: what D-Bus shows of the adapter, the selected peer, its playback PCM and
//! encoder (y2-bt-observe), next to the codec settings; observations only, never codec requests.
// SPDX-License-Identifier: GPL-2.0-only
use super::util::{fields, present, record_fields};
use crate::{
    commands::codec::{inventory_codec, requested, validate},
    reply::Failure,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use y2p_core::{
    command,
    lenient::{self, BootId},
    Root,
};

/// Why `normalize` stopped.
enum Stop {
    /// The observation cannot be read (a codec receipt's arguments that are no list): the section
    /// reports no observation.
    Unobserved,
    /// A broken installed record (the codec inventory): the snapshot is refused.
    Refused(Failure),
}

/// Why `codec_settings` has no answer.
enum Settings {
    /// The owner's codec policy is refused (its reason): the settings are shown as Failed.
    Invalid(String),
    Stop(Stop),
}

/// The codec inventory's entry for `name`; an inventory whose codecs are no object refuses.
fn inventory_entry<'a>(
    inventory: &'a Map<String, Value>,
    name: &str,
) -> Result<Option<&'a Map<String, Value>>, Settings> {
    inventory_codec(inventory, name).map_err(|refusal| Settings::Stop(Stop::Refused(refusal)))
}

/// The installed codec inventory (what this build compiled and qualified).
const INVENTORY: &str = "/etc/y2linux/bluetooth-codecs.json";

/// `/run/y2/codec-runtime.json`: the codec arguments BlueALSA was last started with and the
/// settings they carry (connectivity group).
#[derive(Deserialize, Default)]
#[serde(default)]
struct AppliedCodecs {
    /// Missing (no arguments) differs from anything but a list of texts (unobserved).
    #[serde(deserialize_with = "present")]
    arguments: Option<Value>,
    boot_id: BootId,
    settings: Value,
}

/// When `/run/y2/bt-reconnect.json` was written (y2-bt-reconnect, every few seconds).
#[derive(Deserialize, Default)]
#[serde(default)]
struct ReconnectStamp {
    boot_id: BootId,
    #[serde(deserialize_with = "lenient::integer")]
    monotonic_us: Option<i64>,
}

/// The codec settings: the owner's policy, what BlueALSA was started with this boot (its effective
/// settings) and which qualities this build supports.
fn codec_settings(root: &Root, inventory: &Map<String, Value>) -> Result<Value, Settings> {
    let policy = requested(root)
        .and_then(validate)
        .map_err(|error| Settings::Invalid(error.message()))?;
    let applied: AppliedCodecs = record_fields(root, "/run/y2/codec-runtime.json");
    let boot = root.boot_id();
    // This receipt records startup arguments, never endpoint/negotiation success.
    let pid = root.integer("/run/y2/bluealsa.pid");
    let command = match pid {
        Some(pid) if pid > 1 => root.read_limited(&format!("/proc/{pid}/cmdline"), 8192),
        _ => None,
    };
    let argv: Vec<&str> = match command.as_deref().filter(|c| !c.is_empty()) {
        Some(c) => c.split('\0').collect(),
        None => Vec::new(),
    };
    let live = argv
        .first()
        .is_some_and(|a| a.rsplit('/').next() == Some("bluealsad"))
        && {
            let arguments: Vec<&str> = match &applied.arguments {
                None => Vec::new(),
                Some(Value::Array(items)) => items
                    .iter()
                    .map(Value::as_str)
                    .collect::<Option<_>>()
                    .ok_or(Settings::Stop(Stop::Unobserved))?,
                Some(_) => return Err(Settings::Stop(Stop::Unobserved)),
            };
            arguments.iter().all(|a| argv.contains(a))
        };
    let effective = if live
        && applied.boot_id.is(boot.as_deref())
        && boot.as_deref().is_some_and(|b| !b.is_empty())
    {
        applied.settings
    } else {
        Value::Null
    };
    let policy = Value::Object(policy);
    let aac = inventory_entry(inventory, "AAC")?;
    let ldac = inventory_entry(inventory, "LDAC")?;
    let compiled = |codec: Option<&Map<String, Value>>| {
        codec.and_then(|c| c.get("compiled_locally")) == Some(&Value::Bool(true))
    };
    let qualities = |codec: Option<&Map<String, Value>>, list: Value| {
        if compiled(codec) {
            list
        } else {
            json!([])
        }
    };
    Ok(json!({
        "schema": y2p_contract::schema::CODEC_SETTINGS, "requested": policy.clone(),
        "effective": effective.clone(), "effective_scope": "daemon_start_arguments",
        "pending_restart": effective != policy,
        "apply_action": "restart_player",
        "supported": {
            "sbc_quality": ["high", "xq", "xq+"], "sbc_abr": true,
            "aac_quality": qualities(aac, json!(["standard", "high"])),
            "aac_abr": compiled(aac),
            "ldac_quality": qualities(ldac, json!(["mobile", "standard", "high"])),
            "ldac_abr": compiled(ldac),
        },
    }))
}

/// The encoder as observed, kept apart from the peer's configuration and the preference: BlueALSA's
/// statistics count only while the PCM runs (null where not observed or not this codec's).
#[derive(Serialize, Default)]
struct Encoder {
    bitrate_bps: Option<i64>,
    sbc_quality: Option<&'static str>,
    sbc_bitpool: Option<i64>,
    sbc_xq_peer_mode: Option<bool>,
    sbc_abr_enabled: Option<bool>,
    sbc_abr_adjustments: Option<i64>,
    ldac_quality_index: Option<i64>,
    ldac_nominal_choices_kbps: Option<[i64; 3]>,
    ldac_abr_enabled: Option<bool>,
    ldac_abr_adjustments: Option<i64>,
    aac_abr_enabled: Option<bool>,
    aac_abr_level: Option<i64>,
    aac_abr_adjustments: Option<i64>,
    preference: Value,
    encoder_observed: bool,
    bitrate_scope: &'static str,
    ldac_abr_adaptation_observed: bool,
    encoder_errors: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sbc_negotiated_bitpool_range: Option<[i64; 2]>,
}

fn quality(codec: Option<&str>, pcm: &Map<String, Value>, settings: &Value) -> Encoder {
    let config: Vec<i64> = match pcm.get("CodecConfiguration") {
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| v.as_i64().filter(|n| (0..=255).contains(n)))
            .collect::<Option<Vec<i64>>>()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let stats = match pcm.get("EncoderStats") {
        Some(Value::Object(map)) => Some(map),
        _ => None,
    };
    let live = pcm.get("Running") == Some(&Value::Bool(true))
        && stats.is_some_and(|s| s.get("Active").and_then(Value::as_i64) == Some(1));
    let observed = |key: &str, minimum: i64, maximum: i64| -> Option<i64> {
        if !live {
            return None;
        }
        stats
            .and_then(|s| s.get(key))
            .and_then(Value::as_i64)
            .filter(|v| (minimum..=maximum).contains(v))
    };
    const MAX: i64 = (1 << 31) - 1;
    let flag = |enabled: Option<i64>| enabled.map(|e| e != 0);
    let mut encoder = Encoder {
        preference: settings.get("requested").cloned().unwrap_or(Value::Null),
        encoder_observed: live,
        bitrate_scope: "encoder_payload_nominal_not_radio_throughput",
        encoder_errors: observed("Errors", 0, MAX),
        ..Encoder::default()
    };
    match codec {
        Some("SBC") => {
            if let [c0, c1, c2, c3] = config[..] {
                // A2DP SBC: 44.1kHz dual-channel, 16 blocks, 8 subbands, loudness.
                let xq = c0 == 0x24 && c1 == 0x15;
                encoder.sbc_xq_peer_mode = Some(xq && c3 >= 38);
                encoder.sbc_negotiated_bitpool_range = Some([c2, c3]);
                encoder.sbc_bitpool = observed("Bitpool", 2, 250);
                encoder.sbc_quality = encoder.sbc_bitpool.map(|bitpool| {
                    if xq && bitpool == 47 {
                        "xq+"
                    } else if xq && bitpool == 38 {
                        "xq"
                    } else {
                        "standard"
                    }
                });
            }
            // BlueALSA patch 0003: Bitpool above is the live (possibly stepped down) value;
            // adjustments count how often the link forced a change.
            encoder.sbc_abr_enabled = flag(observed("AbrEnabled", 0, 1));
            encoder.sbc_abr_adjustments = observed("AbrAdjustments", 0, MAX);
        }
        Some("LDAC") => {
            encoder.ldac_nominal_choices_kbps = match pcm.get("Rate").and_then(Value::as_i64) {
                Some(44100 | 88200) => Some([303, 606, 909]),
                Some(48000 | 96000) => Some([330, 660, 990]),
                _ => None,
            };
            encoder.ldac_quality_index = observed("QualityIndex", 0, 15);
            let enabled = observed("AbrEnabled", 0, 1);
            encoder.ldac_abr_enabled = flag(enabled);
            encoder.ldac_abr_adjustments = observed("AbrAdjustments", 0, MAX);
            encoder.ldac_abr_adaptation_observed =
                enabled == Some(1) && encoder.ldac_abr_adjustments.unwrap_or(0) > 0;
        }
        Some("AAC") => {
            // BlueALSA patch 0002: CBR rate actually in use, ABR level 0 = full rate (3 = half), and
            // how often the link forced a change.
            encoder.aac_abr_enabled = flag(observed("AbrEnabled", 0, 1));
            encoder.aac_abr_level = observed("QualityIndex", 0, 3);
            encoder.aac_abr_adjustments = observed("AbrAdjustments", 0, MAX);
        }
        _ => {}
    }
    if matches!(codec, Some("SBC" | "LDAC" | "AAC")) {
        encoder.bitrate_bps = observed("BitrateKbps", 1, 2000)
            .filter(|b| *b != 0)
            .map(|b| b * 1000);
    }
    encoder
}

/// The `bluetooth` section (null where not observed; `devices`, `encoder` and `transport_reason`
/// only once the observation got that far).
#[derive(Serialize)]
pub struct Section {
    pub state: &'static str,
    pub reason: &'static str,
    adapter: Option<Adapter>,
    selected_peer: Option<Device>,
    transport: Option<Transport>,
    negotiated_codec: Option<String>,
    pcm: Option<Pcm>,
    remote_advertised: Option<Value>,
    remote_advertised_reason: &'static str,
    runtime_enabled: Option<Vec<String>>,
    mutually_usable: Option<Map<String, Value>>,
    preferred_codec: Option<String>,
    requested_codec: Option<String>,
    actual_bitrate_bps: Option<i64>,
    packet_loss: Option<Value>,
    reconnect_owner: &'static str,
    reconnect: Option<Value>,
    codec_inventory: Option<Value>,
    codec_settings: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    devices: Option<Vec<Device>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    encoder: Option<Encoder>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transport_reason: Option<&'static str>,
}

/// The adapter as BlueZ shows it (D-Bus booleans; null where BlueZ gave another type).
#[derive(Serialize)]
struct Adapter {
    path: String,
    powered: Option<bool>,
    discovering: Option<bool>,
    pairable: Option<bool>,
    discoverable: Option<bool>,
    bluez_owner: Option<String>,
}

/// One BlueZ device.
#[derive(Serialize, Clone)]
struct Device {
    path: String,
    address: Option<String>,
    paired: Option<bool>,
    bonded: Option<bool>,
    trusted: Option<bool>,
    connected: Option<bool>,
    services_resolved: Option<bool>,
}

/// The playback PCM's sample format.
#[derive(Serialize)]
struct Pcm {
    format_code: Option<i64>,
    format: Option<&'static str>,
    valid_bits: Option<u8>,
    physical_bits: Option<u8>,
    rate_hz: Option<i64>,
    channels: Option<i64>,
}

#[derive(Serialize)]
struct Transport {
    object: String,
    bluealsa_owner: Option<String>,
    connection_sequence: Option<i64>,
    running: Option<bool>,
    lifetime_note: &'static str,
}

impl Section {
    fn new(root: &Root, settings: Value, inventory: Option<Value>) -> Section {
        Section {
            state: "Unavailable",
            reason: "dbus_observation_unavailable",
            adapter: None,
            selected_peer: None,
            transport: None,
            negotiated_codec: None,
            pcm: None,
            remote_advertised: None,
            remote_advertised_reason: "API_exposes_mutual_subset_only",
            runtime_enabled: None,
            mutually_usable: None,
            preferred_codec: None,
            requested_codec: None,
            actual_bitrate_bps: None,
            packet_loss: None,
            reconnect_owner: "y2-bt-reconnect",
            reconnect: root.json("/run/y2/bt-reconnect.json"),
            codec_inventory: inventory,
            codec_settings: settings,
            devices: None,
            encoder: None,
            transport_reason: None,
        }
    }

    fn degraded(&mut self, reason: &'static str) {
        (self.state, self.reason) = ("Degraded", reason);
    }
}

fn flag(map: &Map<String, Value>, key: &str) -> Option<bool> {
    map.get(key).and_then(Value::as_bool)
}

fn text(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn integer(map: &Map<String, Value>, key: &str) -> Option<i64> {
    map.get(key).and_then(Value::as_i64)
}

/// The section from y2-bt-observe's answer (`raw`).
fn normalize(root: &Root, raw: &Map<String, Value>) -> Result<Section, Stop> {
    let mut section = settled_base(root)?;
    observed(root, raw, &mut section);
    Ok(section)
}

/// The section before y2-bt-observe's answer: the codec settings, inventory and the reconnect
/// helper's record (shown only while fresh: written this boot within the last 10 s).
fn settled_base(root: &Root) -> Result<Section, Stop> {
    // Read once: the settings check it and the section shows it.
    let inventory = root.json(INVENTORY);
    let none = Map::new();
    let entries = match &inventory {
        Some(Value::Object(map)) => map,
        _ => &none,
    };
    let settings = match codec_settings(root, entries) {
        Ok(settings) => settings,
        Err(Settings::Invalid(reason)) => {
            json!({"schema": y2p_contract::schema::CODEC_SETTINGS, "state": "Failed",
                   "reason": reason, "requested": null, "effective": null,
                   "pending_restart": false, "supported": {}})
        }
        Err(Settings::Stop(stop)) => return Err(stop),
    };
    let mut section = Section::new(root, settings, inventory);
    let fresh = match &section.reconnect {
        Some(Value::Object(record)) => {
            let stamp: ReconnectStamp = fields(record);
            stamp.boot_id.is(root.boot_id().as_deref())
                && stamp.monotonic_us.is_some_and(|us| {
                    let age = root.clock().monotonic() * 1e6 - us as f64;
                    (0.0..10_000_000.0).contains(&age)
                })
        }
        _ => false,
    };
    if !fresh {
        section.reconnect =
            Some(json!({"state": "Unavailable", "reason": "reconnect_record_stale_or_absent"}));
    }
    Ok(section)
}

/// What y2-bt-observe saw, added step by step: the adapter, the runtime (bluealsa and its codecs),
/// the devices and the selected peer's transport. A step that cannot go on has left its state or
/// reason and ends the section there (None).
fn observed(root: &Root, raw: &Map<String, Value>, section: &mut Section) -> Option<()> {
    if raw.get("schema").and_then(Value::as_i64) != Some(1)
        || raw.get("stable_owners") != Some(&Value::Bool(true))
    {
        return None;
    }
    let Some(Value::Object(bluez)) = raw.get("bluez") else {
        return None;
    };
    adapter(raw, bluez, section)?;
    let bluealsa = runtime(raw, section)?;
    let devices = devices(bluez);
    section.devices = Some(devices.iter().take(128).cloned().collect());
    let device = selected_peer(root, &devices, section)?;
    transport(raw, bluealsa, &device, section)
}

/// The one adapter and whether it is powered.
fn adapter(
    raw: &Map<String, Value>,
    bluez: &Map<String, Value>,
    section: &mut Section,
) -> Option<()> {
    let adapters: Vec<(&String, &Map<String, Value>)> = bluez
        .iter()
        .filter_map(|(path, interfaces)| {
            interfaces
                .get("org.bluez.Adapter1")
                .and_then(Value::as_object)
                .map(|a| (path, a))
        })
        .collect();
    let [(path, adapter)] = adapters[..] else {
        section.reason = "no_unique_adapter";
        return None;
    };
    let powered = flag(adapter, "Powered");
    section.adapter = Some(Adapter {
        path: path.clone(),
        powered,
        discovering: flag(adapter, "Discovering"),
        pairable: flag(adapter, "Pairable"),
        discoverable: flag(adapter, "Discoverable"),
        bluez_owner: text(raw, "bluez_owner"),
    });
    if powered != Some(true) {
        section.reason = "radio_off";
        return None;
    }
    Some(())
}

/// bluealsa and its manager: Ready with SBC enabled and the runtime version 5.0.0, else Degraded
/// (Starting while bluealsa is not up). bluealsa's objects for the transport step.
fn runtime<'a>(
    raw: &'a Map<String, Value>,
    section: &mut Section,
) -> Option<&'a Map<String, Value>> {
    let object = |key: &str| match raw.get(key) {
        Some(Value::Object(map)) => Some(map),
        _ => None,
    };
    let owner = text(raw, "bluealsa_owner").filter(|owner| !owner.is_empty());
    let (Some(bluealsa), Some(manager), Some(_)) = (object("bluealsa"), object("manager"), owner)
    else {
        (section.state, section.reason) = ("Starting", "bluealsa_not_ready");
        return None;
    };
    (section.state, section.reason) = ("Ready", "controller_bluez_bluealsa_observed");
    let enabled: Option<Vec<String>> =
        manager
            .get("Codecs")
            .and_then(Value::as_array)
            .map(|codes| {
                codes
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|c| c.to_lowercase().starts_with("a2dp-source:"))
                    .map(|c| c.split_once(':').map_or("", |(_, r)| r).to_owned())
                    .collect()
            });
    if !enabled
        .as_ref()
        .is_some_and(|e| e.iter().any(|c| c == "SBC"))
    {
        section.degraded("a2dp_source_sbc_not_enabled");
    }
    section.runtime_enabled = enabled;
    // The runtime version: `v5.0.0` or `5.0.0` (no version at all reads as none).
    let version = text(manager, "Version");
    if version.as_deref().map(|v| v.trim_start_matches('v')) != Some("5.0.0") {
        section.degraded("bluealsa_runtime_version_mismatch");
    }
    Some(bluealsa)
}

/// BlueZ's devices in y2-bt-observe's order (the object's keys as the output gives them).
fn devices(bluez: &Map<String, Value>) -> Vec<Device> {
    bluez
        .iter()
        .filter_map(|(path, interfaces)| {
            let device = interfaces.get("org.bluez.Device1")?.as_object()?;
            Some(Device {
                path: path.clone(),
                address: text(device, "Address"),
                paired: flag(device, "Paired"),
                bonded: flag(device, "Bonded"),
                trusted: flag(device, "Trusted"),
                connected: flag(device, "Connected"),
                services_resolved: flag(device, "ServicesResolved"),
            })
        })
        .collect()
}

/// The peer audio goes to: the preferred address when one is set, else the one connected device;
/// it must be unique and connected.
fn selected_peer(root: &Root, devices: &[Device], section: &mut Section) -> Option<Device> {
    let preferred = root.read("/data/bluetooth/preferred-audio");
    let selected: Vec<&Device> = match preferred
        .as_deref()
        .filter(|p| crate::regex!(r"\A(?:[A-Fa-f0-9]{2}:){5}[A-Fa-f0-9]{2}\z").is_match(p))
    {
        Some(p) => devices
            .iter()
            .filter(|d| {
                d.address
                    .as_deref()
                    .is_some_and(|a| a.eq_ignore_ascii_case(p))
            })
            .collect(),
        None => devices
            .iter()
            .filter(|d| d.connected == Some(true))
            .collect(),
    };
    let [device] = selected[..] else {
        section.transport_reason = Some("no_unique_selected_peer");
        return None;
    };
    section.selected_peer = Some(device.clone());
    if device.connected != Some(true) {
        section.transport_reason = Some("selected_peer_disconnected");
        return None;
    }
    Some(device.clone())
}

/// The selected peer's one playback PCM: its format, codec, encoder and transport, Degraded when
/// the PCM is not one the player plays.
fn transport(
    raw: &Map<String, Value>,
    bluealsa: &Map<String, Value>,
    device: &Device,
    section: &mut Section,
) -> Option<()> {
    let pcms: Vec<(&String, &Map<String, Value>)> = bluealsa
        .iter()
        .filter_map(|(path, interfaces)| {
            let pcm = interfaces.get("org.bluealsa.PCM1")?.as_object()?;
            (pcm.get("Device").and_then(Value::as_str) == Some(device.path.as_str())
                && pcm.get("Transport").and_then(Value::as_str) == Some("A2DP-source")
                && pcm.get("Mode").and_then(Value::as_str) == Some("sink"))
            .then_some((path, pcm))
        })
        .collect();
    let [(path, pcm)] = pcms[..] else {
        section.transport_reason = Some("no_unique_playback_pcm");
        return None;
    };
    let format_code = integer(pcm, "Format");
    let format = match format_code {
        Some(0x8210) => Some(("S16_LE", 16, 16)),
        Some(0x8418) => Some(("S24_LE", 24, 32)),
        Some(0x8420) => Some(("S32_LE", 32, 32)),
        _ => None,
    };
    let (rate, channels) = (integer(pcm, "Rate"), integer(pcm, "Channels"));
    section.pcm = Some(Pcm {
        format_code,
        format: format.map(|f| f.0),
        valid_bits: format.map(|f| f.1),
        physical_bits: format.map(|f| f.2),
        rate_hz: rate,
        channels,
    });
    let codec = text(pcm, "Codec");
    let encoder = quality(codec.as_deref(), pcm, &section.codec_settings);
    section.negotiated_codec = codec;
    section.actual_bitrate_bps = encoder.bitrate_bps;
    section.encoder = Some(encoder);
    section.transport = Some(Transport {
        object: path.clone(),
        bluealsa_owner: text(raw, "bluealsa_owner"),
        connection_sequence: integer(pcm, "Sequence"),
        running: flag(pcm, "Running"),
        lifetime_note: "snapshot; use ObjectManager removal signals for per-open lifetime",
    });
    let rate_ok = rate.is_some_and(|r| (8000..=192_000).contains(&r));
    if format.is_none() || !rate_ok || channels != Some(2) {
        section.degraded("unsupported_or_incomplete_pcm");
    }
    if raw.get("codec_pcm").and_then(Value::as_str) == Some(path.as_str()) {
        if let Some(Value::Object(mutual)) = raw.get("mutually_available_codecs") {
            section.mutually_usable = Some(mutual.clone());
        }
    }
    Some(())
}

/// The `bluetooth` section; Unavailable when y2-bt-observe gives no usable answer.
pub fn bluetooth(root: &Root) -> Result<Section, Failure> {
    let answer = command::run_with(root, &["/usr/sbin/y2-bt-observe"], 3.0, 262_144, false);
    let output = answer.output.unwrap_or_default();
    if let Ok(Value::Object(observed)) = serde_json::from_str::<Value>(&output) {
        if observed.get("schema").and_then(Value::as_i64) == Some(1) {
            match normalize(root, &observed) {
                Ok(value) => return Ok(value),
                Err(Stop::Refused(error)) => return Err(error),
                Err(Stop::Unobserved) => {}
            }
        }
    }
    Ok(Section::new(root, Value::Null, None))
}
