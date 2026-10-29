//! The crosscheck scenarios (see crosscheck.rs): each runs steps the Python platform ran the same way
//! and compares with what it gave (recorded).
// SPDX-License-Identifier: GPL-2.0-only
use super::crosscheck::{answer, check, player, refused};
use super::{bluetooth, daemon::Supervisor, observe_wifi, radio_policy, wifi};
use serde_json::{json, Value};
use std::fs;
use y2p_core::{
    clock,
    json::{Object, ObjectExt},
    Root,
};

const SCAN: &[&str] = &["wpa_cli", "-i", "wlan0", "scan_results"];
const LIST: &[&str] = &["wpa_cli", "-i", "wlan0", "list_networks"];

fn scan_answers() -> Vec<Value> {
    vec![
        answer(
            SCAN,
            "bssid / frequency / signal level / flags / ssid\n00:11:22:33:44:55\t2437\t-66\t[WPA2-PSK-CCMP][ESS]\tHome\n\
             aa:bb:cc:dd:ee:ff\t5180\t-66\t[WPA2-PSK-CCMP][ESS]\tHome\n11:22:33:44:55:66\t2462\t-50\t[ESS]\tCafe\n\
             22:33:44:55:66:77\tx\t-40\t[ESS]\tBad",
        ),
        answer(
            LIST,
            "network id / ssid / bssid / flags\n0\tHome\tany\t[CURRENT]\n1\tWork\tany\t[DISABLED]\n4\tHome\tany\t",
        ),
        answer(
            &["wpa_cli", "-i", "wlan0", "bss", "00:11:22:33:44:55"],
            "id=5\nbssid=00:11:22:33:44:55\nage=12\nlevel=-66",
        ),
        answer(
            &["wpa_cli", "-i", "wlan0", "get_network", "0", "key_mgmt"],
            "WPA-PSK",
        ),
        answer(
            &["wpa_cli", "-i", "wlan0", "get_network", "4", "key_mgmt"],
            "SAE WPA-PSK",
        ),
        answer(&["wpa_cli", "-i", "wlan0", "get", "country"], "DE"),
    ]
}

/// A step: an event line for the monitor, or a file to change between events.
fn events() -> Vec<Value> {
    let temp = |id: u32, reason: &str| {
        format!("<3>CTRL-EVENT-SSID-TEMP-DISABLED id={id} ssid=\"Home\" auth_failures=1 duration=10 reason={reason}")
    };
    vec![
        json!(temp(0, "WRONG_KEY")),
        json!(temp(0, "WRONG_KEY")),
        json!("<3>CTRL-EVENT-NETWORK-NOT-FOUND"),
        json!("<3>CTRL-EVENT-NETWORK-NOT-FOUND"),
        json!("<3>CTRL-EVENT-DISCONNECTED bssid=00:11:22:33:44:55 reason=3 locally_generated=1"),
        json!(temp(0, "WRONG_KEY")),
        json!("<3>Authentication with 00:11:22:33:44:55 timed out."),
        json!("<3>CTRL-EVENT-DISCONNECTED bssid=00:11:22:33:44:55 reason=15 locally_generated=1"),
        json!("<3>CTRL-EVENT-AUTH-REJECT 00:11:22:33:44:55 auth_type=0 auth_transaction=2 status_code=15"),
        json!("<3>CTRL-EVENT-SCAN-STARTED "),
        json!("<3>CTRL-EVENT-CONNECTED - Connection to 00:11:22:33:44:55 completed [id=1 id_str=]"),
        json!("<3>Trying to associate with 00:11:22:33:44:55 (SSID='Home' freq=2437 MHz)"),
        json!(temp(1, "CONN_FAILED")),
        json!(temp(1, "WRONG_KEY")),
        json!("CTRL-EVENT-SSID-TEMP-DISABLED id=1 reason=WRONG_KEY"),
        json!("<3>CTRL-EVENT-DISCONNECTED bssid=00:11:22:33:44:55 reason=123456"),
        json!({"write": "/sys/class/rfkill/rfkill0/soft", "text": "1\n"}),
        json!(temp(1, "WRONG_KEY")),
        json!("<3>CTRL-EVENT-DISCONNECTED bssid=00:11:22:33:44:55 reason=3 locally_generated=1"),
        json!("<3>CTRL-EVENT-SSID-REENABLED id=1 ssid=\"Home\""),
        json!("<3>CTRL-EVENT-NETWORK-NOT-FOUND"),
    ]
}

