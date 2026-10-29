//! `observe.bluetooth`, `bluetooth.normalize`, `codec_controls.status` and
//! `codec_observation.quality`: actual D-Bus observations, never codec requests, for platform clients.
// SPDX-License-Identifier: GPL-2.0-only
use super::util::{self, contains, eq, get_eq, is_int};
use crate::{
    commands::codec::{inventory_codec, requested, validate},
    reply::Failure,
};
use serde::de::{Deserializer, IgnoredAny, MapAccess, Visitor};
use serde_json::{json, Map, Value};
use y2p_core::{
    clock, command,
    py::{exact_int, is_true, truthy},
    Root,
};

/// Why `normalize` stopped, as Python's control flow sees it.
enum Stop {
    /// A TypeError or ValueError: `observe.bluetooth` catches it and reports no observation.
    Caught,
    /// Anything else (an AttributeError on an inventory that is no dict): the command crashes.
    Crash(Failure),
}

impl From<Failure> for Stop {
    fn from(error: Failure) -> Self {
        match error {
            Failure::Value(_) | Failure::Os { .. } => Stop::Caught,
            crash => Stop::Crash(crash),
        }
    }
}

/// `codec_controls.status`.
fn codec_settings(root: &Root) -> Result<Value, Stop> {
    let policy = validate(requested(root)?)?;
    let inventory = root.json_object("/etc/y2linux/bluetooth-codecs.json");
    let applied = root.json_object("/run/y2/codec-runtime.json");
    let boot = root.read("/proc/sys/kernel/random/boot_id");
    // This receipt records startup arguments, never endpoint/negotiation success.
    let pid = root.integer("/run/y2/bluealsa.pid");
    let command = match pid {
        Some(pid) if pid > 1 => root.read_limited(&format!("/proc/{pid}/cmdline"), 8192),
        _ => None,
    };
    let argv: Vec<Value> = match command.as_deref().filter(|c| !c.is_empty()) {
        Some(c) => c.split('\0').map(|a| json!(a)).collect(),
        None => Vec::new(),
    };
    let live = !argv.is_empty()
        && argv[0]
            .as_str()
            .is_some_and(|a| a.rsplit('/').next() == Some("bluealsad"))
        && {
            let arguments = match applied.get("arguments") {
                None => Vec::new(),
                // Not iterable: TypeError.
                Some(other) => y2p_core::py::iterate(other).ok_or(Stop::Caught)?,
            };
            arguments.iter().all(|a| contains(&argv, a))
        };
    let effective = if live
        && get_eq(&applied, "boot_id", &util::text(boot.clone()))
        && boot.as_deref().is_some_and(|b| !b.is_empty())
    {
        applied.get("settings").cloned().unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let policy = Value::Object(policy);
    let aac = inventory_codec(&inventory, "AAC")?;
    let ldac = inventory_codec(&inventory, "LDAC")?;
    let compiled =
        |codec: Option<&Map<String, Value>>| codec.and_then(|c| c.get("compiled_locally")).cloned();
    let qualities = |codec: Option<&Map<String, Value>>, list: Value| {
        if compiled(codec).as_ref().is_some_and(truthy) {
            list
        } else {
            json!([])
        }
    };
    Ok(json!({
        "schema": y2p_contract::schema::CODEC_SETTINGS, "requested": policy.clone(),
        "effective": effective.clone(), "effective_scope": "daemon_start_arguments",
        "pending_restart": !eq(&effective, &policy),
        "apply_action": "restart_player",
        "supported": {
            "sbc_quality": ["high", "xq", "xq+"], "sbc_abr": true,
            "aac_quality": qualities(aac, json!(["standard", "high"])),
            "aac_abr": is_true(compiled(aac).as_ref()),
            "ldac_quality": qualities(ldac, json!(["mobile", "standard", "high"])),
            "ldac_abr": is_true(compiled(ldac).as_ref()),
        },
    }))
}

/// `codec_observation.quality`: separate peer configuration, encoder observation and preference.
fn quality(codec: &Value, pcm: &Map<String, Value>, settings: &Value) -> Map<String, Value> {
    let config: Vec<i64> = match pcm.get("CodecConfiguration") {
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| exact_int(Some(v)).filter(|n| (0..=255).contains(n)))
            .collect::<Option<Vec<i64>>>()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let stats = match pcm.get("EncoderStats") {
        Some(Value::Object(map)) => Some(map),
        _ => None,
    };
    let live = is_true(pcm.get("Running")) && stats.is_some_and(|s| get_eq(s, "Active", &json!(1)));
    let observed = |key: &str, minimum: i64, maximum: i64| -> Option<i64> {
        if !live {
            return None;
        }
        exact_int(stats.and_then(|s| s.get(key))).filter(|v| (minimum..=maximum).contains(v))
    };
    const MAX: i64 = (1 << 31) - 1;
    let mut value = Map::new();
    for key in [
        "bitrate_bps",
        "sbc_quality",
        "sbc_bitpool",
        "sbc_xq_peer_mode",
        "sbc_abr_enabled",
        "sbc_abr_adjustments",
        "ldac_quality_index",
        "ldac_nominal_choices_kbps",
        "ldac_abr_enabled",
        "ldac_abr_adjustments",
        "aac_abr_enabled",
        "aac_abr_level",
        "aac_abr_adjustments",
    ] {
        value.insert(key.into(), Value::Null);
    }
    value.insert(
        "preference".into(),
        settings.get("requested").cloned().unwrap_or(Value::Null),
    );
    value.insert("encoder_observed".into(), json!(live));
    value.insert(
        "bitrate_scope".into(),
        json!("encoder_payload_nominal_not_radio_throughput"),
    );
    value.insert("ldac_abr_adaptation_observed".into(), json!(false));
    value.insert("encoder_errors".into(), json!(observed("Errors", 0, MAX)));
    let flag = |enabled: Option<i64>| enabled.map_or(Value::Null, |e| json!(e != 0));
    match codec.as_str() {
        Some("SBC") => {
            if config.len() == 4 {
                // A2DP SBC: 44.1kHz dual-channel, 16 blocks, 8 subbands, loudness.
                let xq = config[0] == 0x24 && config[1] == 0x15;
                value.insert("sbc_xq_peer_mode".into(), json!(xq && config[3] >= 38));
                value.insert(
                    "sbc_negotiated_bitpool_range".into(),
                    json!(config[2..4].to_vec()),
                );
                let bitpool = observed("Bitpool", 2, 250);
                value.insert("sbc_bitpool".into(), json!(bitpool));
                if let Some(bitpool) = bitpool {
                    value.insert(
                        "sbc_quality".into(),
                        json!(if xq && bitpool == 47 {
                            "xq+"
                        } else if xq && bitpool == 38 {
                            "xq"
                        } else {
                            "standard"
                        }),
                    );
                }
            }
            // BlueALSA patch 0003: Bitpool above is the live (possibly stepped down) value;
            // adjustments count how often the link forced a change.
            value.insert("sbc_abr_enabled".into(), flag(observed("AbrEnabled", 0, 1)));
            value.insert(
                "sbc_abr_adjustments".into(),
                json!(observed("AbrAdjustments", 0, MAX)),
            );
        }
        Some("LDAC") => {
            let rate = pcm.get("Rate").cloned().unwrap_or(Value::Null);
            let is = |n: i64| eq(&rate, &json!(n));
            value.insert(
                "ldac_nominal_choices_kbps".into(),
                if is(44100) || is(88200) {
                    json!([303, 606, 909])
                } else if is(48000) || is(96000) {
                    json!([330, 660, 990])
                } else {
                    Value::Null
                },
            );
            value.insert(
                "ldac_quality_index".into(),
                json!(observed("QualityIndex", 0, 15)),
            );
            let enabled = observed("AbrEnabled", 0, 1);
            value.insert("ldac_abr_enabled".into(), flag(enabled));
            let adjustments = observed("AbrAdjustments", 0, MAX);
            value.insert("ldac_abr_adjustments".into(), json!(adjustments));
            value.insert(
                "ldac_abr_adaptation_observed".into(),
                json!(enabled == Some(1) && adjustments.unwrap_or(0) > 0),
            );
        }
        Some("AAC") => {
            // BlueALSA patch 0002: CBR rate actually in use, ABR level 0 = full rate (3 = half), and
            // how often the link forced a change.
            value.insert("aac_abr_enabled".into(), flag(observed("AbrEnabled", 0, 1)));
            value.insert(
                "aac_abr_level".into(),
                json!(observed("QualityIndex", 0, 3)),
            );
            value.insert(
                "aac_abr_adjustments".into(),
                json!(observed("AbrAdjustments", 0, MAX)),
            );
        }
        _ => {}
    }
    if matches!(codec.as_str(), Some("SBC" | "LDAC" | "AAC")) {
        let bitrate = observed("BitrateKbps", 1, 2000);
        value.insert(
            "bitrate_bps".into(),
            json!(bitrate.filter(|b| *b != 0).map(|b| b * 1000)),
        );
    }
    value
}

/// The key order of the `bluez` object in y2-bt-observe's output (Python dicts keep it; the device
/// list follows it).
struct BluezOrder(Vec<String>);

impl<'de> serde::Deserialize<'de> for BluezOrder {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Top;
        impl<'de> Visitor<'de> for Top {
            type Value = BluezOrder;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<BluezOrder, A::Error> {
                let mut order = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    if key == "bluez" {
                        order = map.next_value::<Keys>()?.0;
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(BluezOrder(order))
            }
        }
        deserializer.deserialize_map(Top)
    }
}

/// An object's keys in document order (a repeated key keeps its first position, as a dict).
struct Keys(Vec<String>);

impl<'de> serde::Deserialize<'de> for Keys {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Object;
        impl<'de> Visitor<'de> for Object {
            type Value = Keys;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("any value")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Keys, A::Error> {
                let mut keys: Vec<String> = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    map.next_value::<IgnoredAny>()?;
                    if !keys.contains(&key) {
                        keys.push(key);
                    }
                }
                Ok(Keys(keys))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Keys, A::Error> {
                while seq.next_element::<IgnoredAny>()?.is_some() {}
                Ok(Keys(Vec::new()))
            }
            fn visit_bool<E>(self, _: bool) -> Result<Keys, E> {
                Ok(Keys(Vec::new()))
            }
            fn visit_i64<E>(self, _: i64) -> Result<Keys, E> {
                Ok(Keys(Vec::new()))
            }
            fn visit_u64<E>(self, _: u64) -> Result<Keys, E> {
                Ok(Keys(Vec::new()))
            }
            fn visit_f64<E>(self, _: f64) -> Result<Keys, E> {
                Ok(Keys(Vec::new()))
            }
            fn visit_str<E>(self, _: &str) -> Result<Keys, E> {
                Ok(Keys(Vec::new()))
            }
            fn visit_unit<E>(self) -> Result<Keys, E> {
                Ok(Keys(Vec::new()))
            }
        }
        deserializer.deserialize_any(Object)
    }
}

