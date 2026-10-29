//! The bounded subprocess runner (was tests/test_platform_contract.py).
// SPDX-License-Identifier: GPL-2.0-only
use super::*;

#[test]
fn subprocess_timeout_and_output_limit() {
    let root = Root::new("/");
    let started = Instant::now();
    let result = run(&root, &["/bin/sh", "-c", "sleep 10"], 0.1);
    assert_eq!(result.reason.as_deref(), Some("command_timeout"));
    assert!(started.elapsed() < Duration::from_secs(2));
    let result = run_with(
        &root,
        &["/bin/sh", "-c", "yes x | head -c 100000"],
        TIMEOUT,
        100,
        false,
    );
    assert_eq!(result.reason.as_deref(), Some("command_output_limit"));
}
