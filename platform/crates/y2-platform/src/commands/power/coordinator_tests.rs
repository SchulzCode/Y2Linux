//! The shutdown coordinator tests of tests/test_platform_power_contract.py, on the port.
//!
//! Python gave the coordinator an injected clock; here it reads CLOCK_MONOTONIC, so "the grace has
//! passed" is the intent's deadline moved to now (`expire`) and waits are compared with a tolerance.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::{failed, ok, scripted, Fixture};
use std::time::Instant;
use y2p_core::command::Outcome;

type Answers = Vec<(Vec<String>, Outcome)>;
type Calls = Vec<(Vec<String>, f64)>;

/// Every command a shutdown asks for, answered ok or `injected_failure` (the Python test's runner).
fn answers(succeed: bool) -> Answers {
    let stage = |name| ["/usr/sbin/y2-platform", "boot-stage", name];
    let argvs: Vec<Vec<&str>> = vec![
        stage("shutdown_pending").to_vec(),
        vec!["/etc/init.d/S90reborn", "stop"],
        vec!["/usr/sbin/y2-platform", "time-remember"],
        vec!["/bin/busybox", "sync"],
        vec!["/etc/init.d/S41y2-connectivity", "stop-wifi"],
        vec!["/etc/init.d/S41y2-connectivity", "stop-bluetooth"],
        vec!["/etc/init.d/S41y2-connectivity", "stop-daemon"],
        stage("shutdown_complete").to_vec(),
        vec!["/bin/busybox", "poweroff"],
        vec!["/bin/busybox", "reboot"],
    ];
    argvs
        .iter()
        .map(|argv| {
            if succeed {
                ok(argv, "")
            } else {
                failed(argv, "injected_failure")
            }
        })
        .collect()
}

/// The Python `setUp`: a boot id, every command failing.
fn fixture() -> Fixture {
    let fixture = Fixture::new();
    fixture.put("/proc/sys/kernel/random/boot_id", "one");
    fixture
}

fn json(text: &str) -> Json {
    Json::from(text)
}

fn request(c: &mut Coordinator, action: &str, reason: &str, grace: i64) -> Object {
    c.request(&json(action), &json(reason), grace)
        .expect("request")
}

/// The grace has passed: the pending deadline is now.
fn expire(c: &mut Coordinator) {
    let now = clock::monotonic_ns() as i64;
    c.intent
        .as_mut()
        .expect("intent")
        .insert("deadline_monotonic_ns", now);
}

fn execute(c: &mut Coordinator, answers: Answers) -> Calls {
    let (result, calls) = scripted(answers, || c.execute());
    result.expect("execute");
    calls
}

/// `c.intent['stop_results'][name]` as (ok, reason).
fn stop_result(c: &Coordinator, name: &str) -> Option<(bool, Option<String>)> {
    let results = c.intent.as_ref()?.value("stop_results");
    let result = results.get(name)?;
    let ok = matches!(result.get("ok"), Some(Json::Bool(true)));
    Some((
        ok,
        result
            .get("reason")
            .and_then(Json::as_str)
            .map(str::to_owned),
    ))
}

fn passed() -> Option<(bool, Option<String>)> {
    Some((true, None))
}

fn refused(reason: &str) -> Option<(bool, Option<String>)> {
    Some((false, Some(reason.into())))
}

fn last(calls: &Calls) -> Vec<&str> {
    calls
        .last()
        .expect("a call")
        .0
        .iter()
        .map(String::as_str)
        .collect()
}

/// The step names of the pmsg trail (`line.split(None, 2)[2]`).
fn trail(fixture: &Fixture) -> Vec<String> {
    fixture
        .read("/dev/pmsg0")
        .lines()
        .map(|line| line.splitn(3, ' ').nth(2).unwrap_or_default().to_owned())
        .collect()
}