fn write(root: &Root, step: &Value) -> bool {
    let Some(path) = step.get("write").and_then(Value::as_str) else {
        return false;
    };
    fs::write(root.path(path), step["text"].as_str().unwrap()).unwrap();
    true
}

#[test]
fn supplicant_events_latch_a_wrong_password_as_python_does() {
    let steps = events();
    let outcome = check(
        "supplicant-events",
        &player(),
        json!(scan_answers()),
        |root| {
            let mut monitor = wifi::EventMonitor::new(root);
            let mut result = Vec::new();
            for step in &steps {
                if write(root, step) {
                    continue;
                }
                monitor.event(step.as_str().unwrap()).unwrap();
                result.push(json!([
                    root.json("/run/y2/wifi-failure.json"),
                    monitor.held
                ]));
            }
            result.push(
                json!({"summary": monitor.summary(None).into_sorted(), "held": monitor.held,
                           "connected": monitor.connected_network}),
            );
            Value::Array(result)
        },
    );
    if let Some(outcome) = outcome {
        // The F52 verdict Reborn reads: one failed handshake is not a wrong password, two are.
        assert_eq!(outcome.result[0][0]["error"], "handshake_failed");
        assert_eq!(outcome.result[1][0]["error"], "wrong_credentials");
        assert_eq!(
            outcome.result[1][0]["credential"]["error"],
            "wrong_credentials"
        );
        assert_eq!(
            outcome.result[2][0]["scan"]["saved_key_mgmt"],
            json!(["SAE WPA-PSK", "WPA-PSK"])
        );
    }
}

#[test]
fn the_self_heal_reselects_and_rescans_as_python_does() {
    let list = answer(
        LIST,
        "network id / ssid / bssid / flags\n0\tHome\tany\t\n1\tWork\tany\t[TEMP-DISABLED]\n2\tCafe\tany\t[DISABLED]\n3\tOffice\tany\t",
    );
    let scan = |rows: &str| {
        answer(
            SCAN,
            &format!("bssid / frequency / signal level / flags / ssid\n{rows}"),
        )
    };
    let wpa = |argv: &[&str], output: &str| {
        let mut full = vec!["wpa_cli", "-i", "wlan0"];
        full.extend_from_slice(argv);
        answer(&full, output)
    };
    let plan: &[(f64, &str, Option<&str>)] = &[
        (0.0, "DISCONNECTED", None),
        (31.0, "DISCONNECTED", None),
        (32.0, "DISCONNECTED", None),
        (62.0, "INACTIVE", None),
        (63.0, "SCANNING", None),
        (64.0, "DISCONNECTED", Some("wrong_credentials")),
        (65.0, "DISCONNECTED", None),
        (200.0, "DISCONNECTED", None),
        (201.0, "COMPLETED", None),
    ];
    for (name, rows) in [
        ("reselect", "a\t2437\t-60\t[ESS]\tHome\nb\t2437\t-50\t[ESS]\tWork\nc\t2437\t-50\t[ESS]\tOffice\nd\t2437\t-40\t[ESS]\tCafe"),
        ("rescan", "a\t2437\t-60\t[ESS]\tElsewhere"),
    ] {
        let answers = json!([list, scan(rows), wpa(&["select_network", "1"], "OK"),
                             wpa(&["enable_network", "all"], "FAIL"), wpa(&["scan"], "OK")]);
        let outcome = check(&format!("self-heal-{name}"), &player(), answers, |root| {
            let mut monitor = wifi::EventMonitor::new(root);
            monitor
                .event("<3>CTRL-EVENT-CONNECTED - Connection to x completed [id=2 id_str=]")
                .unwrap();
            let mut recovery = wifi::WifiRecovery::new(root);
            let result: Vec<Value> = plan
                .iter()
                .map(|(at, state, reason)| {
                    let value = Object::new()
                        .with("association", *state)
                        .with("radio_present", true)
                        .with("reason", json!(reason));
                    json!(recovery.observe(&value, 1000.0 + at, Some(&monitor)))
                })
                .collect();
            Value::Array(result)
        });
        if let Some(outcome) = outcome {
            let action = if name == "reselect" { "wifi_reselect" } else { "wifi_rescan" };
            assert_eq!(outcome.result[1], action, "{name}");
            let journal = &outcome.tree["data/system/platform/wifi-journal.jsonl"]["content"];
            assert!(journal[0].as_str().unwrap().contains(action), "{name}");
        }
    }
}