fn base(root: &Root, settings: Value) -> Map<String, Value> {
    let value = json!({
        "state": "Unavailable", "reason": "dbus_observation_unavailable",
        "adapter": null, "selected_peer": null, "transport": null,
        "negotiated_codec": null, "pcm": null,
        "remote_advertised": null, "remote_advertised_reason": "API_exposes_mutual_subset_only",
        "runtime_enabled": null, "mutually_usable": null,
        "preferred_codec": null, "requested_codec": null,
        "actual_bitrate_bps": null, "packet_loss": null,
        "reconnect_owner": "y2-bt-reconnect",
        "reconnect": root.json("/run/y2/bt-reconnect.json"),
        "codec_inventory": root.json("/etc/y2linux/bluetooth-codecs.json"),
        "codec_settings": settings,
    });
    match value {
        Value::Object(map) => map,
        _ => unreachable!(),
    }
}

/// `bluetooth.normalize(ctx, raw)`.
fn normalize(root: &Root, raw: &Map<String, Value>, order: &[String]) -> Result<Value, Stop> {
    let settings = match codec_settings(root) {
        Ok(settings) => settings,
        Err(Stop::Crash(error)) => return Err(Stop::Crash(error)),
        Err(Stop::Caught) => {
            // Only a ValueError is caught here; a TypeError reaches observe.bluetooth.
            match codec_failure(root)? {
                Some(reason) => {
                    json!({"schema": y2p_contract::schema::CODEC_SETTINGS, "state": "Failed",
                                       "reason": reason, "requested": null, "effective": null,
                                       "pending_restart": false, "supported": {}})
                }
                None => return Err(Stop::Caught),
            }
        }
    };
    let mut result = base(root, settings);
    let reconnect = result.get("reconnect").cloned().unwrap_or(Value::Null);
    let stale = match &reconnect {
        Value::Object(r) => {
            !get_eq(r, "boot_id", &util::boot_id(root))
                || !is_int(r.get("monotonic_us")).is_some_and(|us| {
                    let age = clock::monotonic() * 1e6 - us as f64;
                    (0.0..10_000_000.0).contains(&age)
                })
        }
        _ => true,
    };
    if stale {
        result.insert(
            "reconnect".into(),
            json!({"state": "Unavailable", "reason": "reconnect_record_stale_or_absent"}),
        );
    }
    if !get_eq(raw, "schema", &json!(1)) || !raw.get("stable_owners").is_some_and(truthy) {
        return Ok(Value::Object(result));
    }
    let Some(Value::Object(bluez)) = raw.get("bluez") else {
        return Ok(Value::Object(result));
    };
    let adapters: Vec<(&String, &Map<String, Value>)> = bluez
        .iter()
        .filter_map(|(path, interfaces)| {
            interfaces
                .get("org.bluez.Adapter1")
                .and_then(Value::as_object)
                .map(|a| (path, a))
        })
        .collect();
    if adapters.len() != 1 {
        result.insert("reason".into(), json!("no_unique_adapter"));
        return Ok(Value::Object(result));
    }
    let (path, adapter) = adapters[0];
    let get = |m: &Map<String, Value>, k: &str| m.get(k).cloned().unwrap_or(Value::Null);
    result.insert(
        "adapter".into(),
        json!({"path": path, "powered": get(adapter, "Powered"),
        "discovering": get(adapter, "Discovering"), "pairable": get(adapter, "Pairable"),
        "discoverable": get(adapter, "Discoverable"), "bluez_owner": get(raw, "bluez_owner")}),
    );
    if !is_true(adapter.get("Powered")) {
        result.insert("reason".into(), json!("radio_off"));
        return Ok(Value::Object(result));
    }
    let bluealsa = match raw.get("bluealsa") {
        Some(Value::Object(map)) => Some(map),
        _ => None,
    };
    let manager = match raw.get("manager") {
        Some(Value::Object(map)) => Some(map),
        _ => None,
    };
    let (Some(bluealsa), Some(manager)) = (bluealsa, manager) else {
        result.insert("state".into(), json!("Starting"));
        result.insert("reason".into(), json!("bluealsa_not_ready"));
        return Ok(Value::Object(result));
    };
    if !raw.get("bluealsa_owner").is_some_and(truthy) {
        result.insert("state".into(), json!("Starting"));
        result.insert("reason".into(), json!("bluealsa_not_ready"));
        return Ok(Value::Object(result));
    }
    result.insert("state".into(), json!("Ready"));
    result.insert("reason".into(), json!("controller_bluez_bluealsa_observed"));
    let enabled: Option<Vec<String>> = match manager.get("Codecs") {
        Some(Value::Array(codes)) => Some(
            codes
                .iter()
                .filter_map(Value::as_str)
                .filter(|c| c.to_lowercase().starts_with("a2dp-source:"))
                .map(|c| c.split_once(':').map_or("", |(_, r)| r).to_owned())
                .collect(),
        ),
        _ => None,
    };
    result.insert("runtime_enabled".into(), json!(enabled));
    if !enabled
        .as_ref()
        .is_some_and(|e| !e.is_empty() && e.iter().any(|c| c == "SBC"))
    {
        result.insert("state".into(), json!("Degraded"));
        result.insert("reason".into(), json!("a2dp_source_sbc_not_enabled"));
    }
    // str(Version).lstrip('v') == '5.0.0': only a str can spell it.
    let version = match manager.get("Version") {
        None => Some(String::new()),
        Some(Value::String(s)) => Some(s.trim_start_matches('v').to_owned()),
        Some(_) => None,
    };
    if version.as_deref() != Some("5.0.0") {
        result.insert("state".into(), json!("Degraded"));
        result.insert("reason".into(), json!("bluealsa_runtime_version_mismatch"));
    }
    let mut devices = Vec::new();
    for device_path in order {
        let Some(device) = bluez
            .get(device_path)
            .and_then(|i| i.get("org.bluez.Device1"))
            .and_then(Value::as_object)
        else {
            continue;
        };
        devices.push(json!({"path": device_path, "address": get(device, "Address"),
            "paired": get(device, "Paired"), "bonded": get(device, "Bonded"), "trusted": get(device, "Trusted"),
            "connected": get(device, "Connected"), "services_resolved": get(device, "ServicesResolved")}));
    }
    result.insert(
        "devices".into(),
        Value::Array(devices.iter().take(128).cloned().collect()),
    );
    let preferred = root.read("/data/bluetooth/preferred-audio");
    let selected: Vec<&Value> = match preferred
        .as_deref()
        .filter(|p| crate::regex!(r"\A(?:[A-Fa-f0-9]{2}:){5}[A-Fa-f0-9]{2}\z").is_match(p))
    {
        Some(p) => devices
            .iter()
            .filter(|d| {
                d["address"]
                    .as_str()
                    .is_some_and(|a| a.to_lowercase() == p.to_lowercase())
            })
            .collect(),
        None => devices
            .iter()
            .filter(|d| is_true(d.get("connected")))
            .collect(),
    };
    if selected.len() != 1 {
        result.insert("transport_reason".into(), json!("no_unique_selected_peer"));
        return Ok(Value::Object(result));
    }
    let device = selected[0].clone();
    result.insert("selected_peer".into(), device.clone());
    if !is_true(device.get("connected")) {
        result.insert(
            "transport_reason".into(),
            json!("selected_peer_disconnected"),
        );
        return Ok(Value::Object(result));
    }
    let pcms: Vec<(&String, &Map<String, Value>)> = bluealsa
        .iter()
        .filter_map(|(path, interfaces)| {
            let pcm = interfaces.get("org.bluealsa.PCM1")?.as_object()?;
            (get_eq(pcm, "Device", &device["path"])
                && pcm.get("Transport").and_then(Value::as_str) == Some("A2DP-source")
                && pcm.get("Mode").and_then(Value::as_str) == Some("sink"))
            .then_some((path, pcm))
        })
        .collect();
    if pcms.len() != 1 {
        result.insert("transport_reason".into(), json!("no_unique_playback_pcm"));
        return Ok(Value::Object(result));
    }
    let (path, pcm) = pcms[0];
    let format_code = get(pcm, "Format");
    let format = match exact_int(Some(&format_code)) {
        Some(0x8210) => Some(("S16_LE", 16, 16)),
        Some(0x8418) => Some(("S24_LE", 24, 32)),
        Some(0x8420) => Some(("S32_LE", 32, 32)),
        _ => None,
    };
    let (rate, channels) = (get(pcm, "Rate"), get(pcm, "Channels"));
    result.insert(
        "pcm".into(),
        json!({"format_code": format_code, "format": format.map(|f| f.0),
        "valid_bits": format.map(|f| f.1), "physical_bits": format.map(|f| f.2),
        "rate_hz": rate, "channels": channels}),
    );
    let codec = match pcm.get("Codec") {
        Some(Value::String(s)) => json!(s),
        _ => Value::Null,
    };
    result.insert("negotiated_codec".into(), codec.clone());
    let encoder = quality(&codec, pcm, &result["codec_settings"]);
    result.insert("actual_bitrate_bps".into(), encoder["bitrate_bps"].clone());
    result.insert("encoder".into(), Value::Object(encoder));
    result.insert(
        "transport".into(),
        json!({"object": path, "bluealsa_owner": get(raw, "bluealsa_owner"),
        "connection_sequence": get(pcm, "Sequence"), "running": get(pcm, "Running"),
        "lifetime_note": "snapshot; use ObjectManager removal signals for per-open lifetime"}),
    );
    let rate_ok = exact_int(Some(&rate)).is_some_and(|r| (8000..=192_000).contains(&r));
    if format.is_none() || !rate_ok || !eq(&channels, &json!(2)) {
        result.insert("state".into(), json!("Degraded"));
        result.insert("reason".into(), json!("unsupported_or_incomplete_pcm"));
    }
    if get_eq(raw, "codec_pcm", &json!(path)) {
        if let Some(Value::Object(mutual)) = raw.get("mutually_available_codecs") {
            result.insert("mutually_usable".into(), Value::Object(mutual.clone()));
        }
    }
    Ok(Value::Object(result))
}

