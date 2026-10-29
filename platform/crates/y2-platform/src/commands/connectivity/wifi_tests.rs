//! The supplicant event monitor, the DNS probe and its schedule and the F52 self-heal: what
//! tests/test_platform_network_time.py and tests/test_wifi_failure_evidence.py checked of the Python
//! platform (network.py).
// SPDX-License-Identifier: GPL-2.0-only
use super::super::observe_wifi::{wifi_state, DNS_FRESH_S};
use super::*;
use crate::commands::network::wifi_event;
use crate::testing::{ok, scripted, Fixture};
use serde_json::Map;
use std::time::Instant;
use y2p_core::{command::Outcome, json::sorted_object};

type Answer = (Vec<String>, Outcome);

fn wpa_answer(args: &[&str], output: &str) -> Answer {
    let mut argv = vec!["wpa_cli", "-i", "wlan0"];
    argv.extend_from_slice(args);
    ok(&argv, output)
}

fn ordered(value: Value) -> Object {
    sorted_object(value.as_object().expect("object"))
}

fn player() -> Fixture {
    let fixture = Fixture::new();
    fixture.put("/proc/sys/kernel/random/boot_id", "boot-one");
    fixture
}

/// The record of the last supplicant failure, if there is one.
fn failure(fixture: &Fixture) -> Option<Value> {
    fixture
        .exists("/run/y2/wifi-failure.json")
        .then(|| fixture.json("/run/y2/wifi-failure.json"))
}

const LOOKUP: [&str; 5] = [
    "/usr/bin/python3",
    "-I",
    "-c",
    "import socket,sys; print(len(socket.getaddrinfo(sys.argv[1],None,socket.AF_INET)))",
    "pool.ntp.org",
];
const ADDRESS: [&str; 7] = ["ip", "-j", "-4", "addr", "show", "dev", "wlan0"];
const ROUTE: [&str; 8] = ["ip", "-j", "-4", "route", "show", "default", "dev", "wlan0"];

#[test]
fn probe_rejects_peer_disappearance_and_reports_auth_reason_without_ssid() {
    let fixture = player();
    wifi_event(&fixture.root, "CONNECTED").unwrap();
    fixture.mkdir("/sys/class/net/wlan0");
    let before = ordered(json!({"ip_addresses": ["192.0.2.2"], "default_route": [{}],
                                "association_id": "peer-one"}));
    // What the look after the lookup sees.
    let after = |bssid: &str, route: &str| {
        vec![
            wpa_answer(&["status"], &format!("wpa_state=COMPLETED\nbssid={bssid}")),
            ok(
                &ADDRESS,
                r#"[{"addr_info":[{"local":"192.0.2.2","scope":"global"}]}]"#,
            ),
            ok(&ROUTE, route),
            ok(&LOOKUP, "1"),
        ]
    };
    let probe = |answers| scripted(answers, || probe_dns(&fixture.root, &before).unwrap()).0;
    assert!(probe(after("peer-two", "[{}]")).is_none());
    assert!(!fixture.exists("/run/y2/dns.json"));
    assert!(probe(after("peer-one", "[]")).is_none());
    let probed = probe(after("peer-one", "[{}]")).expect("published");
    assert_eq!(probed.get("ok"), Some(&json!(true)));
    let mut monitor = EventMonitor::new(&fixture.root);
    let wrong = "<3>CTRL-EVENT-SSID-TEMP-DISABLED id=0 ssid=\"private-name\" reason=WRONG_KEY";
    monitor.event(wrong).unwrap();
    assert_eq!(failure(&fixture).unwrap()["error"], "handshake_failed");
    monitor.event(wrong).unwrap();
    let record = failure(&fixture).unwrap();
    assert_eq!(record["error"], "wrong_credentials");
    assert!(!record.to_string().contains("private-name"));
}

const NOT_FOUND: &str = "<3>CTRL-EVENT-NETWORK-NOT-FOUND";
const SCAN_HEADER: &str = "bssid / frequency / signal level / flags / ssid\n";