fn network_json(association: &str) -> String {
    json!({"schema": 1, "boot_id": "boot-a", "monotonic_s": clock::monotonic(),
           "association": association, "radio_present": true})
    .to_string()
}

#[test]
fn the_supervisor_renews_a_disabled_or_replaced_interface_as_python_does() {
    let mut files = player();
    files.push(("/sys/class/net/wlan0/ifindex", "3\n".into()));
    files.push(("/run/y2/network.json", network_json("INTERFACE_DISABLED")));
    let global = ["wpa_cli", "-g", "/run/wpa_supplicant/global"];
    let add: Vec<&str> = global
        .iter()
        .copied()
        .chain([
            "interface_add",
            "wlan0",
            "/data/network/wpa_supplicant.conf",
            "nl80211",
            "/run/wpa_supplicant",
        ])
        .collect();
    let answers = json!([
        answer(&add, "OK"),
        answer(
            &["wpa_cli", "-i", "wlan0", "autoscan", "exponential:2:120"],
            "OK"
        ),
        answer(
            &[
                "wpa_cli",
                "-i",
                "wlan0",
                "-a",
                "/usr/libexec/y2/wpa-event",
                "-B",
                "-P",
                "/run/y2/wifi-action.pid"
            ],
            ""
        )
    ]);
    let outcome = check("supervisor-renewal", &files, answers, |root| {
        let mut s = Supervisor::new(root);
        s.attached = true;
        s.ifindex = Some("3".into());
        let t0 = 1000.0;
        let mut result = Vec::new();
        s.verify(t0);
        s.verify(t0 + 29.0);
        result.push(json!(s.attached));
        s.verify(t0 + 31.0);
        result.push(json!([s.attached, s.renewals, s.next_renew - t0]));
        s.interface(t0 + 31.0);
        result.push(json!([s.attached, s.ifindex, s.retry_at - t0]));
        s.verify(t0 + 70.0);
        s.verify(t0 + 101.0);
        result.push(json!([s.attached, s.renewals, s.next_renew - t0]));
        s.interface(t0 + 101.0);
        fs::write(root.path("/sys/class/net/wlan0/ifindex"), "4\n").unwrap();
        s.interface(t0 + 102.0);
        result.push(json!([s.attached, s.ifindex, s.retry_at - t0]));
        fs::write(root.path("/sys/class/rfkill/rfkill0/soft"), "1\n").unwrap();
        s.verify(t0 + 300.0);
        s.verify(t0 + 400.0);
        result.push(json!([s.attached, s.renewals]));
        fs::remove_dir_all(root.path("/sys/class/net/wlan0")).unwrap();
        s.interface(t0 + 401.0);
        result.push(json!([s.attached, s.ifindex, s.retry_at == 0.0]));
        Value::Array(result)
    });
    if let Some(outcome) = outcome {
        let rows = &outcome.tree["data/system/platform/wifi-journal.jsonl"]["content"];
        assert_eq!(rows.as_array().unwrap().len(), 3, "{rows}");
        assert!(rows[2].as_str().unwrap().contains("new_netdev"));
    }
}

