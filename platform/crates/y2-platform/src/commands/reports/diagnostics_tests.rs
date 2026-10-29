//! The tester export (tests/test_diagnostic_export.py, the projection contracts of
//! test_platform_integration.py and test_audio_precision_contract.py): an allowlisted projection, log
//! tails as event classes, a private archive bound to the verified /data.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::{ok, Fixture};

fn projected(text: &str) -> Py {
    project(&pyjson::loads(text).unwrap(), "", 0)
}

fn dumped(value: &Py) -> String {
    pyjson::dumps(value, Style::Spaced)
}

fn at<'a>(value: &'a Py, path: &[&str]) -> &'a Py {
    path.iter().fold(value, |v, k| {
        v.get(k).unwrap_or_else(|| panic!("{k} in {v:?}"))
    })
}

fn string(value: &str) -> Py {
    Py::str(value)
}

#[test]
fn projection_and_event_excerpts_never_include_private_free_text() {
    let secret = "unique-personal-secret";
    let raw = format!(
        r#"{{"ssid": "{secret}", "password": "{secret}", "foo": "{secret}", "track_title": "{secret}",
            "/private/{secret}": 1, "PersonalNameThatMustNeverAppear": 123,
            "nested": {{"temperature_c": 34, "enabled": true, "state": "Ready"}},
            "boot_id": "159f7c81-ab89-4440-bc89-df827dd8bc34"}}"#
    );
    let output = projected(&raw);
    assert!(!dumped(&output).contains(secret));
    assert!(!dumped(&output).contains("PersonalNameThatMustNeverAppear"));
    assert_eq!(at(&output, &["nested", "temperature_c"]), &Py::int(34));
    assert_eq!(
        at(&output, &["boot_id"]),
        &string("159f7c81-ab89-4440-bc89-df827dd8bc34")
    );
    let log = events(Some(&format!(
        "reborn failed to open /data/music/{secret} error=-5\nwifi error password={secret} ssid=personal\n"
    )));
    assert!(!dumped(&log).contains(secret));
    let Py::List(rows) = &log else {
        panic!("{log:?}")
    };
    assert_eq!(at(&rows[0], &["errno"]), &Py::List(vec![Py::int(-5)]));
    assert_eq!(
        at(&rows[0], &["events"]),
        &Py::List(vec![string("failed"), string("error")])
    );
}

#[test]
fn fixed_kernel_fields_keep_useful_pm_storage_usb_evidence() {
    let result = projected(
        r#"{"spm": "entries=1 resumes=1 wake=0x20 secret=owner-token",
            "suspend_persistent": {"current": "valid=1 stage=EXIT error=0"},
            "mmc_runtime_pm": {"11230000.mmc": "gated=1 suspends=42 last_mismatch=0x0"},
            "usb_restore": {"controllers": {"11200000.usb": "transfer=inventra_dma dma_errors=0 irq_max_burst=26"}},
            "product": {"state": "restored", "same_boot": true, "wake_reason": "rtc"}}"#,
    );
    assert_eq!(at(&result, &["spm", "wake"]), &string("0x20"));
    assert_eq!(
        at(&result, &["suspend_persistent", "current", "stage"]),
        &string("EXIT")
    );
    assert_eq!(
        at(&result, &["mmc_runtime_pm", "11230000.mmc", "suspends"]),
        &string("42")
    );
    assert_eq!(
        at(
            &result,
            &["usb_restore", "controllers", "11200000.usb", "transfer"]
        ),
        &string("inventra_dma")
    );
    assert_eq!(at(&result, &["product", "same_boot"]), &Py::Bool(true));
    assert_eq!(at(&result, &["product", "state"]), &string("restored"));
    assert!(!dumped(&result).contains("owner-token"));
}

#[test]
fn codec_observation_survives_without_peer_identifiers() {
    let result = projected(
        r#"{"bluetooth_link": {"negotiated_codec": "LDAC", "preferred_codec": "Auto",
            "runtime_enabled": ["SBC", "LDAC"], "codec_inventory": {"codecs": {
                "LDAC": {"compiled_locally": true, "distribution_approved": false}}},
            "encoder": {"ldac_abr_adjustments": 7, "ldac_abr_adaptation_observed": true},
            "selected_peer": {"address": "AA:BB:CC:DD:EE:FF", "name": "Private peer", "connected": true}}}"#,
    );
    let result = at(&result, &["bluetooth_link"]);
    assert_eq!(at(result, &["negotiated_codec"]), &string("LDAC"));
    assert_eq!(at(result, &["preferred_codec"]), &string("Auto"));
    assert_eq!(
        at(result, &["encoder", "ldac_abr_adjustments"]),
        &Py::int(7)
    );
    assert_eq!(
        at(
            result,
            &["codec_inventory", "codecs", "LDAC", "compiled_locally"]
        ),
        &Py::Bool(true)
    );
    assert!(!dumped(result).contains("AA:BB:CC:DD:EE:FF"));
    assert!(!dumped(result).contains("Private peer"));
}