#[test]
fn network_not_found_records_what_the_scan_saw_without_names() {
    // F13 (70cce0f1): after a wake the saved network was "not found" for minutes; nothing said whether
    // it was out of range, on a passive channel or not seen.
    let fixture = player();
    let answers = |scan: &str| {
        vec![
            wpa_answer(&["scan_results"], scan),
            wpa_answer(
                &["list_networks"],
                "network id / ssid / bssid / flags\n0\thome-net\tany\t[CURRENT]\n",
            ),
            wpa_answer(&["get", "country"], "DE\n"),
        ]
    };
    let scan = format!(
        "{SCAN_HEADER}aa:bb:cc:dd:ee:01\t2462\t-71\t[WPA2-PSK-CCMP][ESS]\thome-net\n\
         aa:bb:cc:dd:ee:02\t5180\t-80\t[WPA2-PSK-CCMP][ESS]\thome-net\n\
         aa:bb:cc:dd:ee:03\t2437\t-50\t[ESS]\tneighbour\n"
    );
    let mut monitor = EventMonitor::new(&fixture.root);
    scripted(answers(&scan), || monitor.event(NOT_FOUND).unwrap());
    let record = failure(&fixture).unwrap();
    assert_eq!(record["error"], "ap_unavailable");
    assert_eq!(
        record["scan"],
        json!({"available": true, "networks": 3, "saved_networks": 1, "band_2ghz": 2,
               "band_5ghz": 1, "saved_seen": true, "saved_signal_dbm": -71,
               "saved_frequency_mhz": 2462, "saved_channel": 11, "saved_access_points": 2,
               "country": "DE", "saved_flags": "[WPA2-PSK-CCMP][ESS]", "saved_key_mgmt": []})
    );
    let text = record.to_string();
    for private in ["home-net", "neighbour", "aa:bb"] {
        assert!(!text.contains(private), "{private}");
    }
    // Reported after every scan: wpa_cli is asked at most every 30 s.
    let (_, calls) = scripted(answers(&scan), || monitor.event(NOT_FOUND).unwrap());
    assert!(calls.is_empty());
    let record = failure(&fixture).unwrap();
    assert_eq!(record["failures"], 2);
    assert_eq!(record["scan"]["saved_seen"], true);
    // 31 s later (the record's scan that much older).
    let mut record = record;
    record["scan_monotonic_s"] = json!(record["scan_monotonic_s"].as_f64().unwrap() - 31.0);
    fixture.put_json("/run/y2/wifi-failure.json", &record);
    scripted(answers(SCAN_HEADER), || monitor.event(NOT_FOUND).unwrap());
    let scan = &failure(&fixture).unwrap()["scan"];
    assert_eq!(
        (&scan["networks"], &scan["saved_seen"]),
        (&json!(0), &json!(false))
    );
    assert!(scan.get("saved_channel").is_none());
}

fn link(association: Option<&str>, id: &str, addresses: &[&str], route: bool) -> Object {
    let route = if route { json!([{}]) } else { json!([]) };
    ordered(json!({"association": association, "association_id": id,
                   "ip_addresses": addresses, "default_route": route}))
}

#[test]
fn new_lease_dns_does_not_inherit_old_probe_deadline() {
    let mut schedule = DnsProbeSchedule::new();
    let up = link(Some("COMPLETED"), "one", &["192.0.2.2"], true);
    assert!(schedule.due(&up, Some("first"), 100.0));
    assert!(!schedule.due(&up, Some("first"), 101.0));
    // Association without DHCP cannot consume the new probe opportunity.
    let unaddressed = link(Some("COMPLETED"), "one", &[], true);
    assert!(!schedule.due(&unaddressed, Some("second"), 102.0));
    assert!(schedule.due(&up, Some("second"), 103.0));
    assert!(!schedule.due(&up, Some("second"), 104.0));
    assert!(schedule.due(&up, Some("second"), 133.0));
    let other_peer = link(Some("COMPLETED"), "two", &["192.0.2.2"], true);
    assert!(schedule.due(&other_peer, Some("second"), 134.0));
    let unassociated = link(None, "one", &["192.0.2.2"], true);
    assert!(!schedule.due(&unassociated, Some("second"), 135.0));
    let other_address = link(Some("COMPLETED"), "one", &["192.0.2.3"], true);
    assert!(schedule.due(&other_address, Some("second"), 136.0));
    let unrouted = link(Some("COMPLETED"), "one", &["192.0.2.2"], false);
    assert!(!schedule.due(&unrouted, Some("third"), 137.0));
}