/// A Reborn process as `process_identity` reads it, started at tick `start`.
fn reborn(fixture: &Fixture, start: u32) {
    fixture.put("/run/reborn/process.pid", "123").put(
        "/proc/123/stat",
        &format!("123 (reborn) S {} {start}", ["0"; 18].join(" ")),
    );
    if !fixture.exists("/proc/123/exe") {
        std::os::unix::fs::symlink(
            fixture.path("/usr/bin/reborn"),
            fixture.path("/proc/123/exe"),
        )
        .unwrap();
    }
}

#[test]
fn ignored_app_and_failed_sync_do_not_block_final_init_request() {
    let fixture = fixture();
    let mut c = Coordinator::new(&fixture.root);
    request(&mut c, "poweroff", "user", 3);
    assert!(!c.due());
    expire(&mut c);
    assert!(c.due());
    // Same request cannot extend grace; conflicting intent is rejected.
    let deadline = c.intent.as_ref().unwrap().value("deadline_monotonic_ns");
    assert_eq!(
        request(&mut c, "poweroff", "user", 30).value("deadline_monotonic_ns"),
        deadline
    );
    assert!(c.request(&json("reboot"), &json("service"), 10).is_err());
    let calls = execute(&mut c, answers(false));
    assert_eq!(last(&calls), ["/bin/busybox", "poweroff"]);
    assert!(calls.iter().all(|(_, timeout)| *timeout <= 5.0));
    assert!(c.intent.as_ref().unwrap().value("state").is_str("Failed"));
    assert!(!c.due());
}

#[test]
fn backlight_is_off_before_init_powers_down_even_without_reborn() {
    let fixture = fixture();
    fixture.put("/sys/class/backlight/panel/bl_power", "0\n");
    let mut c = Coordinator::new(&fixture.root);
    request(&mut c, "reboot", "user", 3);
    expire(&mut c);
    let calls = execute(&mut c, answers(false));
    assert_eq!(
        fixture
            .root
            .read("/sys/class/backlight/panel/bl_power")
            .as_deref(),
        Some("4")
    );
    assert_eq!(stop_result(&c, "backlight"), passed());
    assert_eq!(last(&calls), ["/bin/busybox", "reboot"]);
}

#[test]
fn unacknowledged_shutdown_blanks_the_panel_before_reborn_is_killed() {
    // Reborn hung or crashed: the deadline fires without an acknowledgement.
    let fixture = fixture();
    fixture.put("/sys/class/backlight/panel/bl_power", "0\n");
    let mut c = Coordinator::new(&fixture.root);
    request(&mut c, "poweroff", "user", 3);
    expire(&mut c);
    execute(&mut c, answers(true));
    assert_eq!(stop_result(&c, "backlight_early"), passed());
    assert_eq!(
        fixture
            .root
            .read("/sys/class/backlight/panel/bl_power")
            .as_deref(),
        Some("4")
    );
    // The panel is already dark when Reborn is stopped (Python recorded the light at the stop call;
    // scripted answers cannot read files, so the order is pinned on the source).
    let source = include_str!("coordinator.rs");
    let early = source
        .find("results.push((\"backlight_early\".into(), backlight_off(&self.root)));")
        .expect("early backlight");
    let stop = source
        .find("self.step(\"S90reborn\", Some(&[\"/etc/init.d/S90reborn\", \"stop\"]), 3.0)")
        .expect("Reborn stop");
    assert!(early < stop, "panel is already dark when Reborn is stopped");
}

#[test]
fn acknowledged_shutdown_leaves_reborns_dark_frame_alone_until_the_end() {
    let fixture = fixture();
    reborn(&fixture, 900);
    fixture.put("/sys/class/backlight/panel/bl_power", "4\n");
    let mut c = Coordinator::new(&fixture.root);
    let intent = request(&mut c, "poweroff", "user", 3);
    c.ack(&intent.value("id"), 123, &json("Ready"))
        .expect("ack");
    execute(&mut c, answers(false));
    assert_eq!(stop_result(&c, "backlight_early"), None);
    assert_eq!(stop_result(&c, "backlight"), passed());
}

