//! The `bluetooth` section: what D-Bus shows of the adapter, the selected peer, its playback PCM and
//! encoder (y2-bt-observe), next to the codec settings; observations only, never codec requests.
// SPDX-License-Identifier: GPL-2.0-only
use super::snapshot::readiness::Readiness;
use super::util::fields;
use crate::{commands::codec, reply::Failure};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use y2p_core::{
    command,
    lenient::{self, BootId},
    Root,
};

/// When `/run/y2/bt-reconnect.json` was written (y2-bt-reconnect, every few seconds).
#[derive(Deserialize, Default)]
#[serde(default)]
struct ReconnectStamp {
    boot_id: BootId,
    #[serde(deserialize_with = "lenient::integer")]
    monotonic_us: Option<i64>,
}

/// The codec settings the section shows: `codec-settings`' record, or why there is none (a refused
/// policy or inventory).
#[derive(Serialize)]
#[serde(untagged)]
pub enum CodecSettings {
    Settings(Box<codec::Settings>),
    Failed {
        schema: &'static str,
        state: &'static str,
        reason: String,
        /// Null: no policy was read.
        requested: (),
        effective: (),
        pending_restart: bool,
        supported: std::collections::BTreeMap<String, bool>,
    },
}

impl CodecSettings {
    fn read(root: &Root) -> CodecSettings {
        match codec::settings(root) {
            Ok(settings) => CodecSettings::Settings(Box::new(settings)),
            Err(refusal) => CodecSettings::Failed {
                schema: codec::SETTINGS_SCHEMA,
                state: "Failed",
                reason: refusal.message(),
                requested: (),
                effective: (),
                pending_restart: false,
                supported: Default::default(),
            },
        }
    }

    /// The owner's policy (none where the settings were refused).
    fn requested(&self) -> Option<&codec::Policy> {
        match self {
            CodecSettings::Settings(settings) => Some(&settings.requested),
            CodecSettings::Failed { .. } => None,
        }
    }
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
    preference: Option<codec::Policy>,
    encoder_observed: bool,
    bitrate_scope: &'static str,
    ldac_abr_adaptation_observed: bool,
    encoder_errors: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sbc_negotiated_bitpool_range: Option<[i64; 2]>,
}

