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
    let _env = crate::commands::reports::fixture_environment();
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