fn lease_answers(dns: &str) -> Vec<Value> {
    vec![
        answer(
            &["wpa_cli", "-i", "wlan0", "status"],
            "bssid=00:11:22:33:44:55\nfreq=2437\nssid=Home\nid=0\nmode=station\nwpa_state=COMPLETED\nip_address=192.168.1.40",
        ),
        answer(
            &["ip", "-j", "-4", "addr", "show", "dev", "wlan0"],
            r#"[{"ifname": "wlan0", "addr_info": [{"family": "inet", "local": "192.168.1.40", "prefixlen": 24, "scope": "global"}]}]"#,
        ),
        answer(
            &["ip", "-j", "-4", "route", "show", "default", "dev", "wlan0"],
            r#"[{"dst": "default", "gateway": "192.168.1.1", "dev": "wlan0"}]"#,
        ),
        answer(
            &["wpa_cli", "-i", "wlan0", "signal_poll"],
            "RSSI=-60\nLINKSPEED=72\nNOISE=9999\nFREQUENCY=2437",
        ),
        answer(
            &["getaddrinfo", "pool.ntp.org"],
            dns,
        ),
    ]
}

#[test]
fn the_dns_probe_confirms_a_lease_as_python_does() {
    let mut files = player();
    files.push(("/sys/class/net/wlan0/ifindex", "3\n".into()));
    files.push(("/sys/class/net/wlan0/statistics/rx_bytes", "1000\n".into()));
    files.push(("/sys/class/net/wlan0/statistics/tx_bytes", "x\n".into()));
    files.push((
        "/run/y2/dhcp.json",
        json!({"boot_id": "boot-a", "epoch": "b6a2f0de-6a1c-4b7e-9c0e-3f1d2a4b5c6d", "error": null}).to_string(),
    ));
    for (dns, policy) in [
        ("3", None),
        ("0", None),
        ("x", Some("{\"dns_probe_host\": \"bad host\"}")),
    ] {
        let mut files = files.clone();
        if let Some(policy) = policy {
            files.push(("/etc/y2linux/network-policy.json", policy.into()));
        }
        check(
            &format!("dns-probe-{dns}"),
            &files,
            json!(lease_answers(dns)),
            |root| {
                let v = observe_wifi::wifi(root, None);
                let mut s = wifi::DnsProbeSchedule::new();
                let epoch = root.read("/run/y2/wifi-epoch");
                let due = s.due(&v, epoch.as_deref(), 100.0);
                let dns = match wifi::probe_dns(root, &v) {
                    Ok(dns) => json!(dns.map(|d| Value::Object(y2p_core::json::sorted_object(&d)))),
                    Err(error) => json!(error.message()),
                };
                s.confirmed(100.0);
                let value = |o: Object| Value::Object(y2p_core::json::sorted_object(&o));
                let again = s.due(&v, epoch.as_deref(), 101.0);
                let after = observe_wifi::wifi(root, Some(&v));
                json!([
                    value(v.clone()),
                    due,
                    dns,
                    again,
                    value(after),
                    s.due(&v, epoch.as_deref(), 401.0)
                ])
            },
        );
    }
}

#[test]
fn power_save_with_a_heavy_transfer_as_python_does() {
    let mut files = player();
    files.push((
        "/data/network/power-save.json",
        "{\"schema\": 1, \"mode\": \"automatic\", \"automatic_standard\": true}".into(),
    ));
    let get = ["/usr/sbin/iw", "dev", "wlan0", "get", "power_save"];
    for heavy in [true, false] {
        let answers = json!([
            answer(&get, "Power save: on"),
            answer(
                &["/usr/sbin/iw", "dev", "wlan0", "set", "power_save", "off"],
                ""
            ),
            refused(&["/usr/sbin/iw", "dev", "wlan0", "set", "power_save", "on"])
        ]);
        check(
            &format!("power-save-heavy-{heavy}"),
            &files,
            answers,
            |root| {
                let activity =
                    json!({"wifi_heavy_transfer": if heavy { json!(true) } else { json!(0) }});
                let value = radio_policy::wifi_policy(root, activity.as_object()).unwrap();
                Value::Object(y2p_core::json::sorted_object(&value))
            },
        );
    }
}

