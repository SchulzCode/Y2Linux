//! Bluetooth observations (`bluetooth.normalize`) and the live encoder (`codec_observation.quality`):
//! what tests/test_platform_bluetooth.py and tests/test_radio_policy.py checked of the Python platform.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::Fixture;

const DEVICE: &str = "/org/bluez/hci0/dev_00_11_22_33_44_55";
const PCM: &str = "/org/bluealsa/hci0/dev_00_11_22_33_44_55/a2dpsrc/sink";

/// y2-bt-observe's answer for one connected headphone playing SBC.
fn raw() -> Value {
    json!({"schema": 1, "stable_owners": true, "bluez_owner": ":1.2", "bluealsa_owner": ":1.3",
           "bluez": {"/org/bluez/hci0": {"org.bluez.Adapter1": {"Powered": true}},
                     DEVICE: {"org.bluez.Device1": {"Address": "00:11:22:33:44:55", "Connected": true,
                                                    "Paired": true, "Trusted": true}}},
           "bluealsa": {PCM: {"org.bluealsa.PCM1": {"Device": DEVICE, "Transport": "A2DP-source",
                                                   "Mode": "sink", "Codec": "SBC", "Format": 0x8210,
                                                   "Rate": 44100, "Channels": 2, "Sequence": 3,
                                                   "Running": true}}},
           "manager": {"Version": "5.0.0", "Codecs": ["a2dp-source:SBC"]}, "codec_pcm": PCM,
           "mutually_available_codecs": {"SBC": {"Rates": [44100, 48000], "Channels": [2]}}})
}

fn observe(fixture: &Fixture, raw: &Value) -> Value {
    normalize(&fixture.root, &Json::parse(&raw.to_string()).unwrap()).unwrap()
}

fn pcm(raw: &mut Value) -> &mut Value {
    &mut raw["bluealsa"][PCM]["org.bluealsa.PCM1"]
}

#[test]
fn observation_separates_local_remote_mutual_and_actual_pcm() {
    let fixture = Fixture::new();
    let mut raw = raw();
    let result = observe(&fixture, &raw);
    assert_eq!(result["negotiated_codec"], "SBC");
    assert_eq!(result["pcm"]["format"], "S16_LE");
    assert_eq!(result["runtime_enabled"], json!(["SBC"]));
    assert!(result["remote_advertised"].is_null());
    assert!(result["requested_codec"].is_null());
    assert!(result["actual_bitrate_bps"].is_null());
    let mutual: Vec<&String> = result["mutually_usable"]
        .as_object()
        .unwrap()
        .keys()
        .collect();
    assert_eq!(mutual, ["SBC"]);
    pcm(&mut raw)["Codec"] = json!("aptX-HD");
    pcm(&mut raw)["Format"] = json!(0x8418);
    let result = observe(&fixture, &raw);
    assert_eq!(result["pcm"]["valid_bits"], 24);
    assert_eq!(result["pcm"]["physical_bits"], 32);
}

#[test]
fn bonding_and_pairing_windows_are_independent_observations() {
    let fixture = Fixture::new();
    let mut raw = raw();
    let adapter = &mut raw["bluez"]["/org/bluez/hci0"]["org.bluez.Adapter1"];
    adapter["Pairable"] = json!(false);
    adapter["Discoverable"] = json!(false);
    let device = &mut raw["bluez"][DEVICE]["org.bluez.Device1"];
    for (key, value) in [
        ("Paired", true),
        ("Bonded", false),
        ("Trusted", true),
        ("Connected", true),
    ] {
        device[key] = json!(value);
    }
    let result = observe(&fixture, &raw);
    assert_eq!(result["adapter"]["pairable"], false);
    assert_eq!(result["adapter"]["discoverable"], false);
    assert_eq!(result["selected_peer"]["paired"], true);
    assert_eq!(result["selected_peer"]["bonded"], false);
    raw["bluez"][DEVICE]["org.bluez.Device1"]
        .as_object_mut()
        .unwrap()
        .shift_remove("Bonded");
    assert!(observe(&fixture, &raw)["selected_peer"]["bonded"].is_null());
}

#[test]
fn disappearance_wrong_direction_owner_change_and_missing_properties_fail_closed() {
    let fixture = Fixture::new();
    let mut raw = raw();
    raw["bluez"][DEVICE]["org.bluez.Device1"]["Connected"] = json!(false);
    assert!(observe(&fixture, &raw)["negotiated_codec"].is_null());
    raw["bluez"][DEVICE]["org.bluez.Device1"]["Connected"] = json!(true);
    pcm(&mut raw)["Mode"] = json!("source");
    assert!(observe(&fixture, &raw)["pcm"].is_null());
    pcm(&mut raw)["Mode"] = json!("sink");
    pcm(&mut raw)["Format"] = json!(123);
    assert_eq!(observe(&fixture, &raw)["state"], "Degraded");
    raw["stable_owners"] = json!(false);
    assert!(observe(&fixture, &raw)["pcm"].is_null());
}

#[test]
fn invalid_saved_codec_preferences_do_not_hide_observed_transport() {
    let fixture = Fixture::new();
    let raw = raw();
    for invalid in [
        json!({"schema": 99}),
        json!({"schema": 1, "ldac_abr": "yes"}),
    ] {
        let original = invalid.to_string();
        fixture.put("/data/bluetooth/codec-policy.json", &original);
        let result = observe(&fixture, &raw);
        assert_eq!(result["negotiated_codec"], "SBC");
        assert_eq!(result["pcm"]["format"], "S16_LE");
        assert_eq!(result["codec_settings"]["state"], "Failed");
        assert_eq!(fixture.read("/data/bluetooth/codec-policy.json"), original);
    }
}

