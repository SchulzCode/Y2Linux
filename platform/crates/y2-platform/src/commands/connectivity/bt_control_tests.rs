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

#[test]
fn a_busy_power_change_asks_the_reconnect_owner_to_give_way() {
    // Test build 190: the owner paged headphones that were off for ≈20 s while the sleep helper
    // retried Bluetooth off; without the intent nothing told it a power change was waiting.
    let fixture = Fixture::new();
    fixture.mkdir("/run/y2").put(
        "/proc/sys/kernel/random/boot_id",
        "11111111-1111-1111-1111-111111111111",
    );
    let automatic = y2p_core::lock::exclusive(&fixture.path("/run/y2/bt-operation.lock")).unwrap();
    let busy = power(&fixture.root, "off").unwrap();
    assert_eq!(
        busy.get("reason"),
        Some(&Value::from("platform_bluetooth_operation_in_progress"))
    );
    assert!(fixture.root.calls().is_empty());
    assert!(fixture
        .read("/run/y2/bt-intent/bt-control.ini")
        .contains("operation=power_pending"));
    drop(automatic);
}