fn observation(pcm: Value, extra: Value) -> String {
    let mut raw = json!({
        "schema": 1, "stable_owners": true, "bluez_owner": ":1.2", "bluealsa_owner": ":1.3",
        "manager": {"Version": "v5.0.0", "Codecs": ["A2DP-source:SBC", "A2DP-source:AAC", "a2dp-sink:SBC", 3]},
        "codec_pcm": "/org/bluealsa/hci0/dev_AA/a2dpsrc/sink",
        "mutually_available_codecs": {"SBC": {"rates": [44100]}}});
    for (key, value) in extra.as_object().unwrap() {
        raw[key] = value.clone();
    }
    // Hand-built text so the device order is not alphabetical (BB before AA).
    let bluez = r#""bluez": {"/org/bluez/hci0": {"org.bluez.Adapter1": {"Powered": true, "Discovering": false, "Pairable": true, "Discoverable": false}},
        "/org/bluez/hci0/dev_BB": {"org.bluez.Device1": {"Address": "BB:BB:BB:BB:BB:BB", "Paired": true, "Bonded": true, "Trusted": true, "Connected": false, "ServicesResolved": false}},
        "/org/bluez/hci0/dev_AA": {"org.bluez.Device1": {"Address": "AA:AA:AA:AA:AA:AA", "Paired": true, "Bonded": true, "Trusted": true, "Connected": true, "ServicesResolved": true}},
        "/org/bluez/hci0/other": 5}"#;
    let bluealsa = y2p_core::json::sorted(
        json!({"/org/bluealsa/hci0/dev_AA/a2dpsrc/sink": {"org.bluealsa.PCM1": pcm}}),
    );
    let text = y2p_core::json::sorted(raw).to_string();
    let text = text.trim_end_matches('}');
    if extra.get("bluez").is_some() {
        format!("{text}, \"bluealsa\": {bluealsa}}}")
    } else {
        format!("{text}, {bluez}, \"bluealsa\": {bluealsa}}}")
    }
}

