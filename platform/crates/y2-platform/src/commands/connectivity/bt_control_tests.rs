//! An explicit Bluetooth power action against the automatic reconnect owner: what
//! tests/test_platform_bluetooth.py checked of the Python platform (bt_control.power).
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::{failed, Fixture};

#[test]
fn manual_operation_cannot_overlap_auto_and_unknown_completion_inhibits() {
    let fixture = Fixture::new();
    fixture.mkdir("/run/y2").put(
        "/proc/sys/kernel/random/boot_id",
        "11111111-1111-1111-1111-111111111111",
    );
    let answers = || vec![failed(&["bluetoothctl", "power", "on"], "command_timeout")];
    let automatic = y2p_core::lock::exclusive(&fixture.path("/run/y2/bt-operation.lock")).unwrap();
    let root = fixture.root.clone().with_answers(answers());
    let busy = power(&root, "on").unwrap();
    assert_eq!(
        busy.get("reason"),
        Some(&Value::from("platform_bluetooth_operation_in_progress"))
    );
    assert!(root.calls().is_empty());
    drop(automatic);
    let root = fixture.root.clone().with_answers(answers());
    let result = power(&root, "on").unwrap();
    assert_eq!(result.get("ok"), Some(&Value::from(false)));
    assert_eq!(root.calls().len(), 1);
    assert!(fixture
        .read("/run/y2/bt-intent/bt-control.ini")
        .contains("operation=uncertain"));
}