/// The ValueError text of `codec_controls.status` (None when what failed was a TypeError).
fn codec_failure(root: &Root) -> Result<Option<String>, Stop> {
    match requested(root).and_then(validate) {
        Err(error @ (Failure::Value(_) | Failure::Os { .. })) => Ok(Some(error.message())),
        Err(crash) => Err(Stop::Crash(crash)),
        Ok(_) => Ok(None),
    }
}

/// `observe.bluetooth`.
pub fn bluetooth(root: &Root) -> Result<Value, Failure> {
    let answer = command::run_with(root, &["/usr/sbin/y2-bt-observe"], 3.0, 262_144, false);
    let output = answer.output.unwrap_or_default();
    if let Ok(Value::Object(observed)) = serde_json::from_str::<Value>(&output) {
        if get_eq(&observed, "schema", &json!(1)) {
            let order = serde_json::from_str::<BluezOrder>(&output)
                .map(|o| o.0)
                .unwrap_or_default();
            match normalize(root, &observed, &order) {
                Ok(value) => return Ok(value),
                Err(Stop::Crash(error)) => return Err(error),
                Err(Stop::Caught) => {}
            }
        }
    }
    Ok(
        json!({"state": "Unavailable", "reason": "dbus_observation_unavailable",
              "adapter": null, "selected_peer": null, "transport": null,
              "negotiated_codec": null, "pcm": null, "reconnect_owner": "y2-bt-reconnect",
              "reconnect": root.json("/run/y2/bt-reconnect.json")}),
    )
}