#[test]
fn missing_backlight_is_recorded_not_fatal() {
    let fixture = fixture();
    let mut c = Coordinator::new(&fixture.root);
    request(&mut c, "poweroff", "user", 3);
    expire(&mut c);
    let calls = execute(&mut c, answers(false));
    assert_eq!(
        stop_result(&c, "backlight").and_then(|r| r.1).as_deref(),
        Some("no_backlight")
    );
    assert_eq!(last(&calls), ["/bin/busybox", "poweroff"]);
}

#[test]
fn shutdown_steps_reach_pmsg_and_stage_writes_get_three_seconds() {
    // F39 (471831b6): an update restart left no stage and no trail.
    let fixture = fixture();
    fixture.put("/dev/pmsg0", "");
    let mut c = Coordinator::new(&fixture.root);
    request(&mut c, "reboot", "update", 3);
    expire(&mut c);
    let calls = execute(&mut c, answers(false));
    // F39 (8.39-8.41): every step is announced before it runs, so a stall
    // names the step it happened in; the radio stop is split and comes
    // after the sync, before shutdown_complete.
    assert_eq!(
        trail(&fixture),
        [
            "execute reboot update",
            "watchdog watchdog_unavailable",
            "long_press_reset kept_without_watchdog",
            "begin journal_pending",
            "journal_pending injected_failure",
            "begin S90reborn",
            "S90reborn injected_failure",
            "begin kill",
            "begin backlight",
            "begin time_remember",
            "time_remember injected_failure",
            "begin sync",
            "sync injected_failure",
            "begin S41_wifi",
            "S41_wifi injected_failure",
            "begin S41_wifi_removed",
            "S41_wifi_removed ok",
            "begin S41_bluetooth",
            "S41_bluetooth injected_failure",
            "begin S41_daemon",
            "S41_daemon injected_failure",
            "begin init_reboot",
            "init_reboot injected_failure",
        ]
    );
    let stages: Vec<f64> = calls
        .iter()
        .filter(|(argv, _)| argv.iter().any(|a| a == "boot-stage"))
        .map(|(_, timeout)| *timeout)
        .collect();
    assert_eq!(stages, [3.0]);
    let radio: Vec<&Vec<String>> = calls
        .iter()
        .map(|(argv, _)| argv)
        .filter(|argv| argv[0] == "/etc/init.d/S41y2-connectivity")
        .collect();
    let expected: Vec<Vec<String>> = ["wifi", "bluetooth", "daemon"]
        .iter()
        .map(|n| {
            vec![
                "/etc/init.d/S41y2-connectivity".to_owned(),
                format!("stop-{n}"),
            ]
        })
        .collect();
    assert_eq!(radio, expected.iter().collect::<Vec<_>>());
}

#[test]
fn radio_stop_runs_after_sync_and_before_shutdown_complete() {
    let fixture = fixture();
    let mut c = Coordinator::new(&fixture.root);
    request(&mut c, "poweroff", "user", 3);
    expire(&mut c);
    let calls = execute(&mut c, answers(true));
    let order: Vec<&str> = calls
        .iter()
        .map(|(argv, _)| argv.last().unwrap().as_str())
        .collect();
    assert_eq!(
        order,
        [
            "shutdown_pending",
            "stop",
            "time-remember",
            "sync",
            "stop-wifi",
            "stop-bluetooth",
            "stop-daemon",
            "shutdown_complete",
            "poweroff"
        ]
    );
    assert!(calls.iter().all(|(_, timeout)| *timeout <= 5.0));
    for name in [
        "S41_wifi",
        "S41_wifi_removed",
        "S41_bluetooth",
        "S41_daemon",
    ] {
        assert_eq!(stop_result(&c, name), passed(), "{name}");
    }
}