#[test]
fn wifi_observations_survive_without_peer_or_lease_identifiers() {
    let peer = "AA:BB:CC:DD:EE:FF";
    let result = projected(&format!(
        r#"{{"wifi": {{"state": "Online", "association": "COMPLETED",
            "association_id": "{peer}", "ip_addresses": ["192.0.2.2"],
            "status_query_failures": 3, "rssi_dbm": -52,
            "signal_observation": {{"monotonic_s": 100.0, "ok": false,
                                    "reason": "command_timeout", "bssid": "{peer}"}},
            "events": {{"real_disconnects": 1, "reconnects": 2,
                        "association_active": true, "last_event": "CONNECTED",
                        "link_counts_since_s": 90.0}},
            "power_save": {{"mode": "automatic", "requested_driver_profile": "CAM",
                            "applied": false, "ifindex": "4", "epoch": "private-lease"}},
            "traffic_counters": {{"rx_errors": 0, "tx_packets": 100}}}}}}"#
    ));
    let result = at(&result, &["wifi"]);
    assert_eq!(at(result, &["state"]), &string("Online"));
    assert_eq!(at(result, &["association"]), &string("COMPLETED"));
    assert_eq!(at(result, &["status_query_failures"]), &Py::int(3));
    assert_eq!(
        at(result, &["signal_observation", "reason"]),
        &string("command_timeout")
    );
    assert_eq!(
        at(result, &["signal_observation", "monotonic_s"]),
        &Py::Float(100.0)
    );
    assert_eq!(at(result, &["events", "real_disconnects"]), &Py::int(1));
    assert_eq!(at(result, &["events", "reconnects"]), &Py::int(2));
    assert_eq!(
        at(result, &["events", "association_active"]),
        &Py::Bool(true)
    );
    assert_eq!(
        at(result, &["power_save", "requested_driver_profile"]),
        &string("CAM")
    );
    assert_eq!(at(result, &["power_save", "applied"]), &Py::Bool(false));
    assert_eq!(at(result, &["traffic_counters", "rx_errors"]), &Py::int(0));
    for private in [peer, "192.0.2.2", "private-lease"] {
        assert!(!dumped(result).contains(private), "{private}");
    }
    for key in [
        "state",
        "reason",
        "requested_driver_profile",
        "status_query_failures",
    ] {
        let value = Py::dict().with(key, string("Private peer"));
        assert_eq!(at(&project(&value, "", 0), &[key]), &string("[redacted]"));
    }
    assert_eq!(
        at(
            &projected(r#"{"power_save": {"requested_driver_profile": "FAST_PSP"}}"#),
            &["power_save", "requested_driver_profile"]
        ),
        &string("FAST_PSP")
    );
}

const DATA_UUID: &str = "79324c69-6e75-4801-8000-000000000102";
const MOUNTS: &str = "22 1 179:2 / / ro - ext4 /dev/mmcblk0p2 ro\n\
                      30 22 259:5 / /data rw,noatime - ext4 /dev/mmcblk0p9 rw\n";

/// A verified /data (tests/platform_cases/reports.py `fixture`): the eMMC's partition behind its
/// device number, its Y2Data UUID from blkid.
fn verified_data(fixture: &Fixture) {
    let device = "sys/devices/platform/11230000.mmc/mmc_host/mmc0/mmc0:aaaa/block/mmcblk0p9";
    fixture.put(&format!("/{device}/dev"), "259:5\n");
    fixture.mkdir("/sys/dev/block");
    std::os::unix::fs::symlink(
        format!("../../../{device}"),
        fixture.path("/sys/dev/block/259:5"),
    )
    .unwrap();
    fixture.put("/proc/self/mountinfo", MOUNTS);
    fixture.put("/proc/sys/kernel/random/boot_id", "boot-a\n");
    fixture.mkdir("/data/updates");
}

/// This root with /data's blkid answer.
fn answering(root: &Root) -> Root {
    root.clone().with_answers(vec![ok(
        &["blkid", "-p", "-s", "UUID", "-o", "value", "/dev/mmcblk0p9"],
        DATA_UUID,
    )])
}

fn exported(root: &Root) -> Result<Py, Failure> {
    export_inner(&answering(root))
}

fn names(directory: &std::path::Path, suffix: &str) -> Vec<String> {
    std::fs::read_dir(directory)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(suffix))
        .collect()
}

