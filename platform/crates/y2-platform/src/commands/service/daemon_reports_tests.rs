//! The service starts `report work` in its own process, and only when due (tests/test_report_upload.py:
//! Service).
//!
//! Below a fixture root nothing starts (process.rs refuses it like a missing binary); a start was asked
//! for when the tick set the next send gap from its time. A run still in progress is a real child.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::commands::reports::state::{set_enabled, QUEUE, STATE, UPLOAD_KEY};
use crate::testing::Fixture;

const KEY: &str = "kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk";

/// The service's error-report ticks and the `report work` runs they started.
struct Service {
    fixture: Fixture,
    started: usize,
}

impl Service {
    fn new() -> Self {
        Self {
            fixture: Fixture::new(),
            started: 0,
        }
    }

    fn root(&self) -> &Root {
        &self.fixture.root
    }

    fn key(&self) {
        self.fixture
            .put_json(UPLOAD_KEY, &json!({"upload_key": KEY}));
    }

    fn tick(&mut self, run: Option<Reports>, now: f64, network: &str) -> Reports {
        let before = run.as_ref().and_then(|r| r.send_after);
        let state = Ordered::new().with("state", network);
        let run = error_reports(self.root(), &state, run, now).unwrap();
        if run.send_after != before && run.send_after == Some(now + REPORT_SEND_GAP_S) {
            self.started += 1;
        }
        run
    }
}

/// A `report work` run that is still going (a real child in its own session).
fn running(started: f64) -> Option<(Child, f64)> {
    let child = process::spawn(&Root::new("/"), &["sleep", "30"], Start::Service).unwrap();
    Some((child, started))
}

#[test]
fn nothing_runs_without_an_upload_key_or_opt_in() {
    let _env = crate::commands::reports::fixture_environment();
    let mut service = Service::new();
    set_enabled(service.root(), true).unwrap();
    let run = service.tick(None, 0.0, "Online");
    let (next_check, send_after) = (run.next_check, run.send_after);
    let run = service.tick(Some(run), 10_000.0, "Online");
    assert_eq!((run.next_check, run.send_after), (next_check, send_after));
    assert_eq!(service.started, 0);
    service.key();
    set_enabled(service.root(), false).unwrap();
    service.tick(Some(run), 10_000.0, "Online");
    assert_eq!(service.started, 0);
}

#[test]
fn checks_run_after_start_then_every_period_and_never_twice_at_once() {
    let _env = crate::commands::reports::fixture_environment();
    let mut service = Service::new();
    service.key();
    set_enabled(service.root(), true).unwrap();
    let run = service.tick(None, 1000.0, "Online");
    assert_eq!(service.started, 0); // not right at start
    let mut run = service.tick(Some(run), 1000.0 + REPORT_FIRST_S, "Online");
    assert_eq!(service.started, 1);
    run.process = running(1000.0 + REPORT_FIRST_S); // still running
    let mut run = service.tick(
        Some(run),
        1000.0 + REPORT_FIRST_S + REPORT_LIMIT_S - 1.0,
        "Online",
    );
    assert_eq!(service.started, 1);
    let (mut child, started) = run.process.take().expect("kept while it runs");
    child.kill();
    assert!(child.wait(5.0));
    run.process = Some((child, started)); // ended
    service.tick(
        Some(run),
        1000.0 + REPORT_FIRST_S + REPORT_CHECK_S,
        "Online",
    );
    assert_eq!(service.started, 2);
}

#[test]
fn a_due_report_is_sent_before_the_next_check_but_not_every_pass() {
    let _env = crate::commands::reports::fixture_environment();
    let mut service = Service::new();
    service.key();
    set_enabled(service.root(), true).unwrap();
    let run = service.tick(None, 1000.0, "Online");
    let name = "20261019T120000Z-0a0a0a0a-0a0a-4a0a-8a0a-0a0a0a0a0a0a.tar.gz";
    service.fixture.put(&format!("{QUEUE}/{name}"), "queued");
    let run = service.tick(Some(run), 1030.0, "Connecting");
    assert_eq!(service.started, 0); // waits for Wi-Fi
    let run = service.tick(Some(run), 1060.0, "Online");
    assert_eq!(service.started, 1);
    let run = service.tick(Some(run), 1090.0, "Online");
    assert_eq!(service.started, 1); // blocked upload: not every pass
    let run = service.tick(Some(run), 1060.0 + REPORT_SEND_GAP_S, "Online");
    assert_eq!(service.started, 2);
    // A report waiting for its retry time does not start a run.
    let mut state = reports::state(service.root());
    state.set(
        "uploads",
        Py::dict().with(
            name,
            Py::dict()
                .with("attempts", Py::int(1))
                .with("next", Py::Float(time::wall() + 3600.0)),
        ),
    );
    reports::save(service.root(), &state).unwrap();
    service.tick(Some(run), 1060.0 + 2.0 * REPORT_SEND_GAP_S, "Online");
    assert_eq!(service.started, 2);
}

#[test]
fn a_broken_reports_record_never_stops_the_service_loop() {
    let _env = crate::commands::reports::fixture_environment();
    let mut service = Service::new();
    service.key();
    service.fixture.put_json(
        STATE,
        &json!({"enabled": true, "uploads": ["not", "a", "dict"]}),
    );
    service.fixture.put(&format!("{QUEUE}/a.tar.gz"), "x");
    let run = service.tick(None, 0.0, "Online");
    service.tick(Some(run), REPORT_FIRST_S, "Online"); // no error
}

#[test]
fn a_stuck_run_is_stopped() {
    let _env = crate::commands::reports::fixture_environment();
    let mut service = Service::new();
    service.key();
    set_enabled(service.root(), true).unwrap();
    let run = service.tick(None, 0.0, "Online");
    let mut run = service.tick(Some(run), REPORT_FIRST_S, "Online");
    run.process = running(REPORT_FIRST_S);
    let pid = run.process.as_ref().unwrap().0.pid();
    let run = service.tick(Some(run), REPORT_FIRST_S + REPORT_LIMIT_S, "Online");
    assert!(run.process.is_none());
    // SIGKILLed with its tree and reaped.
    // SAFETY: signal 0 only asks whether the process exists.
    let alive = unsafe { libc::kill(pid, 0) } == 0;
    assert!(!alive);
}