#[test]
fn bluetooth_observations_normalise_as_python_does() {
    let pcm = |codec: &str, rate: u32, config: Value, stats: Value| {
        json!({"Device": "/org/bluez/hci0/dev_AA", "Transport": "A2DP-source", "Mode": "sink",
               "Format": 33296, "Rate": rate, "Channels": 2, "Codec": codec, "Running": true, "Sequence": 4,
               "CodecConfiguration": config, "EncoderStats": stats})
    };
    let sbc_stats = json!({"Active": 1, "Bitpool": 47, "AbrEnabled": 1, "AbrAdjustments": 3, "BitrateKbps": 552, "Errors": 0});
    let ldac_stats = json!({"Active": true, "QualityIndex": 1, "AbrEnabled": 1, "AbrAdjustments": 2, "BitrateKbps": 660});
    let cases: Vec<(String, Vec<(&str, String)>)> = vec![
        (
            observation(
                pcm("SBC", 44100, json!([36, 21, 2, 53]), sbc_stats.clone()),
                json!({}),
            ),
            vec![],
        ),
        (
            observation(
                pcm("SBC", 44100, json!([33, 21, 2, 256]), sbc_stats.clone()),
                json!({}),
            ),
            vec![],
        ),
        (
            observation(pcm("LDAC", 96000, json!([]), ldac_stats.clone()), json!({})),
            vec![],
        ),
        (
            observation(
                pcm(
                    "AAC",
                    48000,
                    json!(null),
                    json!({"Active": 1, "QualityIndex": 2, "AbrEnabled": 0}),
                ),
                json!({"manager": {"Version": "4.3.1", "Codecs": ["A2DP-source:AAC"]}}),
            ),
            vec![],
        ),
        (
            observation(
                json!({"Device": "/org/bluez/hci0/dev_AA", "Transport": "A2DP-source", "Mode": "sink",
                            "Format": true, "Rate": 7000.0, "Channels": 2.0}),
                json!({}),
            ),
            vec![],
        ),
        (
            observation(
                pcm("SBC", 44100, json!([]), json!({})),
                json!({"bluealsa_owner": ""}),
            ),
            vec![],
        ),
        (
            observation(
                pcm("SBC", 44100, json!([]), json!({})),
                json!({"bluez": {"/a": {"org.bluez.Adapter1": {"Powered": false}}}}),
            ),
            vec![],
        ),
        (
            observation(
                pcm("SBC", 44100, json!([]), json!({})),
                json!({"bluez": {}}),
            ),
            vec![],
        ),
        (
            observation(pcm("SBC", 44100, json!([]), json!({})), json!({})),
            vec![(
                "/data/bluetooth/preferred-audio",
                "bb:bb:bb:bb:bb:bb\n".into(),
            )],
        ),
        (
            observation(pcm("SBC", 44100, json!([]), json!({})), json!({})),
            vec![(
                "/data/bluetooth/codec-policy.json",
                "{\"schema\": 1, \"sbc_quality\": \"max\"}".into(),
            )],
        ),
        (
            observation(
                pcm(
                    "SBC",
                    44100,
                    json!([36, 21, 2, 38]),
                    json!({"Active": 1, "Bitpool": 38}),
                ),
                json!({}),
            ),
            vec![
                ("/run/y2/bluealsa.pid", "4242\n".into()),
                (
                    "/proc/4242/cmdline",
                    "/usr/bin/bluealsad\0-p\0a2dp-source\0--sbc-quality=xq\0".into(),
                ),
                (
                    "/run/y2/codec-runtime.json",
                    json!({"boot_id": "boot-a", "arguments": ["-p", "--sbc-quality=xq"],
                "settings": {"schema": 1, "experimental": false, "sbc_quality": "xq"}})
                    .to_string(),
                ),
                (
                    "/run/y2/bt-reconnect.json",
                    json!({"boot_id": "boot-a", "state": "Idle",
                "monotonic_us": (clock::monotonic() * 1e6) as u64})
                    .to_string(),
                ),
            ],
        ),
        ("{\"schema\": 1, \"stable_owners\": false}".into(), vec![]),
        ("[1, 2]".into(), vec![]),
    ];
    for (index, (raw, extra)) in cases.into_iter().enumerate() {
        let mut files = player();
        files.extend(extra);
        let outcome = check(
            &format!("bluetooth-normalize-{index}"),
            &files,
            json!([]),
            |root| bluetooth::normalize(root, &serde_json::from_str(&raw).unwrap()).unwrap(),
        );
        if let Some(outcome) = outcome {
            if raw.contains("dev_BB") && raw.contains("[36, 21, 2, 53]") {
                assert_eq!(outcome.result["state"], "Ready", "{}", outcome.result);
                assert_eq!(outcome.result["devices"][0]["address"], "BB:BB:BB:BB:BB:BB");
                assert_eq!(outcome.result["encoder"]["sbc_quality"], "xq+");
            }
        }
    }
}

#[test]
fn python_rounding_to_one_digit() {
    for (value, rounded) in [
        (0.25, 0.2),
        (0.35, 0.3),
        (2.675, 2.7),
        (-0.04, -0.0),
        (31.0, 31.0),
        (1.05, 1.1),
    ] {
        assert_eq!(
            wifi::round1(value).to_bits(),
            f64::to_bits(rounded),
            "{value}"
        );
    }
    // Ages and dwell times as the journal rows carry them: round(x, 1) as CPython printed it
    // (recorded from host Python 3.15).
    let values: Vec<f64> = (0..4000)
        .map(|i| f64::from(i) * 0.05 + f64::from(i % 7) * 1e-9 - 3.0)
        .chain((0..2000).map(|i| 1e5 + f64::from(i) * 0.123_456_7))
        .collect();
    let ours: String = values
        .iter()
        .map(|v| y2p_core::json::float_repr(wifi::round1(*v)) + "\n")
        .collect();
    y2p_core::recorded::check(env!("CARGO_MANIFEST_DIR"), "round1.txt", ours.as_bytes());
}