#[test]
fn wifi_removal_is_waited_for_but_bounded() {
    let fixture = fixture();
    fixture
        .put("/sys/class/net/wlan0/operstate", "up")
        .put("/dev/pmsg0", "");
    let mut c = Coordinator::new(&fixture.root);
    request(&mut c, "reboot", "update", 3);
    // Python counted 19+ sleeps of 0.1 s on a mocked clock ending 2.0 s later; this one waits.
    let started = Instant::now();
    execute(&mut c, answers(false));
    let waited = started.elapsed().as_secs_f64();
    assert_eq!(
        stop_result(&c, "S41_wifi_removed"),
        refused("still_present")
    );
    assert!((2.0..3.0).contains(&waited), "{waited}");
    assert!(fixture
        .read("/dev/pmsg0")
        .contains("shutdown S41_wifi_removed still_present"));
}

#[test]
fn shutdown_arms_the_watchdog_and_a_power_off_reset_powers_off() {
    // F39: a stalled shutdown (3b2364ca drained a full battery while it
    // looked off) ends in the SoC watchdog reset. For a power-off the
    // reset must not start the system: the loader's reason 3 charges or
    // powers off. A restart keeps AUTO_RESTART (update: MISC -> recovery).
    let fixture = fixture();
    for (action, reason, expected) in [
        ("poweroff", "user", Some("1\n")),
        ("poweroff", "low_battery", Some("1\n")),
        ("reboot", "update", None),
        ("reboot", "user", None),
    ] {
        fixture
            .put("/dev/watchdog", "")
            .put(WATCHDOG_POWERS_OFF, "0\n")
            .put("/run/y2/shutdown.json", "null");
        let mut c = Coordinator::new(&fixture.root);
        request(&mut c, action, reason, 3);
        execute(&mut c, answers(false));
        assert_eq!(stop_result(&c, "watchdog"), passed(), "{action} {reason}");
        assert_eq!(
            fixture.root.read(WATCHDOG_POWERS_OFF).as_deref(),
            Some(expected.unwrap_or("0\n").trim()),
            "{action} {reason}"
        );
        // Fed before every step; never closed with the magic 'V'.
        let fed = fixture.read("/dev/watchdog");
        assert!(fed.len() >= 10, "{fed:?}");
        assert!(fed.chars().all(|c| c == '.'), "{fed:?}");
        drop(c); // os.close(c.watchdog)
    }
}

#[test]
fn power_off_without_the_kernel_switch_still_arms_the_watchdog() {
    let fixture = fixture();
    fixture.put("/dev/watchdog", "");
    let mut c = Coordinator::new(&fixture.root);
    request(&mut c, "poweroff", "user", 3);
    execute(&mut c, answers(false));
    assert_eq!(
        stop_result(&c, "watchdog"),
        refused("powers_off_unavailable")
    );
    assert!(fixture.read("/dev/watchdog").starts_with('.'));
}

#[test]
fn armed_watchdog_turns_the_long_press_reset_off_before_the_first_step() {
    // F57 (496f54ad): Power still held through the power-off let the PMIC
    // long-press reset cut it. From the watchdog on, that reset is off, for
    // a restart as for a power-off; the preloader sets it again at the start.
    let fixture = fixture();
    for action in ["poweroff", "reboot"] {
        fixture
            .put("/dev/watchdog", "")
            .put("/dev/pmsg0", "")
            .put(LONG_PRESS_RESET, "")
            .put("/run/y2/shutdown.json", "null");
        let mut c = Coordinator::new(&fixture.root);
        request(&mut c, action, "user", 3);
        let calls = execute(&mut c, answers(true));
        assert!(!calls.is_empty());
        assert_eq!(fixture.root.read(LONG_PRESS_RESET).as_deref(), Some("0"));
        assert_eq!(stop_result(&c, "long_press_reset"), passed(), "{action}");
        // Python read the switch at the first command; the trail shows it was written before the
        // first shutdown step began.
        let steps = trail(&fixture);
        assert_eq!(steps[2], "long_press_reset ok", "{action}");
        assert_eq!(
            steps[3], "begin journal_pending",
            "off before the first shutdown step"
        );
    }
}