#[test]
fn archive_is_private_projected_and_mount_bound() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    verified_data(&fixture);
    let root = &fixture.root;
    let volume = data_volume(&answering(root)).unwrap();
    if !admitted(&volume, MAX_BYTES as u64) {
        // This host's /data (the temporary directory) has too little room: the space reserve refuses.
        let refused = exported(root).err().unwrap();
        assert_eq!(refused.message(), "diagnostic_space_reserved");
        return;
    }
    let result = exported(root).unwrap();
    let path = root.path(result.get("path").and_then(Py::as_str).unwrap());
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let members = tarball::members(&std::fs::read(&path).unwrap(), true, 64, |_| true).unwrap();
    let member_names: Vec<&str> = members.iter().map(|(m, _)| m.name.as_str()).collect();
    assert_eq!(member_names, ["diagnostics.json"]);
    let bundle = pyjson::loads_bytes(members[0].1.as_deref().unwrap()).unwrap();
    assert_eq!(bundle.get("redacted"), Some(&Py::Bool(true)));
    assert_eq!(result.get("redacted"), Some(&Py::Bool(true)));
    // Bound to the verified /data: without it nothing is written.
    fixture.put(
        "/proc/self/mountinfo",
        "22 1 179:2 / / ro - ext4 /dev/mmcblk0p2 ro\n",
    );
    let refused = exported(root).err().unwrap();
    assert_eq!(refused.message(), "verified_data_unavailable");
    let directory = path.parent().unwrap();
    assert_eq!(names(directory, ".tar.gz").len(), 1);
    assert!(names(directory, ".partial").is_empty());
}

#[test]
fn log_parent_symlink_is_rejected() {
    let fixture = Fixture::new();
    fixture.mkdir("/data");
    fixture.put("/secret/system.log", "usb error private");
    std::os::unix::fs::symlink(fixture.path("/secret"), fixture.path("/data/logs")).unwrap();
    let refused = log_tail(&fixture.root, "logs/system.log");
    assert!(matches!(refused, Err(Failure::Os { .. })), "{refused:?}");
}

#[test]
fn shared_public_observations_survive_without_private_values() {
    let private = "private-control-secret";
    let result = projected(&format!(
        r#"{{"bluetooth": {{"devices": [{{"bonded": true, "paired": false, "trusted": true,
                                          "address": "{private}", "name": "{private}"}}]}},
            "wifi": {{"state": "Online", "status_query_failures": 3,
                      "signal_observation": {{"ok": false, "reason": "command_timeout", "bssid": "{private}"}}}},
            "system": {{"usb": {{"file_transfer": {{
                "schema": "org.y2linux.usb-transfer/v1", "state": "Copying",
                "mode": "mtp", "filesystem_owner": "linux", "block_export": false,
                "active": true, "refresh_generation": 23,
                "views": [{{"name": "SD Card", "access": "rw", "path": "{private}", "cid": "{private}"}}]}}}}}},
            "sleep": {{"state": "waking", "same_boot": true}},
            "audio": {{"pcm_contract": {{"evidence": "driver_contract",
                      "physical_bit_capture": false, "hardware_max_meaningful_bits": null,
                      "supported_modes": [{{"meaningful_bits": 16, "i2s_slot_bits": 32}}]}}}},
            "spm": "suspend_restore_failures=0 private=secret"}}"#
    ));
    let Py::List(devices) = at(&result, &["bluetooth", "devices"]) else {
        panic!("{result:?}")
    };
    assert_eq!(at(&devices[0], &["bonded"]), &Py::Bool(true));
    assert_eq!(
        at(&result, &["wifi", "signal_observation", "reason"]),
        &string("command_timeout")
    );
    let usb = at(&result, &["system", "usb", "file_transfer"]);
    assert_eq!(at(usb, &["schema"]), &string("org.y2linux.usb-transfer/v1"));
    assert_eq!(at(usb, &["refresh_generation"]), &Py::int(23));
    assert_eq!(at(usb, &["active"]), &Py::Bool(true));
    let Py::List(views) = at(usb, &["views"]) else {
        panic!("{usb:?}")
    };
    assert_eq!(at(&views[0], &["name"]), &string("SD Card"));
    assert_eq!(at(&result, &["sleep", "state"]), &string("waking"));
    assert_eq!(
        at(&result, &["audio", "pcm_contract", "physical_bit_capture"]),
        &Py::Bool(false)
    );
    assert_eq!(
        at(&result, &["spm", "suspend_restore_failures"]),
        &string("0")
    );
    assert!(!dumped(&result).contains(private));
}