#[test]
fn confirmed_dns_is_rechecked_every_five_minutes_not_every_30_s() {
    // Live Beta 1: the probe re-ran every 30 s forever after success.
    let mut schedule = DnsProbeSchedule::new();
    let up = link(Some("COMPLETED"), "one", &["192.0.2.2"], true);
    assert!(schedule.due(&up, Some("lease"), 100.0));
    schedule.confirmed(100.0);
    assert!(!schedule.due(&up, Some("lease"), 131.0));
    assert!(!schedule.due(&up, Some("lease"), 399.0));
    assert!(schedule.due(&up, Some("lease"), 400.0));
    // A failed re-check retries after 30 s; freshness outlasts one miss.
    assert!(!schedule.due(&up, Some("lease"), 429.0));
    assert!(schedule.due(&up, Some("lease"), 430.0));
    const { assert!(DNS_FRESH_S > DnsProbeSchedule::CONFIRMED_S + DnsProbeSchedule::RETRY_S) };
    // A new lease is still probed at once.
    assert!(schedule.due(&up, Some("next"), 431.0));
}

#[test]
fn event_wait_wakes_for_real_supplicant_event() {
    let fixture = player();
    let mut monitor = EventMonitor::new(&fixture.root);
    let (socket, sender) = UnixDatagram::pair().unwrap();
    monitor.sock = Some(socket);
    let timer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        sender.send(b"event").unwrap();
        sender
    });
    let started = Instant::now();
    monitor.wait(2.0, None);
    let _sender = timer.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
    let mut buffer = [0u8; 16];
    let n = monitor.sock.as_ref().unwrap().recv(&mut buffer).unwrap();
    assert_eq!(&buffer[..n], b"event");
    monitor.close();
}

// ---- F31/F52: tests/test_wifi_failure_evidence.py ----------------------------------------------

/// A monitor whose wpa_cli answers as the player near one saved network in range.
struct Evidence {
    fixture: Fixture,
    monitor: EventMonitor,
}

impl Evidence {
    fn new() -> Self {
        let fixture = player();
        fixture.put("/run/y2/wifi-epoch", "3");
        let monitor = EventMonitor::new(&fixture.root);
        Self { fixture, monitor }
    }

    fn event(&mut self, text: &str) {
        let answers = vec![
            wpa_answer(
                &["scan_results"],
                &format!(
                    "{SCAN_HEADER}aa:bb:cc:dd:ee:01\t2462\t-71\t[WPA2-PSK-CCMP][ESS]\thome-net\n"
                ),
            ),
            wpa_answer(
                &["list_networks"],
                "network id / ssid / bssid / flags\n0\thome-net\tany\t[CURRENT]\n",
            ),
            wpa_answer(&["get", "country"], "DE\n"),
            wpa_answer(
                &["bss", "aa:bb:cc:dd:ee:01"],
                "id=3\nbssid=aa:bb:cc:dd:ee:01\nage=142\nssid=home-net\n",
            ),
            wpa_answer(&["get_network", "0", "key_mgmt"], "WPA-PSK\n"),
        ];
        let monitor = &mut self.monitor;
        scripted(answers, || monitor.event(text).unwrap());
    }

    fn record(&self) -> Option<Value> {
        failure(&self.fixture)
    }
}

const CONNECTED: &str =
    "<3>CTRL-EVENT-CONNECTED - Connection to aa:bb:cc:dd:ee:01 completed [id=0 id_str=]";