#[test]
fn long_press_reset_without_kernel_support_or_watchdog_never_blocks_the_shutdown() {
    // An older kernel has no switch: recorded, the shutdown goes on.
    let fixture = fixture();
    fixture.put("/dev/watchdog", "");
    let mut c = Coordinator::new(&fixture.root);
    request(&mut c, "poweroff", "user", 3);
    let calls = execute(&mut c, answers(false));
    assert_eq!(
        stop_result(&c, "long_press_reset"),
        refused("long_press_reset_unavailable")
    );
    assert!(!fixture.exists(LONG_PRESS_RESET), "never created");
    assert_eq!(last(&calls), ["/bin/busybox", "poweroff"]);
    drop(c);
    // No watchdog: the long-press reset stays the only escape from a stall.
    fixture
        .remove("/dev/watchdog")
        .put(LONG_PRESS_RESET, "")
        .put("/run/y2/shutdown.json", "null");
    let mut c = Coordinator::new(&fixture.root);
    request(&mut c, "reboot", "user", 3);
    let calls = execute(&mut c, answers(false));
    assert_eq!(
        stop_result(&c, "long_press_reset"),
        refused("kept_without_watchdog")
    );
    assert_eq!(fixture.read(LONG_PRESS_RESET), "");
    assert_eq!(last(&calls), ["/bin/busybox", "reboot"]);
}

#[test]
fn ack_requires_same_request_pid_start_time_and_boot() {
    let fixture = fixture();
    reborn(&fixture, 900);
    let mut c = Coordinator::new(&fixture.root);
    let intent = request(&mut c, "reboot", "update", 10);
    let id = intent.value("id");
    let ready = json("Ready");
    assert!(c.ack(&json("old"), 123, &ready).is_err());
    assert!(c.ack(&id, 124, &ready).is_err());
    reborn(&fixture, 901);
    assert!(c.ack(&id, 123, &ready).is_err());
    reborn(&fixture, 900);
    c.ack(&id, 123, &ready).expect("ack");
    assert!(c.due());
    let resumed = Coordinator::new(&fixture.root);
    assert_eq!(resumed.intent.as_ref().unwrap().value("id"), id);
    fixture.put("/proc/sys/kernel/random/boot_id", "two");
    assert!(Coordinator::new(&fixture.root).intent.is_none());
}

/// `c.wait_seconds(next)` against the real clock: within 20 ms of `expected` seconds.
fn assert_wait(c: &Coordinator, next_sample_ns: i128, expected: f64) {
    let waited = c.wait_ns(next_sample_ns) as f64 / 1e9;
    assert!(
        waited <= expected && waited >= (expected - 0.02).max(0.),
        "{waited} vs {expected}"
    );
}

fn now() -> i128 {
    i128::from(clock::monotonic_ns())
}

#[test]
fn idle_waits_for_observation_but_never_past_shutdown_deadline() {
    let fixture = fixture();
    let mut c = Coordinator::new(&fixture.root);
    // One observation wake each second instead of four accept timeouts.
    assert_wait(&c, now() + 1_000_000_000, 1.0);
    request(&mut c, "poweroff", "low_battery", 3);
    let deadline = now() + 125_000_000;
    c.intent
        .as_mut()
        .unwrap()
        .insert("deadline_monotonic_ns", deadline as i64);
    assert_wait(&c, now() + 1_000_000_000, 0.125);
    expire(&mut c);
    assert_eq!(c.wait_ns(now() + 1_000_000_000), 0);
    assert!(c.due());
    execute(&mut c, answers(false));
    // A failed init request must not turn an expired deadline into a spin.
    assert_wait(&c, now() + 1_000_000_000, 1.0);
    assert_eq!(c.wait_ns(now() - 1), 0);
}