#[test]
fn bond_records_and_unknown_strings_remain_private() {
    for value in [
        r#""private-secret""#,
        r#"{"link_key": "private-secret"}"#,
        "[true]",
        "1",
    ] {
        let raw = Py::dict().with("bonded", pyjson::loads(value).unwrap());
        assert_eq!(
            at(&project(&raw, "", 0), &["bonded"]),
            &string("[redacted]")
        );
    }
    for value in [false, true] {
        let raw = Py::dict().with("bonded", Py::Bool(value));
        assert_eq!(at(&project(&raw, "", 0), &["bonded"]), &Py::Bool(value));
    }
    assert_eq!(
        at(
            &projected(r#"{"file_transfer": {"mode": "private-secret"}}"#),
            &["file_transfer", "mode"]
        ),
        &string("[redacted]")
    );
}

/// `pcm_contract_show` of kernel/audio/mt6582-afe.c as it prints idle, configured (44.1 kHz) and
/// running (48 kHz); tests/test_audio_precision_contract.py runs the driver function itself.
const PCM_CONTRACT: [&str; 3] = [
    r#"{"schema":"org.y2linux.pcm-contract/v1","evidence":"driver_contract","physical_bit_capture":false,"hardware_max_meaningful_bits":null,"supported_modes":[{"alsa_format":"S16_LE","container_bits":16,"meaningful_bits":16,"channels":2,"dma_format":"interleaved_s16_le","dl1_format":"s16_stereo","afe_format":"I05_I06_to_O00_O01_s16","afe_interconnect_physical_bits":null,"i2s_slot_bits":32,"i2s_payload_bits":16,"dac_input_word_bits":16,"rates_hz":[44100]}],"current":{"configured":false,"running":false,"rate_hz":0,"alsa_format":"NONE","container_bits":0,"meaningful_bits":0,"dma_format":"NONE","dl1_format":"NONE","afe_format":"NONE","i2s_slot_bits":0,"i2s_payload_bits":0,"dac_input_word_bits":0}}"#,
    r#"{"schema":"org.y2linux.pcm-contract/v1","evidence":"driver_contract","physical_bit_capture":false,"hardware_max_meaningful_bits":null,"supported_modes":[{"alsa_format":"S16_LE","container_bits":16,"meaningful_bits":16,"channels":2,"dma_format":"interleaved_s16_le","dl1_format":"s16_stereo","afe_format":"I05_I06_to_O00_O01_s16","afe_interconnect_physical_bits":null,"i2s_slot_bits":32,"i2s_payload_bits":16,"dac_input_word_bits":16,"rates_hz":[44100]}],"current":{"configured":true,"running":false,"rate_hz":44100,"alsa_format":"S16_LE","container_bits":16,"meaningful_bits":16,"dma_format":"interleaved_s16_le","dl1_format":"s16_stereo","afe_format":"I05_I06_to_O00_O01_s16","i2s_slot_bits":32,"i2s_payload_bits":16,"dac_input_word_bits":16}}"#,
    r#"{"schema":"org.y2linux.pcm-contract/v1","evidence":"driver_contract","physical_bit_capture":false,"hardware_max_meaningful_bits":null,"supported_modes":[{"alsa_format":"S16_LE","container_bits":16,"meaningful_bits":16,"channels":2,"dma_format":"interleaved_s16_le","dl1_format":"s16_stereo","afe_format":"I05_I06_to_O00_O01_s16","afe_interconnect_physical_bits":null,"i2s_slot_bits":32,"i2s_payload_bits":16,"dac_input_word_bits":16,"rates_hz":[44100]}],"current":{"configured":true,"running":true,"rate_hz":48000,"alsa_format":"S16_LE","container_bits":16,"meaningful_bits":16,"dma_format":"interleaved_s16_le","dl1_format":"s16_stereo","afe_format":"I05_I06_to_O00_O01_s16","i2s_slot_bits":32,"i2s_payload_bits":16,"dac_input_word_bits":16}}"#,
];

#[test]
fn idle_configured_and_running_driver_contract() {
    for line in PCM_CONTRACT {
        let row = pyjson::loads(line).unwrap();
        let raw = Py::dict().with("audio", Py::dict().with("pcm_contract", row.clone()));
        let exported = project(&raw, "", 0);
        let exported = at(&exported, &["audio", "pcm_contract"]);
        assert!(
            at(exported, &["current"]).py_eq(at(&row, &["current"])),
            "{exported:?}"
        );
        assert!(at(exported, &["supported_modes"]).py_eq(at(&row, &["supported_modes"])));
        assert_eq!(at(exported, &["evidence"]), &string("driver_contract"));
    }
}