#[test]
fn reason_is_kept_without_names() {
    let mut wifi = Evidence::new();
    wifi.event(
        "<3>CTRL-EVENT-SSID-TEMP-DISABLED id=0 ssid=\"home-net\" auth_failures=1 duration=10 \
         reason=CONN_FAILED",
    );
    let record = wifi.record().unwrap();
    assert_eq!(
        (&record["error"], &record["reason"]),
        (&json!("authentication_failed"), &json!("CONN_FAILED"))
    );
    wifi.event(
        "<3>CTRL-EVENT-AUTH-REJECT aa:bb:cc:dd:ee:01 auth_type=0 auth_transaction=2 status_code=15",
    );
    let record = wifi.record().unwrap();
    assert_eq!(record["reason"], "15");
    assert!(!record.to_string().contains("home-net"));
    assert!(!record.to_string().contains("aa:bb"));
}

#[test]
fn last_failure_survives_connects() {
    let mut wifi = Evidence::new();
    wifi.event(NOT_FOUND);
    wifi.event(CONNECTED);
    let record = wifi.record().unwrap();
    assert!(record["error"].is_null());
    let last = &record["last_failure"];
    assert_eq!(
        json!([last["error"], last["failures"], last["boot_id"]]),
        json!(["ap_unavailable", 1, "boot-one"])
    );
    assert_eq!(last["scan"]["saved_seen"], true);
    // F13 (cb904ffb): seen at a good level and still "not found": entry age and security of the hit
    // vs the saved network, no name or address.
    assert_eq!(
        json!([
            last["scan"]["saved_age_s"],
            last["scan"]["saved_flags"],
            last["scan"]["saved_key_mgmt"]
        ]),
        json!([142, "[WPA2-PSK-CCMP][ESS]", ["WPA-PSK"]])
    );
    assert!(!record.to_string().contains("home-net"));
    assert!(!record.to_string().contains("aa:bb"));
    // A second connect keeps it; a new failure replaces it at the next connect.
    wifi.event(CONNECTED);
    assert_eq!(
        wifi.record().unwrap()["last_failure"]["error"],
        "ap_unavailable"
    );
    wifi.event("<3>CTRL-EVENT-SSID-TEMP-DISABLED id=0 ssid=\"home-net\" reason=WRONG_KEY");
    assert_eq!(
        wifi.record().unwrap()["last_failure"]["error"],
        "ap_unavailable"
    );
    wifi.event(CONNECTED);
    let last = &wifi.record().unwrap()["last_failure"];
    assert_eq!(
        json!([last["error"], last["reason"]]),
        json!(["handshake_failed", "WRONG_KEY"])
    );
}

