//! cli.py's `report` branch (tests/test_error_reports.py: Report).
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::Fixture;

/// This thread's nice value (Linux: `nice(2)` and `getpriority(PRIO_PROCESS, 0)` are per thread, so
/// the other tests keep theirs).
fn niceness() -> i32 {
    // SAFETY: getpriority only reads the calling thread's priority.
    unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) }
}

#[test]
fn the_command_saves_at_low_priority() {
    let fixture = Fixture::new();
    test_player::player(fixture.base());
    test_player::mount(fixture.base(), &["/media/sd"], "rw,relatime");
    let before = niceness();
    let reply = report(&fixture.root, "save", Some("sd"), Some("battery")).unwrap();
    assert_eq!(niceness(), (before + 10).min(19));
    assert_eq!(reply.code, 0);
    let result: serde_json::Value = serde_json::from_str(&reply.stdout).unwrap();
    assert_eq!(result["saved"], true);
    assert!(fixture
        .path("/media/sd")
        .join(result["file"].as_str().unwrap())
        .is_file());
}

#[test]
fn an_odd_state_field_reads_as_its_default_and_other_errors_keep_their_class() {
    let fixture = Fixture::new();
    test_player::player(fixture.base());
    // `seen` is no object here: it reads as empty, and the opt-in stays.
    fixture.put(state::STATE, r#"{"enabled": true, "seen": []}"#);
    let reply = report(&fixture.root, "check", None, None).unwrap();
    assert_eq!(reply.code, 0, "{}", reply.stdout);
    assert!(state::state(&fixture.root).enabled());
    let reply = report(&fixture.root, "save", None, None).unwrap();
    assert_eq!(
        (reply.code, reply.stdout.as_str()),
        (1, r#"{"failure":"report_failed","reason":"ValueError"}"#)
    );
}