#[test]
fn native_observer_private_bus_with_missing_owners_is_not_ready() {
    // What y2-bt-observe prints on a bus without BlueZ and BlueALSA (tests/test_platform_bluetooth.py
    // runs the observer itself on a private bus).
    let fixture = Fixture::new();
    let raw = json!({"schema": 1, "stable_owners": true, "bluez_owner": null, "bluealsa_owner": null,
                     "bluez": null, "bluealsa": null, "manager": null, "codec_pcm": null,
                     "mutually_available_codecs": null});
    assert_eq!(observe(&fixture, &raw)["state"], "Unavailable");
}

// ---- codec_observation.quality (tests/test_radio_policy.py, CodecQuality) ----------------------

fn encoder(codec: &str, pcm: &Value) -> Value {
    quality(
        Some(codec),
        &Json::parse(&pcm.to_string()).unwrap(),
        &json!({}),
    )
}

#[test]
fn sbc_live_bitpool_and_adjustments_are_observed() {
    let mut pcm = json!({"Running": true, "Rate": 44100, "CodecConfiguration": [0x21, 0x15, 2, 53],
                         "EncoderStats": {"Active": 1, "Bitpool": 53, "BitrateKbps": 328,
                                          "AbrEnabled": 1, "AbrAdjustments": 0}});
    let value = encoder("SBC", &pcm);
    assert_eq!(
        json!([
            value["sbc_bitpool"],
            value["sbc_abr_enabled"],
            value["sbc_abr_adjustments"]
        ]),
        json!([53, true, 0])
    );
    pcm["EncoderStats"]["Bitpool"] = json!(35);
    pcm["EncoderStats"]["BitrateKbps"] = json!(229);
    pcm["EncoderStats"]["AbrAdjustments"] = json!(2);
    let value = encoder("SBC", &pcm);
    assert_eq!(
        json!([
            value["sbc_bitpool"],
            value["bitrate_bps"],
            value["sbc_abr_adjustments"]
        ]),
        json!([35, 229000, 2])
    );
    pcm["Running"] = json!(false);
    assert!(encoder("SBC", &pcm)["sbc_abr_enabled"].is_null());
}

#[test]
fn sbc_xq_requires_negotiated_mode_and_actual_bitpool() {
    let mut pcm = json!({"Running": true, "Rate": 44100, "CodecConfiguration": [0x24, 0x15, 2, 53],
                         "EncoderStats": {"Active": 1, "Bitpool": 38, "BitrateKbps": 452}});
    assert_eq!(encoder("SBC", &pcm)["sbc_quality"], "xq");
    pcm["EncoderStats"]["Bitpool"] = json!(35);
    assert_eq!(encoder("SBC", &pcm)["sbc_quality"], "standard");
    pcm["EncoderStats"]["Bitpool"] = json!(47);
    assert_eq!(encoder("SBC", &pcm)["sbc_quality"], "xq+");
    pcm["CodecConfiguration"][0] = json!(0x21);
    assert_eq!(encoder("SBC", &pcm)["sbc_quality"], "standard");
    pcm["Running"] = json!(false);
    assert!(encoder("SBC", &pcm)["sbc_quality"].is_null());
}

#[test]
fn ldac_abr_is_observed_only_from_active_encoder_transitions() {
    let mut pcm = json!({"Running": true, "Rate": 44100,
                         "EncoderStats": {"Active": 1, "BitrateKbps": 606, "QualityIndex": 1,
                                          "AbrEnabled": 1, "AbrAdjustments": 0}});
    let first = encoder("LDAC", &pcm);
    assert_eq!(first["ldac_nominal_choices_kbps"], json!([303, 606, 909]));
    assert_eq!(first["ldac_abr_adaptation_observed"], false);
    pcm["EncoderStats"]["AbrAdjustments"] = json!(2);
    pcm["Rate"] = json!(48000);
    let next = encoder("LDAC", &pcm);
    assert_eq!(next["ldac_nominal_choices_kbps"], json!([330, 660, 990]));
    assert_eq!(next["ldac_abr_adaptation_observed"], true);
    pcm["EncoderStats"]["Active"] = json!(0);
    assert!(encoder("LDAC", &pcm)["bitrate_bps"].is_null());
    assert_eq!(encoder("LDAC", &pcm)["ldac_abr_adaptation_observed"], false);
}

#[test]
fn aac_rate_and_adaptation_are_observed_from_the_live_encoder() {
    let mut pcm = json!({"Running": true, "Rate": 44100,
                         "EncoderStats": {"Active": 1, "BitrateKbps": 256, "QualityIndex": 0,
                                          "AbrEnabled": 1, "AbrAdjustments": 0, "Errors": 0}});
    let value = encoder("AAC", &pcm);
    assert_eq!(
        json!([
            value["bitrate_bps"],
            value["aac_abr_enabled"],
            value["aac_abr_level"]
        ]),
        json!([256000, true, 0])
    );
    pcm["EncoderStats"]["BitrateKbps"] = json!(160);
    pcm["EncoderStats"]["QualityIndex"] = json!(2);
    pcm["EncoderStats"]["AbrAdjustments"] = json!(2);
    let value = encoder("AAC", &pcm);
    assert_eq!(
        json!([
            value["bitrate_bps"],
            value["aac_abr_level"],
            value["aac_abr_adjustments"]
        ]),
        json!([160000, 2, 2])
    );
    pcm["Running"] = json!(false);
    assert!(encoder("AAC", &pcm)["bitrate_bps"].is_null());
}
