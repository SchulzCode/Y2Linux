// SPDX-License-Identifier: MIT
use super::*;
use serde_json::json;

#[test]
fn a_rename_is_one_line_with_its_name() {
    let request = Request::DeviceName {
        name: "Kitchen Y2".into(),
    };
    let line = serde_json::to_value(&request).unwrap_or_default();
    assert_eq!(line, json!({"op": "device_name", "name": "Kitchen Y2"}));
    let back: Result<Request, _> = serde_json::from_value(line);
    assert_eq!(back.ok(), Some(request));
}

#[test]
fn a_state_without_the_name_still_reads() {
    // What a daemon from before the rename sends (and what one sends without an adapter).
    let mut written = serde_json::to_value(Bluetooth::default()).unwrap_or_default();
    assert!(written.get("name").is_none(), "no name, no key");
    written["name"] = json!("Kitchen Y2");
    let named: Result<Bluetooth, _> = serde_json::from_value(written);
    assert_eq!(
        named.ok().and_then(|b| b.name).as_deref(),
        Some("Kitchen Y2")
    );
}