#[test]
fn a_wrong_password_stays_reported_until_a_connect() {
    // Y2Reborn #20: one wrong letter, 15 tries, the player said "Connecting…" and "Saved", never why.
    // On the owner's player (8.27) each try ended in WRONG_KEY, then "not found" while the network was
    // disabled for a while.
    let mut wifi = Evidence::new();
    // F52: one failed handshake is no verdict; the second without a connect is.
    wifi.event(
        "<3>CTRL-EVENT-SSID-TEMP-DISABLED id=7 ssid=\"home-net\" auth_failures=1 duration=10 \
         reason=WRONG_KEY",
    );
    assert_eq!(wifi.record().unwrap()["error"], "handshake_failed");
    assert!(wifi.record().unwrap().get("credential").is_none());
    wifi.event(NOT_FOUND);
    wifi.event("<3>Authentication with aa:bb:cc:dd:ee:01 timed out.");
    wifi.event("<3>CTRL-EVENT-DISCONNECTED bssid=aa:bb:cc:dd:ee:01 reason=3 locally_generated=1");
    wifi.event(
        "<3>CTRL-EVENT-SSID-TEMP-DISABLED id=7 ssid=\"home-net\" auth_failures=2 duration=20 \
         reason=WRONG_KEY",
    );
    wifi.event(NOT_FOUND);
    let record = wifi.record().unwrap();
    assert_eq!(record["error"], "ap_unavailable");
    assert_eq!(
        json!([
            record["credential"]["error"],
            record["credential"]["network_id"]
        ]),
        json!(["wrong_credentials", 7])
    );
    assert!(!record.to_string().contains("home-net"));
    let wpa = |state: &str, failure: &Value| -> Map<String, Value> {
        json!({"wpa_state": state, "credential_failure": failure})
            .as_object()
            .cloned()
            .unwrap()
    };
    for state in ["SCANNING", "4WAY_HANDSHAKE", "DISCONNECTED"] {
        assert_eq!(
            wifi_state(
                true,
                &wpa(state, &record["credential"]["error"]),
                &[],
                &json!([]),
                false,
                None
            ),
            (json!("Failed"), json!("wrong_credentials"))
        );
    }
    assert_eq!(
        wifi_state(
            true,
            &wpa("COMPLETED", &json!("wrong_credentials")),
            &[json!("192.0.2.2")],
            &json!([{}]),
            true,
            None
        ),
        (json!("Online"), Value::Null)
    );
    wifi.event(
        "<3>CTRL-EVENT-CONNECTED - Connection to aa:bb:cc:dd:ee:01 completed [id=7 id_str=]",
    );
    assert!(wifi.record().unwrap().get("credential").is_none());
    // Another kind of failure is not a password.
    wifi.event(NOT_FOUND);
    assert!(wifi.record().unwrap().get("credential").is_none());
    for event in [
        "<3>CTRL-EVENT-SSID-TEMP-DISABLED id=7 ssid=\"home-net\" reason=CONN_FAILED",
        "<3>CTRL-EVENT-AUTH-REJECT aa:bb:cc:dd:ee:01 auth_type=0 status_code=1",
    ] {
        wifi.event(event);
        wifi.event(event);
        assert!(wifi.record().unwrap().get("credential").is_none());
    }
}

#[test]
fn a_handshake_cut_short_here_is_no_password_verdict() {
    // F52 (aa96e00a): wpa_supplicant says WRONG_KEY for any handshake that ends early. 950a93ac: the
    // sleep's radio off cut one at 246 s and the player said "wrong password" until the owner joined
    // by hand.
    let mut wifi = Evidence::new();
    let local = "<3>CTRL-EVENT-DISCONNECTED bssid=aa:bb:cc:dd:ee:01 reason=3 locally_generated=1";
    let wrong =
        "<3>CTRL-EVENT-SSID-TEMP-DISABLED id=4 ssid=\"home-net\" auth_failures=1 duration=10 \
                 reason=WRONG_KEY";
    for _ in 0..3 {
        wifi.event(local);
        wifi.event(wrong);
    }
    assert!(wifi.record().is_none());
    assert_eq!(wifi.monitor.interrupted_handshakes, 3);
    // A blocked radio (a missed disconnect event) is ours too.
    wifi.fixture
        .put("/sys/class/rfkill/rfkill1/type", "wlan\n")
        .put("/sys/class/rfkill/rfkill1/soft", "1\n");
    wifi.event(wrong);
    assert!(wifi.record().is_none());
    wifi.fixture.put("/sys/class/rfkill/rfkill1/soft", "0\n");
    // The access point's disconnect counts; a connect starts the count again.
    wifi.event("<3>CTRL-EVENT-DISCONNECTED bssid=aa:bb:cc:dd:ee:01 reason=15");
    wifi.event(wrong);
    let record = wifi.record().unwrap();
    assert_eq!(
        json!([record["error"], record["wrong_key"]["count"]]),
        json!(["handshake_failed", 1])
    );
    wifi.event(
        "<3>CTRL-EVENT-CONNECTED - Connection to aa:bb:cc:dd:ee:01 completed [id=4 id_str=]",
    );
    assert!(wifi.record().unwrap().get("wrong_key").is_none());
    wifi.event(wrong);
    assert_eq!(wifi.record().unwrap()["error"], "handshake_failed");
    // Another network's failure starts its own count.
    let other = wrong.replace("id=4", "id=5");
    wifi.event(&other);
    assert_eq!(wifi.record().unwrap()["error"], "handshake_failed");
    wifi.event(&other);
    let record = wifi.record().unwrap();
    assert_eq!(
        json!([record["error"], record["credential"]["network_id"]]),
        json!(["wrong_credentials", 5])
    );
    let summary = wifi.monitor.summary(None).into_sorted();
    assert_eq!(
        json!([
            summary["counts"]["DISCONNECTED"],
            summary["counts"]["SSID-TEMP-DISABLED"]
        ]),
        json!([4, 8])
    );
    assert_eq!(
        summary["last_disconnect"],
        json!({"reason": 15, "local": false, "timeout": false})
    );
    assert!(!summary.to_string().contains("aa:bb"));
}