fn quality(codec: Option<&str>, pcm: &PcmProperties, settings: Option<&CodecSettings>) -> Encoder {
    let config: Vec<i64> = pcm
        .codec_configuration
        .iter()
        .flatten()
        .map(|v| Some(*v).filter(|n| (0..=255).contains(n)))
        .collect::<Option<Vec<i64>>>()
        .unwrap_or_default();
    let stats = pcm.encoder_stats.as_ref();
    let live = pcm.running == Some(true) && stats.is_some_and(|s| s.get("Active") == Some(&1));
    let observed = |key: &str, minimum: i64, maximum: i64| -> Option<i64> {
        if !live {
            return None;
        }
        stats
            .and_then(|s| s.get(key).copied())
            .filter(|v| (minimum..=maximum).contains(v))
    };
    const MAX: i64 = (1 << 31) - 1;
    let flag = |enabled: Option<i64>| enabled.map(|e| e != 0);
    let mut encoder = Encoder {
        preference: settings.and_then(CodecSettings::requested).cloned(),
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
            encoder.ldac_nominal_choices_kbps = match pcm.rate {
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
    pub state: Readiness,
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
    codec_settings: Option<CodecSettings>,
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
    fn new(root: &Root, settings: Option<CodecSettings>, inventory: Option<Value>) -> Section {
        Section {
            state: Readiness::Unavailable,
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
        (self.state, self.reason) = (Readiness::Degraded, reason);
    }
}

/// y2-bt-observe's answer (schema 1): the bus owners and BlueZ's and BlueALSA's objects, each by
/// object path (in the helper's order) with the properties of the interfaces read here.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Observed {
    schema: Option<i64>,
    stable_owners: bool,
    #[serde(deserialize_with = "lenient::text")]
    bluez_owner: Option<String>,
    #[serde(deserialize_with = "lenient::text")]
    bluealsa_owner: Option<String>,
    #[serde(deserialize_with = "lenient::or_none")]
    bluez: Option<Map<String, Value>>,
    #[serde(deserialize_with = "lenient::or_none")]
    bluealsa: Option<Map<String, Value>>,
    #[serde(deserialize_with = "lenient::or_none")]
    manager: Option<Manager>,
    #[serde(deserialize_with = "lenient::text")]
    codec_pcm: Option<String>,
    /// The codecs both ends offer, by codec (BlueALSA's capabilities, shown as they are).
    #[serde(deserialize_with = "lenient::or_none")]
    mutually_available_codecs: Option<Map<String, Value>>,
}

/// BlueALSA's manager object.
#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Manager {
    #[serde(deserialize_with = "lenient::some_texts")]
    codecs: Option<Vec<String>>,
    #[serde(deserialize_with = "lenient::text")]
    version: Option<String>,
}

/// A BlueZ object's interfaces read here.
#[derive(Deserialize, Default)]
#[serde(default)]
struct BluezObject {
    #[serde(rename = "org.bluez.Adapter1")]
    adapter: Option<AdapterProperties>,
    #[serde(rename = "org.bluez.Device1")]
    device: Option<DeviceProperties>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct AdapterProperties {
    #[serde(deserialize_with = "lenient::or_none")]
    powered: Option<bool>,
    #[serde(deserialize_with = "lenient::or_none")]
    discovering: Option<bool>,
    #[serde(deserialize_with = "lenient::or_none")]
    pairable: Option<bool>,
    #[serde(deserialize_with = "lenient::or_none")]
    discoverable: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct DeviceProperties {
    #[serde(deserialize_with = "lenient::or_none")]
    address: Option<String>,
    #[serde(deserialize_with = "lenient::or_none")]
    paired: Option<bool>,
    #[serde(deserialize_with = "lenient::or_none")]
    bonded: Option<bool>,
    #[serde(deserialize_with = "lenient::or_none")]
    trusted: Option<bool>,
    #[serde(deserialize_with = "lenient::or_none")]
    connected: Option<bool>,
    #[serde(deserialize_with = "lenient::or_none")]
    services_resolved: Option<bool>,
}

/// A BlueALSA object's PCM interface.
#[derive(Deserialize, Default)]
#[serde(default)]
struct BluealsaObject {
    #[serde(rename = "org.bluealsa.PCM1")]
    pcm: Option<PcmProperties>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct PcmProperties {
    #[serde(deserialize_with = "lenient::or_none")]
    device: Option<String>,
    #[serde(deserialize_with = "lenient::or_none")]
    transport: Option<String>,
    #[serde(deserialize_with = "lenient::or_none")]
    mode: Option<String>,
    #[serde(deserialize_with = "lenient::or_none")]
    codec: Option<String>,
    #[serde(deserialize_with = "lenient::or_none")]
    format: Option<i64>,
    #[serde(deserialize_with = "lenient::or_none")]
    rate: Option<i64>,
    #[serde(deserialize_with = "lenient::or_none")]
    channels: Option<i64>,
    #[serde(deserialize_with = "lenient::or_none")]
    sequence: Option<i64>,
    #[serde(deserialize_with = "lenient::or_none")]
    running: Option<bool>,
    #[serde(deserialize_with = "lenient::or_none")]
    codec_configuration: Option<Vec<i64>>,
    /// The encoder's counters (BlueALSA patches 0002/0003), by name.
    #[serde(deserialize_with = "lenient::or_none")]
    encoder_stats: Option<std::collections::BTreeMap<String, i64>>,
}

/// The objects of `map` whose interfaces read as `T`, in the helper's order (an object whose
/// properties have other types is left out).
fn objects<T: serde::de::DeserializeOwned>(map: &Map<String, Value>) -> Vec<(&String, T)> {
    map.iter()
        .filter_map(|(path, value)| Some((path, T::deserialize(value).ok()?)))
        .collect()
}

/// The section from y2-bt-observe's answer (`raw`).
fn normalize(root: &Root, raw: &Observed) -> Section {
    let mut section = settled_base(root);
    observed(root, raw, &mut section);
    section
}

/// The section before y2-bt-observe's answer: the codec settings, inventory and the reconnect
/// helper's record (shown only while fresh: written this boot within the last 10 s).
fn settled_base(root: &Root) -> Section {
    // The installed inventory is shown as installed.
    let inventory = root.json(codec::INVENTORY);
    let settings = Some(CodecSettings::read(root));
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
    section
}

/// What y2-bt-observe saw, added step by step: the adapter, the runtime (bluealsa and its codecs),
/// the devices and the selected peer's transport. A step that cannot go on has left its state or
/// reason and ends the section there (None).
fn observed(root: &Root, raw: &Observed, section: &mut Section) -> Option<()> {
    if raw.schema != Some(1) || !raw.stable_owners {
        return None;
    }
    let bluez: Vec<(&String, BluezObject)> = objects(raw.bluez.as_ref()?);
    adapter(raw, &bluez, section)?;
    let bluealsa = runtime(raw, section)?;
    let devices = devices(&bluez);
    section.devices = Some(devices.iter().take(128).cloned().collect());
    let device = selected_peer(root, &devices, section)?;
    transport(raw, &bluealsa, &device, section)
}

/// The one adapter and whether it is powered.
fn adapter(raw: &Observed, bluez: &[(&String, BluezObject)], section: &mut Section) -> Option<()> {
    let adapters: Vec<(&String, &AdapterProperties)> = bluez
        .iter()
        .filter_map(|(path, object)| Some((*path, object.adapter.as_ref()?)))
        .collect();
    let [(path, adapter)] = adapters[..] else {
        section.reason = "no_unique_adapter";
        return None;
    };
    section.adapter = Some(Adapter {
        path: path.clone(),
        powered: adapter.powered,
        discovering: adapter.discovering,
        pairable: adapter.pairable,
        discoverable: adapter.discoverable,
        bluez_owner: raw.bluez_owner.clone(),
    });
    if adapter.powered != Some(true) {
        section.reason = "radio_off";
        return None;
    }
    Some(())
}

/// bluealsa and its manager: Ready with SBC enabled and the runtime version 5.0.0, else Degraded
/// (Starting while bluealsa is not up). bluealsa's objects for the transport step.
fn runtime<'a>(
    raw: &'a Observed,
    section: &mut Section,
) -> Option<Vec<(&'a String, BluealsaObject)>> {
    let owner = raw
        .bluealsa_owner
        .as_deref()
        .filter(|owner| !owner.is_empty());
    let (Some(bluealsa), Some(manager), Some(_)) = (&raw.bluealsa, &raw.manager, owner) else {
        (section.state, section.reason) = (Readiness::Starting, "bluealsa_not_ready");
        return None;
    };
    (section.state, section.reason) = (Readiness::Ready, "controller_bluez_bluealsa_observed");
    let enabled: Option<Vec<String>> = manager.codecs.as_ref().map(|codecs| {
        codecs
            .iter()
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
    if manager
        .version
        .as_deref()
        .map(|v| v.trim_start_matches('v'))
        != Some("5.0.0")
    {
        section.degraded("bluealsa_runtime_version_mismatch");
    }
    Some(objects(bluealsa))
}

/// BlueZ's devices in y2-bt-observe's order (the object's keys as the output gives them).
fn devices(bluez: &[(&String, BluezObject)]) -> Vec<Device> {
    bluez
        .iter()
        .filter_map(|(path, object)| {
            let device = object.device.as_ref()?;
            Some(Device {
                path: (*path).clone(),
                address: device.address.clone(),
                paired: device.paired,
                bonded: device.bonded,
                trusted: device.trusted,
                connected: device.connected,
                services_resolved: device.services_resolved,
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
    raw: &Observed,
    bluealsa: &[(&String, BluealsaObject)],
    device: &Device,
    section: &mut Section,
) -> Option<()> {
    let pcms: Vec<(&String, &PcmProperties)> = bluealsa
        .iter()
        .filter_map(|(path, object)| {
            let pcm = object.pcm.as_ref()?;
            (pcm.device.as_deref() == Some(device.path.as_str())
                && pcm.transport.as_deref() == Some("A2DP-source")
                && pcm.mode.as_deref() == Some("sink"))
            .then_some((*path, pcm))
        })
        .collect();
    let [(path, pcm)] = pcms[..] else {
        section.transport_reason = Some("no_unique_playback_pcm");
        return None;
    };
    let format_code = pcm.format;
    let format = match format_code {
        Some(0x8210) => Some(("S16_LE", 16, 16)),
        Some(0x8418) => Some(("S24_LE", 24, 32)),
        Some(0x8420) => Some(("S32_LE", 32, 32)),
        _ => None,
    };
    let (rate, channels) = (pcm.rate, pcm.channels);
    section.pcm = Some(Pcm {
        format_code,
        format: format.map(|f| f.0),
        valid_bits: format.map(|f| f.1),
        physical_bits: format.map(|f| f.2),
        rate_hz: rate,
        channels,
    });
    let codec = pcm.codec.clone();
    let encoder = quality(codec.as_deref(), pcm, section.codec_settings.as_ref());
    section.negotiated_codec = codec;
    section.actual_bitrate_bps = encoder.bitrate_bps;
    section.encoder = Some(encoder);
    section.transport = Some(Transport {
        object: path.clone(),
        bluealsa_owner: raw.bluealsa_owner.clone(),
        connection_sequence: pcm.sequence,
        running: pcm.running,
        lifetime_note: "snapshot; use ObjectManager removal signals for per-open lifetime",
    });
    let rate_ok = rate.is_some_and(|r| (8000..=192_000).contains(&r));
    if format.is_none() || !rate_ok || channels != Some(2) {
        section.degraded("unsupported_or_incomplete_pcm");
    }
    if raw.codec_pcm.as_deref() == Some(path.as_str()) {
        section.mutually_usable = raw.mutually_available_codecs.clone();
    }
    Some(())
}

/// The `bluetooth` section; Unavailable when y2-bt-observe gives no usable answer.
pub fn bluetooth(root: &Root) -> Result<Section, Failure> {
    let answer = command::run_with(root, &["/usr/sbin/y2-bt-observe"], 3.0, 262_144, false);
    let output = answer.output.unwrap_or_default();
    match serde_json::from_str::<Observed>(&output) {
        Ok(observed) if observed.schema == Some(1) => Ok(normalize(root, &observed)),
        _ => Ok(Section::new(root, None, None)),
    }
}
