//! `bluetooth.normalize` (with `codec_observation.quality` and this group's copy of
//! `codec_controls.status`): actual D-Bus observations, never codec requests, for platform clients.
//! `status bluetooth` (observation group) gives it `/usr/sbin/y2-bt-observe`'s JSON, read with
//! [`Json::parse`] so the devices keep the order y2-bt-observe printed them in. The result is printed
//! sorted (`status`), so it is a plain value.
// SPDX-License-Identifier: GPL-2.0-only
use super::wifi::Json;
use crate::commands::codec::{inventory_codec, not_iterable, requested, validate};
use crate::reply::Failure;
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{json, Map, Value};
use y2p_core::py;
use y2p_core::{
    clock,
    py::{is_true, number, truthy},
    Root,
};

impl Json {
    /// `json.loads(text)` keeping object order (a later duplicate key keeps the first one's place).
    pub fn parse(text: &str) -> serde_json::Result<Json> {
        serde_json::from_str(text)
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            Json::Value(_) => None,
        }
    }

    pub fn pairs(&self) -> Option<&[(String, Json)]> {
        match self {
            Json::Object(pairs) => Some(pairs),
            _ => None,
        }
    }

    pub fn value(&self) -> Value {
        self.clone().into_value()
    }

    fn leaf(&self) -> Option<&Value> {
        match self {
            Json::Value(value) => Some(value),
            Json::Object(_) => None,
        }
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Tree;
        impl<'de> Visitor<'de> for Tree {
            type Value = Json;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON")
            }
            fn visit_bool<E>(self, v: bool) -> Result<Json, E> {
                Ok(Json::Value(v.into()))
            }
            fn visit_i64<E>(self, v: i64) -> Result<Json, E> {
                Ok(Json::Value(v.into()))
            }
            fn visit_u64<E>(self, v: u64) -> Result<Json, E> {
                Ok(Json::Value(v.into()))
            }
            fn visit_f64<E>(self, v: f64) -> Result<Json, E> {
                Ok(Json::Value(json!(v)))
            }
            fn visit_str<E>(self, v: &str) -> Result<Json, E> {
                Ok(Json::Value(v.into()))
            }
            fn visit_string<E>(self, v: String) -> Result<Json, E> {
                Ok(Json::Value(v.into()))
            }
            fn visit_unit<E>(self) -> Result<Json, E> {
                Ok(Json::Value(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = seq.next_element::<Value>()? {
                    items.push(item);
                }
                Ok(Json::Value(Value::Array(items)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
                let mut pairs: Vec<(String, Json)> = Vec::new();
                while let Some((key, value)) = map.next_entry::<String, Json>()? {
                    match pairs.iter_mut().find(|(k, _)| *k == key) {
                        Some(entry) => entry.1 = value,
                        None => pairs.push((key, value)),
                    }
                }
                Ok(Json::Object(pairs))
            }
        }
        deserializer.deserialize_any(Tree)
    }
}

/// `codec_controls.status(ctx)` (the devices group ports `codec-settings` itself).
pub fn codec_settings(root: &Root) -> Result<Value, Failure> {
    let policy = validate(requested(root)?)?;
    let inventory = root.json_object("/etc/y2linux/bluetooth-codecs.json");
    let applied = root.json_object("/run/y2/codec-runtime.json");
    let boot = root.read("/proc/sys/kernel/random/boot_id");
    // This receipt records startup arguments, never endpoint/negotiation success.
    let pid = root.integer("/run/y2/bluealsa.pid").filter(|p| *p > 1);
    let command = pid.and_then(|p| root.read_limited(&format!("/proc/{p}/cmdline"), 8192));
    let argv: Vec<&str> = command
        .as_deref()
        .filter(|c| !c.is_empty())
        .map(|c| c.split('\0').collect())
        .unwrap_or_default();
    // all(arg in argv for arg in applied.get('arguments', [])): an iteration over whatever it is.
    let present = |arg: &str| argv.contains(&arg);
    let live = !argv.is_empty()
        && argv[0].rsplit('/').next() == Some("bluealsad")
        && match applied.get("arguments") {
            None => true,
            Some(Value::Array(arguments)) => {
                arguments.iter().all(|a| a.as_str().is_some_and(present))
            }
            Some(Value::String(text)) => text.chars().all(|c| present(&c.to_string())),
            Some(Value::Object(keys)) => keys.keys().all(|k| present(k)),
            Some(other) => return Err(not_iterable(other)), // all(... for arg in other)
        };
    let effective = if live
        && super::observe_wifi::eq_text(applied.get("boot_id"), boot.as_deref())
        && boot.as_deref().is_some_and(|b| !b.is_empty())
    {
        applied.get("settings").cloned().unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let policy = Value::Object(policy);
    let compiled = |name: &str| -> Result<Option<Value>, Failure> {
        Ok(inventory_codec(&inventory, name)?.and_then(|c| c.get("compiled_locally").cloned()))
    };
    let (aac, ldac) = (compiled("AAC")?, compiled("LDAC")?);
    let qualities = |flag: &Option<Value>, choices: Value| {
        if flag.as_ref().is_some_and(truthy) {
            choices
        } else {
            json!([])
        }
    };
    Ok(json!({
        "schema": "org.y2linux.codec-settings/v1", "requested": policy.clone(),
        "effective": effective.clone(), "effective_scope": "daemon_start_arguments",
        "pending_restart": !py::eq(Some(&effective), Some(&policy)),
        "apply_action": "restart_player",
        "supported": {"sbc_quality": ["high", "xq", "xq+"], "sbc_abr": true,
                      "aac_quality": qualities(&aac, json!(["standard", "high"])),
                      "aac_abr": is_true(aac.as_ref()),
                      "ldac_quality": qualities(&ldac, json!(["mobile", "standard", "high"])),
                      "ldac_abr": is_true(ldac.as_ref())}}))
}

fn exact_int(value: Option<&Value>) -> Option<i64> {
    value
        .filter(|v| v.is_i64() || v.is_u64())
        .and_then(Value::as_i64)
}

/// `codec_observation.quality(codec, pcm, settings)`: separate peer configuration, encoder
/// observation and quality preference.
pub fn quality(codec: Option<&str>, pcm: &Json, settings: &Value) -> Value {
    let config: Vec<i64> = match pcm.get("CodecConfiguration").and_then(Json::leaf) {
        Some(Value::Array(items)) => {
            let ints: Vec<Option<i64>> = items
                .iter()
                .map(|v| exact_int(Some(v)).filter(|n| (0..=255).contains(n)))
                .collect();
            if ints.iter().all(Option::is_some) {
                ints.into_iter().flatten().collect()
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    };
    let stats = pcm.get("EncoderStats").filter(|s| s.pairs().is_some());
    let live = pcm.get("Running").and_then(Json::leaf) == Some(&Value::Bool(true))
        && stats
            .is_some_and(|s| py::eq(s.get("Active").map(Json::value).as_ref(), Some(&json!(1))));
    let observed = |key: &str, minimum: i64, maximum: i64| -> Value {
        let value = if live {
            stats.and_then(|s| s.get(key)).and_then(Json::leaf)
        } else {
            None
        };
        json!(exact_int(value).filter(|v| (minimum..=maximum).contains(v)))
    };
    const MAX: i64 = (1 << 31) - 1;
    let mut value = json!({
        "preference": settings.get("requested").cloned().unwrap_or(Value::Null),
        "encoder_observed": live, "bitrate_bps": null,
        "bitrate_scope": "encoder_payload_nominal_not_radio_throughput",
        "sbc_quality": null, "sbc_bitpool": null, "sbc_xq_peer_mode": null,
        "sbc_abr_enabled": null, "sbc_abr_adjustments": null,
        "ldac_quality_index": null, "ldac_nominal_choices_kbps": null,
        "ldac_abr_enabled": null, "ldac_abr_adjustments": null,
        "ldac_abr_adaptation_observed": false,
        "aac_abr_enabled": null, "aac_abr_level": null, "aac_abr_adjustments": null,
        "encoder_errors": observed("Errors", 0, MAX)});
    let set = |value: &mut Value, key: &str, v: Value| {
        value.as_object_mut().expect("object").insert(key.into(), v);
    };
    let enabled_flag = |enabled: &Value| match enabled.as_i64() {
        Some(n) => json!(n != 0),
        None => Value::Null,
    };
    match codec {
        Some("SBC") => {
            if config.len() == 4 {
                // A2DP SBC: 44.1kHz dual-channel, 16 blocks, 8 subbands, loudness.
                let xq_mode = config[0] == 0x24 && config[1] == 0x15;
                set(
                    &mut value,
                    "sbc_xq_peer_mode",
                    json!(xq_mode && config[3] >= 38),
                );
                set(
                    &mut value,
                    "sbc_negotiated_bitpool_range",
                    json!(config[2..4]),
                );
                let bitpool = observed("Bitpool", 2, 250);
                set(&mut value, "sbc_bitpool", bitpool.clone());
                if let Some(bitpool) = bitpool.as_i64() {
                    let quality = if xq_mode && bitpool == 47 {
                        "xq+"
                    } else if xq_mode && bitpool == 38 {
                        "xq"
                    } else {
                        "standard"
                    };
                    set(&mut value, "sbc_quality", json!(quality));
                }
            }
            // BlueALSA patch 0003: Bitpool above is the live (possibly stepped down) value;
            // adjustments count how often the link forced a change.
            let enabled = observed("AbrEnabled", 0, 1);
            set(&mut value, "sbc_abr_enabled", enabled_flag(&enabled));
            set(
                &mut value,
                "sbc_abr_adjustments",
                observed("AbrAdjustments", 0, MAX),
            );
        }
        Some("LDAC") => {
            let rate = pcm.get("Rate").map(Json::value);
            let rate_in = |choices: [i64; 2]| {
                choices
                    .iter()
                    .any(|c| py::eq(rate.as_ref(), Some(&json!(c))))
            };
            let choices = if rate_in([44100, 88200]) {
                json!([303, 606, 909])
            } else if rate_in([48000, 96000]) {
                json!([330, 660, 990])
            } else {
                Value::Null
            };
            set(&mut value, "ldac_nominal_choices_kbps", choices);
            set(
                &mut value,
                "ldac_quality_index",
                observed("QualityIndex", 0, 15),
            );
            let enabled = observed("AbrEnabled", 0, 1);
            set(&mut value, "ldac_abr_enabled", enabled_flag(&enabled));
            let adjustments = observed("AbrAdjustments", 0, MAX);
            set(&mut value, "ldac_abr_adjustments", adjustments.clone());
            set(
                &mut value,
                "ldac_abr_adaptation_observed",
                json!(enabled.as_i64() == Some(1) && adjustments.as_i64().unwrap_or(0) > 0),
            );
        }
        Some("AAC") => {
            // BlueALSA patch 0002: CBR rate actually in use, ABR level 0 = full rate (3 = half), and
            // how often the link forced a change.
            let enabled = observed("AbrEnabled", 0, 1);
            set(&mut value, "aac_abr_enabled", enabled_flag(&enabled));
            set(&mut value, "aac_abr_level", observed("QualityIndex", 0, 3));
            set(
                &mut value,
                "aac_abr_adjustments",
                observed("AbrAdjustments", 0, MAX),
            );
        }
        _ => {}
    }
    if matches!(codec, Some("SBC" | "LDAC" | "AAC")) {
        let bitrate = observed("BitrateKbps", 1, 2000);
        set(
            &mut value,
            "bitrate_bps",
            match bitrate.as_i64() {
                Some(kbps) if kbps != 0 => json!(kbps * 1000),
                _ => Value::Null,
            },
        );
    }
    value
}

fn is_mac(text: &str) -> bool {
    let parts: Vec<&str> = text.split(':').collect();
    parts.len() == 6
        && parts
            .iter()
            .all(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn field(object: &Json, key: &str) -> Value {
    object.get(key).map_or(Value::Null, Json::value)
}

/// `bluetooth.normalize(ctx, raw)`. `Err(Crash)` where Python raises something no caller catches
/// (an inventory `codecs` that is not an object).
pub fn normalize(root: &Root, raw: &Json) -> Result<Value, Failure> {
    let settings = match codec_settings(root) {
        Ok(settings) => settings,
        // Invalid saved preferences must not hide a live radio/PCM observation. The mutation/startup
        // path still refuses this policy without changing it.
        Err(Failure::Value(reason)) => json!({
            "schema": "org.y2linux.codec-settings/v1", "state": "Failed", "reason": reason,
            "requested": null, "effective": null, "pending_restart": false, "supported": {}}),
        Err(other) => return Err(other),
    };
    let mut result: Map<String, Value> = json!({
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
        "codec_settings": settings})
    .as_object()
    .cloned()
    .expect("object");
    let fresh = match &result["reconnect"] {
        Value::Object(reconnect) => {
            super::observe_wifi::eq_text(
                reconnect.get("boot_id"),
                root.read("/proc/sys/kernel/random/boot_id").as_deref(),
            ) && matches!(reconnect.get("monotonic_us"), Some(v) if v.is_boolean() || v.is_i64() || v.is_u64())
                && number(reconnect.get("monotonic_us")).is_some_and(|at| {
                    (0.0..10_000_000.0).contains(&(clock::monotonic() * 1e6 - at))
                })
        }
        _ => false,
    };
    if !fresh {
        result.insert(
            "reconnect".into(),
            json!({"state": "Unavailable", "reason": "reconnect_record_stale_or_absent"}),
        );
    }
    let done = |result: Map<String, Value>| Ok(Value::Object(result));
    if raw.pairs().is_none()
        || !py::eq(raw.get("schema").map(Json::value).as_ref(), Some(&json!(1)))
        || !raw.get("stable_owners").is_some_and(|s| truthy(&s.value()))
    {
        return done(result);
    }
    let (Some(bluez), bluealsa) = (raw.get("bluez").and_then(Json::pairs), raw.get("bluealsa"))
    else {
        return done(result);
    };
    let adapters: Vec<(&String, &Json)> = bluez
        .iter()
        .filter_map(|(path, interfaces)| {
            interfaces
                .get("org.bluez.Adapter1")
                .filter(|a| a.pairs().is_some())
                .map(|a| (path, a))
        })
        .collect();
    if adapters.len() != 1 {
        result.insert("reason".into(), json!("no_unique_adapter"));
        return done(result);
    }
    let (path, adapter) = adapters[0];
    result.insert(
        "adapter".into(),
        json!({"path": path, "powered": field(adapter, "Powered"),
               "discovering": field(adapter, "Discovering"),
               "pairable": field(adapter, "Pairable"), "discoverable": field(adapter, "Discoverable"),
               "bluez_owner": field(raw, "bluez_owner")}),
    );
    if field(adapter, "Powered") != Value::Bool(true) {
        result.insert("reason".into(), json!("radio_off"));
        return done(result);
    }
    let manager = raw.get("manager").filter(|m| m.pairs().is_some());
    let bluealsa = bluealsa.and_then(Json::pairs);
    let (Some(manager), Some(bluealsa), true) =
        (manager, bluealsa, truthy(&field(raw, "bluealsa_owner")))
    else {
        result.insert("state".into(), json!("Starting"));
        result.insert("reason".into(), json!("bluealsa_not_ready"));
        return done(result);
    };
    result.insert("state".into(), json!("Ready"));
    result.insert("reason".into(), json!("controller_bluez_bluealsa_observed"));
    let runtime_enabled = match field(manager, "Codecs") {
        Value::Array(codes) => json!(codes
            .iter()
            .filter_map(Value::as_str)
            .filter(|c| c.to_lowercase().starts_with("a2dp-source:"))
            .map(|c| c.split_once(':').expect("prefix").1)
            .collect::<Vec<&str>>()),
        _ => Value::Null,
    };
    let sbc = runtime_enabled
        .as_array()
        .is_some_and(|a| a.iter().any(|c| c == "SBC"));
    result.insert("runtime_enabled".into(), runtime_enabled);
    if !sbc {
        result.insert("state".into(), json!("Degraded"));
        result.insert("reason".into(), json!("a2dp_source_sbc_not_enabled"));
    }
    let version = field(manager, "Version");
    if version.as_str().map(|v| v.trim_start_matches('v')) != Some("5.0.0") {
        result.insert("state".into(), json!("Degraded"));
        result.insert("reason".into(), json!("bluealsa_runtime_version_mismatch"));
    }
    let mut devices: Vec<Value> = Vec::new();
    for (device_path, interfaces) in bluez {
        let Some(device) = interfaces
            .get("org.bluez.Device1")
            .filter(|d| d.pairs().is_some())
        else {
            continue;
        };
        devices.push(json!({"path": device_path, "address": field(device, "Address"),
                            "paired": field(device, "Paired"), "bonded": field(device, "Bonded"),
                            "trusted": field(device, "Trusted"), "connected": field(device, "Connected"),
                            "services_resolved": field(device, "ServicesResolved")}));
    }
    result.insert(
        "devices".into(),
        Value::Array(devices.iter().take(128).cloned().collect()),
    );
    let preferred = root
        .read("/data/bluetooth/preferred-audio")
        .filter(|p| is_mac(p));
    let selected: Vec<&Value> = devices
        .iter()
        .filter(|d| match &preferred {
            Some(mac) => d["address"]
                .as_str()
                .is_some_and(|a| a.to_lowercase() == mac.to_lowercase()),
            None => d["connected"] == Value::Bool(true),
        })
        .collect();
    if selected.len() != 1 {
        result.insert("transport_reason".into(), json!("no_unique_selected_peer"));
        return done(result);
    }
    let device = selected[0].clone();
    result.insert("selected_peer".into(), device.clone());
    if device["connected"] != Value::Bool(true) {
        result.insert(
            "transport_reason".into(),
            json!("selected_peer_disconnected"),
        );
        return done(result);
    }
    let pcms: Vec<(&String, &Json)> = bluealsa
        .iter()
        .filter_map(|(path, interfaces)| {
            let pcm = interfaces
                .get("org.bluealsa.PCM1")
                .filter(|p| p.pairs().is_some())?;
            (field(pcm, "Device") == device["path"]
                && field(pcm, "Transport") == json!("A2DP-source")
                && field(pcm, "Mode") == json!("sink"))
            .then_some((path, pcm))
        })
        .collect();
    if pcms.len() != 1 {
        result.insert("transport_reason".into(), json!("no_unique_playback_pcm"));
        return done(result);
    }
    let (path, pcm) = pcms[0];
    let format_code = field(pcm, "Format");
    let format = match exact_int(Some(&format_code)) {
        Some(0x8210) => Some(("S16_LE", 16, 16)),
        Some(0x8418) => Some(("S24_LE", 24, 32)),
        Some(0x8420) => Some(("S32_LE", 32, 32)),
        _ => None,
    };
    let (rate, channels) = (field(pcm, "Rate"), field(pcm, "Channels"));
    result.insert(
        "pcm".into(),
        json!({"format_code": format_code, "format": format.map(|f| f.0),
               "valid_bits": format.map(|f| f.1), "physical_bits": format.map(|f| f.2),
               "rate_hz": rate, "channels": channels}),
    );
    let codec = field(pcm, "Codec");
    let codec = codec.as_str();
    result.insert("negotiated_codec".into(), json!(codec));
    let encoder = quality(codec, pcm, &result["codec_settings"]);
    result.insert("actual_bitrate_bps".into(), encoder["bitrate_bps"].clone());
    result.insert("encoder".into(), encoder);
    result.insert(
        "transport".into(),
        json!({"object": path, "bluealsa_owner": field(raw, "bluealsa_owner"),
               "connection_sequence": field(pcm, "Sequence"), "running": field(pcm, "Running"),
               "lifetime_note": "snapshot; use ObjectManager removal signals for per-open lifetime"}),
    );
    let rate_ok = exact_int(Some(&rate)).is_some_and(|r| (8000..=192_000).contains(&r));
    if format.is_none() || !rate_ok || !py::eq(Some(&channels), Some(&json!(2))) {
        result.insert("state".into(), json!("Degraded"));
        result.insert("reason".into(), json!("unsupported_or_incomplete_pcm"));
    }
    if raw.get("codec_pcm").map(Json::value) == Some(json!(path)) {
        if let Some(mutual) = raw
            .get("mutually_available_codecs")
            .filter(|m| m.pairs().is_some())
        {
            result.insert("mutually_usable".into(), mutual.value());
        }
    }
    done(result)
}

#[cfg(test)]
#[path = "bluetooth_tests.rs"]
mod bluetooth_tests;