// ---- F52 self-heal ---------------------------------------------------------------------------

/// A player with its /data verified (journal rows are written) and two saved networks in range.
struct Recovery {
    fixture: Fixture,
    recovery: WifiRecovery,
    monitor: EventMonitor,
    scan: String,
    /// The wpa_cli requests after `wpa_cli -i wlan0`.
    calls: Vec<Vec<String>>,
}

impl Recovery {
    fn new() -> Self {
        let fixture = player();
        fixture
            .put("/data/.y2data-schema", "1\n")
            .put("/run/y2-data-device", "/dev/mmcblk0p9\n")
            .put(
                "/proc/self/mountinfo",
                "30 22 179:9 / /data rw - ext4 /dev/mmcblk0p9 rw\n",
            );
        let recovery = WifiRecovery::new(&fixture.root);
        let monitor = EventMonitor::new(&fixture.root);
        Self {
            fixture,
            recovery,
            monitor,
            scan: format!(
                "{SCAN_HEADER}aa:bb:cc:dd:ee:01\t2462\t-61\t[WPA2-PSK-CCMP][ESS]\thome-net\n\
                 aa:bb:cc:dd:ee:02\t2412\t-50\t[WPA2-PSK-CCMP][ESS]\tcafe\n"
            ),
            calls: Vec::new(),
        }
    }

    fn answers(&self) -> Vec<Answer> {
        let mut answers = vec![
            wpa_answer(
                &["list_networks"],
                "network id / ssid / bssid / flags\n0\thome-net\tany\t\n1\tcafe\tany\t[DISABLED]\n\
                 2\thome-net\tany\t[TEMP-DISABLED]\n",
            ),
            wpa_answer(&["scan_results"], &self.scan),
            wpa_answer(&["enable_network", "all"], "OK\n"),
            wpa_answer(&["scan"], "OK\n"),
        ];
        for id in ["0", "1", "2"] {
            answers.push(wpa_answer(&["select_network", id], "OK\n"));
        }
        answers
    }

