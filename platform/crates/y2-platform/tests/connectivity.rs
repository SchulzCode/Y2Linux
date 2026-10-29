//! Group CONNECTIVITY on the command line: what tests/test_platform_network_time.py,
//! test_connectivity_supervisor.py, test_radio_policy.py and test_hardware_final_contract.py checked of
//! the Python platform where the Rust port reads its environment (udhcpc's and ntpd's variables), the
//! real clocks (wall, CLOCK_MONOTONIC, CLOCK_BOOTTIME: the times Python's tests passed in are made by
//! shaping the build floor and the anchors around the clock as it is), signals or a whole daemon.
// SPDX-License-Identifier: GPL-2.0-only
mod common;

use common::{Fixture, Run, BINARY};
use serde_json::{json, Value};
use std::{
    fs,
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use y2p_core::clock;

const BOOT_ID: &str = "/proc/sys/kernel/random/boot_id";

fn player() -> Fixture {
    let fixture = Fixture::new();
    fixture.put(BOOT_ID, "boot-one");
    fixture
}

fn words(argv: &[&str]) -> Vec<String> {
    argv.iter().map(|w| w.to_string()).collect()
}

// ---- DHCP (network.dhcp_event) -------------------------------------------------------------------

const FLUSH_ADDRESS: [&str; 8] = [
    "ip", "-4", "address", "flush", "dev", "wlan0", "scope", "global",
];
const FLUSH_ROUTE: [&str; 6] = ["ip", "-4", "route", "flush", "dev", "wlan0"];
const LINK_UP: [&str; 6] = ["ip", "link", "set", "dev", "wlan0", "up"];
const ADDRESS: [&str; 7] = ["ip", "-4", "address", "add", "192.0.2.2/24", "dev", "wlan0"];

/// The commands of a lease on 192.0.2.2/24 succeed, with these routes.
fn lease_answers(fixture: &mut Fixture, routes: &[&[&str]]) {
    for argv in [&FLUSH_ADDRESS[..], &FLUSH_ROUTE, &LINK_UP, &ADDRESS]
        .into_iter()
        .chain(routes.iter().copied())
    {
        fixture.answer(argv, true, "1");
    }
}

/// `wifi-event <event>`: the new lease epoch.
fn link_event(fixture: &Fixture, event: &str) -> String {
    let run = fixture.run(&["wifi-event", event]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    run.json()["epoch"].as_str().unwrap().to_owned()
}

fn dhcp(fixture: &Fixture, event: &str, env: &[(&str, &str)]) -> Run {
    fixture.run_as(None, &["dhcp-event", event], env)
}

const DEFAULT_ROUTE: [&str; 11] = [
    "ip",
    "-4",
    "route",
    "replace",
    "default",
    "via",
    "192.0.2.1",
    "dev",
    "wlan0",
    "metric",
    "100",
];

#[test]
fn dhcp_timeout_epoch_replacement_and_dns_lifecycle() {
    let mut fixture = player();
    let epoch = link_event(&fixture, "CONNECTED");
    let mut env = vec![
        ("interface", "wlan0"),
        ("Y2_DHCP_GENERATION", epoch.as_str()),
    ];
    assert_eq!(
        dhcp(&fixture, "leasefail", &env).json()["error"],
        "dhcp_timeout"
    );
    env.extend([
        ("ip", "192.0.2.2"),
        ("subnet", "255.255.255.0"),
        ("router", "192.0.2.1"),
        ("dns", "192.0.2.1"),
        ("lease", "300"),
    ]);
    lease_answers(&mut fixture, &[&DEFAULT_ROUTE]);
    let bound = dhcp(&fixture, "bound", &env);
    let result = bound.json();
    assert_eq!(result["state"], "Authenticated");
    assert!(result["error"].is_null());
    assert!(fixture
        .read("/run/y2/resolv.conf")
        .contains("nameserver 192.0.2.1"));
    assert!(bound
        .argvs()
        .iter()
        .all(|argv| !argv.iter().any(|a| a == "usb0")));
    link_event(&fixture, "DISCONNECTED");
    let stale = dhcp(&fixture, "renew", &env);
    assert_eq!(stale.code, Some(1));
    assert_eq!(stale.json()["reason"], "stale_dhcp_epoch");
    assert!(stale.calls.is_empty());
    assert!(!fixture.read("/run/y2/resolv.conf").contains("nameserver"));
}

#[test]
fn classless_routes_and_untrusted_dhcp_syntax() {
    let mut fixture = player();
    let epoch = link_event(&fixture, "CONNECTED");
    let mut env = vec![
        ("interface", "wlan0"),
        ("Y2_DHCP_GENERATION", epoch.as_str()),
        ("ip", "192.0.2.2"),
        ("subnet", "255.255.255.0"),
        ("router", "192.0.2.3"),
        ("dns", "192.0.2.1"),
        (
            "staticroutes",
            "0.0.0.0/0 192.0.2.1 198.51.100.0/24 0.0.0.0",
        ),
    ];
    let classless: [&str; 9] = [
        "ip",
        "-4",
        "route",
        "replace",
        "198.51.100.0/24",
        "dev",
        "wlan0",
        "metric",
        "100",
    ];
    let default: [&str; 11] = [
        "ip",
        "-4",
        "route",
        "replace",
        "0.0.0.0/0",
        "via",
        "192.0.2.1",
        "dev",
        "wlan0",
        "metric",
        "100",
    ];
    lease_answers(&mut fixture, &[&default, &classless]);
    let bound = dhcp(&fixture, "bound", &env);
    assert_eq!(bound.code, Some(0), "{}", bound.stdout);
    let calls = bound.argvs();
    assert!(!calls
        .iter()
        .any(|argv| argv.iter().any(|a| a.contains("192.0.2.3"))));
    assert!(calls.contains(&words(&classless)));
    env[5] = ("dns", "192.0.2.1;touch /bad");
    let refused = dhcp(&fixture, "renew", &env);
    assert_eq!(refused.code, Some(1));
    assert!(refused.calls.is_empty());
}

#[test]
fn dhcp_renew_of_the_same_lease_keeps_the_dns_confirmation() {
    // A renew of this epoch's lease keeps dns.json (it was deleted, so Wi-Fi showed dns_unavailable
    // for up to 300 s until the next probe); tests/dhcp_renew.rs covers the leases that drop it.
    let mut fixture = player();
    let epoch = link_event(&fixture, "CONNECTED");
    let env = [
        ("interface", "wlan0"),
        ("Y2_DHCP_GENERATION", epoch.as_str()),
        ("ip", "192.0.2.2"),
        ("subnet", "255.255.255.0"),
        ("router", "192.0.2.1"),
        ("dns", "192.0.2.1"),
    ];
    lease_answers(&mut fixture, &[&DEFAULT_ROUTE]);
    assert_eq!(dhcp(&fixture, "bound", &env).code, Some(0));
    fixture.put_json(
        "/run/y2/dns.json",
        &json!({"ok": true, "epoch": epoch, "address": "192.0.2.2"}),
    );
    let renewed = dhcp(&fixture, "renew", &env);
    assert_eq!(renewed.code, Some(0));
    assert_eq!(renewed.json()["state"], "Authenticated");
    assert!(fixture.exists("/run/y2/dns.json"));
}

// ---- the clock (timekeeping) ---------------------------------------------------------------------

const DAY: i64 = 86_400;

fn wall() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn boottime() -> f64 {
    clock::boottime_ns() as f64 / 1e9
}

fn time_status(fixture: &Fixture) -> Value {
    let run = fixture.run(&["time"]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    run.json()
}

fn tls_ready(fixture: &Fixture) -> bool {
    time_status(fixture)["tls_ready"] == true
}

fn floor(fixture: &Fixture, epoch: i64) {
    fixture.put("/etc/y2linux/build-epoch", &epoch.to_string());
}

/// `ntp-event <event>` as BusyBox ntpd runs it, with `stratum` in the environment.
fn ntp(fixture: &Fixture, event: &str, stratum: &str) -> Run {
    let run = fixture.run_as(None, &["ntp-event", event], &[("stratum", stratum)]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    run
}

fn date_set(epoch: i64) -> Vec<String> {
    words(&["/bin/busybox", "date", "-u", "-s", &format!("@{epoch}")])
}

#[test]
fn implausible_rtc_build_floor_and_network_clock_readiness() {
    let mut fixture = player();
    let now = wall();
    // A clock before the build date, then one far after it: neither is a reasonable clock.
    floor(&fixture, now + 10 * DAY);
    assert!(!tls_ready(&fixture));
    let built = now - 6 * 366 * DAY;
    floor(&fixture, built);
    assert_eq!(time_status(&fixture)["clock_sane"], false);
    fixture.answer(
        &["/bin/busybox", "date", "-u", "-s", &format!("@{built}")],
        true,
        "",
    );
    let bootstrap = fixture.run(&["time-bootstrap"]);
    assert_eq!(bootstrap.json()["source"], "build_floor");
    assert!(bootstrap.argvs().contains(&date_set(built)));
    // The clock now reads as just after the build date (the floor a little before it).
    floor(&fixture, now - 100);
    assert!(!tls_ready(&fixture));
    let synced = ntp(&fixture, "stratum", "3");
    assert!(tls_ready(&fixture));
    // The wall clock 900 s off the anchor (Python's status at floor + 1000).
    let anchor = fixture.json("/run/y2/time.json");
    let mut stepped = anchor.clone();
    stepped["epoch"] = json!(anchor["epoch"].as_f64().unwrap() - 900.0);
    fixture.put_json("/run/y2/time.json", &stepped);
    assert!(!tls_ready(&fixture));
    fixture.put_json("/run/y2/time.json", &anchor);
    assert!(tls_ready(&fixture));
    assert!(!synced
        .argvs()
        .iter()
        .any(|argv| argv.iter().any(|a| a == "hwclock")));
    fixture.put(BOOT_ID, "boot-two");
    assert!(!tls_ready(&fixture));
    fixture.put("/sys/class/rtc/rtc0/date", "2026-09-22");
    assert_eq!(
        fixture.run(&["time-bootstrap"]).json()["source"],
        "retained_rtc"
    );
    assert!(tls_ready(&fixture));
}

#[test]
fn tls_clock_stays_ready_across_suspend() {
    let fixture = player();
    let now = wall() as f64;
    floor(&fixture, now as i64 - 100);
    let anchor = |epoch: f64, monotonic: f64, boottime: Option<f64>| {
        let mut value = json!({"schema": 1, "boot_id": "boot-one", "epoch": epoch,
                               "monotonic_s": monotonic, "source": "ntp", "stratum": 2,
                               "authenticated": false, "event": "stratum"});
        if let Some(boottime) = boottime {
            value["boottime_s"] = json!(boottime);
        }
        fixture.put_json("/run/y2/time.json", &value);
    };
    let (monotonic, elapsed) = (clock::monotonic(), boottime());
    // One hour asleep: wall clock and CLOCK_BOOTTIME advance, CLOCK_MONOTONIC only by the few awake
    // seconds. Beta 4 refused update checks here.
    anchor(now - 3605.0, monotonic - 5.0, Some(elapsed - 3605.0));
    assert!(tls_ready(&fixture));
    // Someone stepping the wall clock is still caught.
    anchor(now - 7205.0, monotonic - 5.0, Some(elapsed - 3605.0));
    assert!(!tls_ready(&fixture));
    // An anchor written before this change keeps the monotonic comparison.
    anchor(now - 5.0, monotonic - 5.0, None);
    assert!(tls_ready(&fixture));
    anchor(now - 3605.0, monotonic - 5.0, None);
    assert!(!tls_ready(&fixture));
}

/// `time-set <epoch>` with the date, the RTC write and the time server's restart answered.
fn set_time(fixture: &mut Fixture, epoch: i64) -> Run {
    fixture
        .answer(
            &["/bin/busybox", "date", "-u", "-s", &format!("@{epoch}")],
            true,
            "",
        )
        .answer(&["/bin/busybox", "hwclock", "-w", "-u"], true, "")
        .answer(&["/etc/init.d/S43y2-time", "stop"], true, "")
        .answer(&["/etc/init.d/S43y2-time", "start"], true, "");
    fixture.run(&["time-set", &epoch.to_string()])
}

#[test]
fn manual_time_is_kept_as_a_floor_never_trusted_and_ntp_replaces_it() {
    // #53: a player that never had Wi-Fi started from the build date every boot; the owner's own time
    // now carries over a restart. The owner's time is the clock as it is (the floor 3 days before).
    let mut fixture = player();
    let chosen = wall();
    let built = chosen - 3 * DAY;
    floor(&fixture, built);
    fixture.put(
        "/etc/y2linux/rtc-policy.json",
        r#"{"write_enabled": true, "source": "receipt"}"#,
    );
    for wrong in [built - 86_401, built + 6 * 366 * DAY] {
        let run = fixture.run(&["time-set", &wrong.to_string()]);
        assert_eq!(run.code, Some(1));
        assert_eq!(run.json()["failure"], "date_out_of_range");
        assert!(run.calls.is_empty());
    }
    // Not an integer: argparse's (Python's set_manual refused 'now', 0.5 and True the same way).
    for wrong in ["now", &format!("{}.5", built + DAY), "True"] {
        let run = fixture.run(&["time-set", wrong]);
        assert_eq!(run.code, Some(2));
        assert!(run.calls.is_empty());
    }
    assert!(!fixture.exists("/run/y2/time.json"));
    let set = set_time(&mut fixture, chosen);
    let value = set.json();
    assert_eq!(value["source"], "manual");
    let calls = set.argvs();
    assert!(calls.contains(&date_set(chosen)));
    assert!(calls.contains(&words(&["/bin/busybox", "hwclock", "-w", "-u"])));
    // No time server running.
    assert!(!calls.contains(&words(&["/etc/init.d/S43y2-time", "start"])));
    let clock = time_status(&fixture);
    assert_eq!(clock["clock_sane"], true);
    assert_eq!(clock["tls_ready"], false);
    assert_eq!(clock["ntp_authenticated"], false);
    assert_eq!(clock["source"], "manual");
    let written = fixture.json("/run/y2/time-status.json");
    assert_eq!(
        json!([written["source"], written["tls_ready"]]),
        json!(["manual", false])
    );
    let last = fixture.json("/data/system/platform/time-last.json");
    assert_eq!(
        json!([last["source"], last["epoch"]]),
        json!(["manual", chosen])
    );
    // Next boot, RTC lost: the clock resumes from the owner's time, untrusted. (The clock reads before
    // the build date here: the owner set a time 3 days after a build of 2 days ahead.)
    let ahead = chosen + 2 * DAY;
    floor(&fixture, ahead);
    set_time(&mut fixture, ahead + 3 * DAY);
    fixture.put(BOOT_ID, "boot-two");
    let bootstrap = fixture.run(&["time-bootstrap"]);
    assert_eq!(bootstrap.json()["source"], "manual");
    assert!(bootstrap.argvs().contains(&date_set(ahead + 3 * DAY)));
    assert!(!tls_ready(&fixture));
    // An RTC that kept the owner's time is still only the owner's time.
    floor(&fixture, built);
    set_time(&mut fixture, chosen);
    fixture.put("/sys/class/rtc/rtc0/date", "2026-09-22");
    assert_eq!(fixture.run(&["time-bootstrap"]).json()["source"], "manual");
    assert!(!tls_ready(&fixture));
    // The first network time replaces it, even within the hour.
    ntp(&fixture, "stratum", "2");
    let last = fixture.json("/data/system/platform/time-last.json");
    assert_eq!(last["source"], "ntp");
    assert!((last["epoch"].as_f64().unwrap() - wall() as f64).abs() < 5.0);
    assert!(tls_ready(&fixture));
    // A running, not yet synchronised time server is restarted: a fresh one steps at its first sync.
    fixture.put("/run/y2/ntp.pid", "4242");
    fixture.put_json(
        "/run/y2/time.json",
        &json!({"source": "manual", "boot_id": "boot-two"}),
    );
    let set = set_time(&mut fixture, chosen + 400);
    let calls = set.argvs();
    assert_eq!(
        calls[calls.len() - 2..],
        [
            words(&["/etc/init.d/S43y2-time", "stop"]),
            words(&["/etc/init.d/S43y2-time", "start"])
        ]
    );
    assert_eq!(set.json()["ntp_restart"], true);
    ntp(&fixture, "stratum", "2");
    // Network time this boot: a hand-set time is refused (busybox ntpd kept one 2 h off for minutes
    // while its periodic callback called it network time, 8.22 test build).
    let refused = set_time(&mut fixture, chosen + 7200);
    assert_eq!(refused.code, Some(1));
    assert_eq!(refused.json()["failure"], "network_time_active");
    assert!(refused.calls.is_empty());
    fixture.put(BOOT_ID, "boot-three");
    assert_eq!(
        fixture.run(&["time-bootstrap"]).json()["source"],
        "retained_rtc"
    );
}

#[test]
fn ntp_source_backed_default_and_owner_disable() {
    let mut fixture = player();
    floor(&fixture, wall() - 10);
    fixture.put(
        "/etc/y2linux/rtc-policy.json",
        r#"{"schema":1,"write_enabled":true,"source":"stock"}"#,
    );
    fixture.answer(&["/bin/busybox", "hwclock", "-w", "-u"], true, "");
    assert_eq!(ntp(&fixture, "step", "3").json()["rtc_write"], true);
    fixture.put(
        "/data/system/platform/rtc-policy.json",
        r#"{"schema":1,"write_enabled":false}"#,
    );
    let run = ntp(&fixture, "step", "3");
    assert!(run.json().get("rtc_write").is_none());
    assert!(run.calls.is_empty());
}

// ---- daemons -------------------------------------------------------------------------------------

/// `y2-platform args…` started on the fixture root (no answers), and where its requests go.
fn spawn(fixture: &Fixture, args: &[&str]) -> (Child, std::path::PathBuf) {
    let scratch = fixture.root.parent().unwrap().join("spawn");
    fs::create_dir_all(&scratch).unwrap();
    let (commands, calls) = (scratch.join("commands.json"), scratch.join("calls.jsonl"));
    fs::write(&commands, "[]").unwrap();
    let child = Command::new(BINARY)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("Y2_ROOT", &fixture.root)
        .env("Y2_FIXTURE_COMMANDS", &commands)
        .env("Y2_FIXTURE_CALLS", &calls)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    (child, calls)
}

fn wait_for(what: impl Fn() -> bool, seconds: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        if what() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    what()
}

/// SIGTERM, then the exit code, how long the end took and stderr (killed after 2 s).
fn terminate(child: &mut Child) -> (Option<i32>, Duration, String) {
    let started = Instant::now();
    // SAFETY: a plain signal to the child this test started.
    unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
    while child.try_wait().unwrap().is_none() && started.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(5));
    }
    let took = started.elapsed();
    let _ = child.kill();
    let code = child.wait().unwrap().code();
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = std::io::Read::read_to_string(&mut pipe, &mut stderr);
    }
    (code, took, stderr)
}

#[test]
fn idle_fd_wait_is_signal_interruptible_and_cleans_up() {
    let fixture = player();
    fixture.put("/run/y2/.keep", "");
    let (mut child, calls) = spawn(&fixture, &["connectivity-daemon"]);
    // Its first health look restarts the (absent) helpers; then it sleeps on its descriptors.
    assert!(wait_for(|| calls.exists(), 5));
    std::thread::sleep(Duration::from_millis(100));
    let before = fs::read_to_string(&calls).unwrap().lines().count();
    let (code, took, stderr) = terminate(&mut child);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(took < Duration::from_secs(1));
    // The helpers are stopped on the way out.
    let after = fs::read_to_string(&calls).unwrap();
    assert!(after.lines().count() > before);
    assert!(after
        .lines()
        .skip(before)
        .all(|l| l.contains("start-stop-daemon")));
}

const STATUS: [&str; 4] = ["wpa_cli", "-i", "wlan0", "status"];

#[test]
fn readiness_query_failures_are_counted_without_link_events() {
    let fixture = player();
    // Radio present, the supplicant's status query fails (the fixture answers nothing), screen dark.
    fixture
        .put("/sys/class/net/wlan0/ifindex", "4")
        .put("/sys/class/backlight/panel/bl_power", "4\n")
        .put("/sys/class/backlight/panel/brightness", "200\n");
    let run = fixture.run_as(None, &["service-daemon"], &[("Y2_DAEMON_TICKS", "2")]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    let result = fixture.json("/run/y2/network.json");
    assert_eq!(result["state"], "Starting");
    assert_eq!(result["status_query_failures"], 2);
    assert!(!fixture.exists("/run/y2/wifi-events.json"));
    // Without the radio a pass is no failed query; the count of this boot is kept.
    fs::remove_dir_all(fixture.path("/sys/class/net/wlan0")).unwrap();
    let run = fixture.run_as(None, &["service-daemon"], &[("Y2_DAEMON_TICKS", "1")]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    let result = fixture.json("/run/y2/network.json");
    assert_eq!(result["state"], "Off");
    assert_eq!(result["status_query_failures"], 2);
    assert!(!fixture.exists("/run/y2/wifi-events.json"));
    // Unknown status must not trigger fast polling: one look in 1.5 s (the dark cadence is 10 s).
    fixture.put("/sys/class/net/wlan0/ifindex", "4");
    let (mut child, calls) = spawn(&fixture, &["service-daemon"]);
    std::thread::sleep(Duration::from_millis(1500));
    let (code, _, stderr) = terminate(&mut child);
    assert_eq!(code, Some(0), "{stderr}");
    let looks = fs::read_to_string(&calls)
        .unwrap_or_default()
        .lines()
        .filter(|line| serde_json::from_str::<Value>(line).unwrap()["argv"] == json!(STATUS))
        .count();
    assert_eq!(looks, 1);
}

// ---- owner checks and observations ---------------------------------------------------------------

#[test]
fn qualification_never_passes_invalid_iperf_output_or_offline_wifi() {
    let mut fixture = player();
    // No radio: Wi-Fi is Off.
    let off = fixture.run(&["network-check", "--peer", "192.0.2.10"]);
    assert_eq!(off.code, Some(1));
    assert_eq!(off.json()["record"]["result"], "FAILED");
    assert!(!off
        .argvs()
        .iter()
        .any(|argv| argv[0] == "ping" || argv[0] == "iperf3"));
    // Online: associated, addressed, routed, DNS confirmed for this lease.
    fixture
        .put("/sys/class/net/wlan0/ifindex", "4")
        .put("/run/y2/wifi-epoch", "current")
        .put_json(
            "/run/y2/dns.json",
            &json!({"ok": true, "boot_id": "boot-one", "monotonic_s": clock::monotonic(),
                    "address": "192.0.2.2", "bssid": "one", "epoch": "current"}),
        );
    fixture
        .answer(&STATUS, true, "wpa_state=COMPLETED\nbssid=one")
        .answer(
            &["ip", "-j", "-4", "addr", "show", "dev", "wlan0"],
            true,
            r#"[{"addr_info":[{"local":"192.0.2.2","scope":"global"}]}]"#,
        )
        .answer(
            &["ip", "-j", "-4", "route", "show", "default", "dev", "wlan0"],
            true,
            r#"[{"dst":"default"}]"#,
        )
        .answer(
            &["ping", "-I", "wlan0", "-c", "5", "-W", "2", "192.0.2.10"],
            true,
            "1",
        )
        .answer(
            &[
                "iperf3",
                "--bind-dev",
                "wlan0",
                "-c",
                "192.0.2.10",
                "-t",
                "10",
                "-J",
            ],
            true,
            "1",
        );
    let checked = fixture.run(&["network-check", "--peer", "192.0.2.10", "--throughput"]);
    let result = checked.json();
    assert_eq!(result["before"]["wifi"]["state"], "Online");
    assert_eq!(result["record"]["result"], "FAILED");
    assert_eq!(checked.code, Some(1));
}

fn section(fixture: &Fixture, name: &str) -> Value {
    let run = fixture.run(&["status", name]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    run.json()
}

#[test]
fn actual_timer_modes_are_observed_and_unknown_is_not_false_success() {
    let fixture = player();
    assert!(section(&fixture, "cpu")["cpu"]["timer"]["highres_active"].is_null());
    fixture.put(
        "/proc/timer_list",
        "cpu: 0\n .hres_active : 1\n .nohz : 1\n .tick_stopped : 0\n\
         cpu: 1\n .hres_active : 0\n .nohz : 1\n .tick_stopped : 1\n",
    );
    let timer = &section(&fixture, "cpu")["cpu"]["timer"];
    assert_eq!(timer["highres_active"], false);
    assert_eq!(timer["no_hz_active"], true);
    assert_eq!(timer["per_cpu"][1]["tick_stopped"], 1);
    fixture.put("/proc/timer_list", "cpu: 0\n .hres_active : 1\n");
    assert!(section(&fixture, "cpu")["cpu"]["timer"]["no_hz_active"].is_null());
    fixture.put(
        "/sys/bus/platform/devices/11230000.mmc/y2_performance",
        "cap_hz=25000000 actual_hz=25000000 transport_errors=2 fallbacks=1 clock_error=0\n",
    );
    assert_eq!(
        section(&fixture, "storage")["storage"]["controllers"][0]["fallbacks"],
        1
    );
    fixture.put(
        "/sys/bus/platform/drivers/y2-usb/11200000.usb/status",
        "transfer=inventra_dma dma_irqs=12 dma_rx_programmed_bytes=1024 dma_errors=1\n",
    );
    let usb = section(&fixture, "usb");
    let dma = &usb["usb"]["dma"];
    assert_eq!(dma["transfer"], "inventra_dma");
    assert_eq!(dma["dma_rx_programmed_bytes"], 1024);
    assert_eq!(dma["dma_errors"], 1);
}

#[test]
fn invalid_codec_switches_never_modify_saved_settings() {
    // codec_controls.configure refused a non-boolean `*_abr` (tests/test_radio_policy.py); here the
    // command line takes on|off only (argparse's choices, exit 2).
    let fixture = player();
    fixture.put("/run/y2/.keep", "");
    for option in ["--aac-abr", "--ldac-abr", "--sbc-abr"] {
        let run = fixture.run(&["codec-settings", option, "yes"]);
        assert_eq!(run.code, Some(2), "{option}");
        assert!(run.stderr.contains("invalid choice"), "{}", run.stderr);
    }
    assert!(!fixture.exists("/data/bluetooth/codec-policy.json"));
}