    fn observe(&mut self, value: &Value, now: f64, monitor: bool) -> Option<&'static str> {
        let (value, answers) = (ordered(value.clone()), self.answers());
        let monitor = monitor.then_some(&self.monitor);
        let recovery = &mut self.recovery;
        let (action, calls) = scripted(answers, || recovery.observe(&value, now, monitor));
        self.calls
            .extend(calls.into_iter().map(|(argv, _)| argv[3..].to_vec()));
        action
    }

    fn event(&mut self, text: &str) {
        let answers = self.answers();
        let monitor = &mut self.monitor;
        scripted(answers, || monitor.event(text).unwrap());
    }

    fn journal(&self) -> Vec<Value> {
        if !self.fixture.exists(JOURNAL) {
            return Vec::new();
        }
        self.fixture
            .read(JOURNAL)
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

#[test]
fn the_saved_network_in_range_is_selected_like_a_join() {
    // F52: DISCONNECTED for minutes with the saved network in range; a Join connected at once.
    let mut wifi = Recovery::new();
    let value = json!({"association": "DISCONNECTED", "radio_present": true, "state": "Failed",
                       "reason": "not_authenticated"});
    assert!(wifi.observe(&value, 100.0, true).is_none());
    assert!(wifi.observe(&value, 129.0, true).is_none());
    assert!(wifi.calls.is_empty());
    wifi.monitor.connected_network = Some(2);
    assert_eq!(wifi.observe(&value, 130.0, true), Some("wifi_reselect"));
    // The last network that connected, then the others enabled again.
    assert_eq!(
        wifi.calls[wifi.calls.len() - 2..],
        [
            argv(&["select_network", "2"]),
            argv(&["enable_network", "all"])
        ]
    );
    let row = wifi.journal().pop().unwrap();
    assert_eq!(
        json!([
            row["event"],
            row["network_id"],
            row["saved"],
            row["in_range"],
            row["temp_disabled"],
            row["ok"]
        ]),
        json!(["wifi_reselect", 2, 2, 2, 1, [true, true]])
    );
    assert!(row["events"].get("counts").is_some());
    assert!(!row.to_string().contains("home-net"));
    // Backed off: 30 s, then 60 s.
    wifi.calls.clear();
    wifi.observe(&value, 159.0, true);
    assert!(wifi.calls.is_empty());
    wifi.observe(&value, 160.0, true);
    wifi.observe(&value, 219.0, true);
    assert_eq!(wifi.calls.len(), 4);
    // A trying supplicant resets the wait, not the back-off.
    let mut scanning = value.clone();
    scanning["association"] = json!("SCANNING");
    wifi.observe(&scanning, 230.0, true);
    wifi.observe(&value, 231.0, true);
    wifi.observe(&value, 260.0, true);
    assert_eq!(wifi.calls.len(), 4);
    wifi.observe(&value, 261.0, true);
    assert_eq!(wifi.calls.len(), 8);
    let mut completed = value.clone();
    completed["association"] = json!("COMPLETED");
    wifi.observe(&completed, 270.0, true);
    let row = wifi.journal().pop().unwrap();
    assert_eq!(
        json!([row["event"], row["tries"]]),
        json!(["wifi_recovered", 3])
    );
}

#[test]
fn the_owners_disconnect_is_not_undone() {
    let mut wifi = Recovery::new();
    let value = json!({"association": "DISCONNECTED", "radio_present": true});
    wifi.event("<3>CTRL-EVENT-DISCONNECTED bssid=aa:bb:cc:dd:ee:01 reason=3 locally_generated=1");
    for now in [0.0, 30.0, 100.0, 400.0] {
        assert!(wifi.observe(&value, now, true).is_none());
    }
    assert!(wifi.calls.is_empty());
    // A join after it (any try) ends the hold.
    wifi.event("<3>Trying to associate with aa:bb:cc:dd:ee:01 (SSID='home-net' freq=2462 MHz)");
    wifi.observe(&value, 500.0, true);
    assert_eq!(wifi.observe(&value, 530.0, true), Some("wifi_reselect"));
}

#[test]
fn not_in_range_scans_and_a_password_verdict_is_left_alone() {
    let mut wifi = Recovery::new();
    wifi.scan = SCAN_HEADER.to_owned();
    let value = json!({"association": "INACTIVE", "radio_present": true});
    wifi.observe(&value, 0.0, false);
    assert_eq!(wifi.observe(&value, 30.0, false), Some("wifi_rescan"));
    assert_eq!(wifi.calls.last().unwrap(), &argv(&["scan"]));
    assert_eq!(wifi.journal().pop().unwrap()["in_range"], 0);
    wifi.calls.clear();
    let mut wrong = value.clone();
    wrong["association"] = json!("DISCONNECTED");
    wrong["state"] = json!("Failed");
    wrong["reason"] = json!("wrong_credentials");
    for now in [100.0, 200.0, 400.0] {
        assert!(wifi.observe(&wrong, now, false).is_none());
    }
    // Off, sleeping, or no radio: nothing.
    let mut off = value.clone();
    off["radio_present"] = json!(false);
    wifi.observe(&off, 500.0, false);
    wifi.fixture
        .put("/sys/class/rfkill/rfkill1/type", "wlan\n")
        .put("/sys/class/rfkill/rfkill1/soft", "1\n");
    for now in [600.0, 700.0, 800.0] {
        wifi.observe(&value, now, false);
    }
    assert!(wifi.calls.is_empty());
}
