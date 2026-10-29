//! `capabilities`: what tests/test_hardware_final_contract.py checked of the Python platform
//! (capabilities.status). The template's own entries (usb_host disabled) are checked in Python, next
//! to the image's configuration files.
// SPDX-License-Identifier: GPL-2.0-only
use super::run;
use crate::testing::Fixture;
use serde_json::{json, Value};

fn capabilities(fixture: &Fixture) -> Value {
    serde_json::from_str::<Value>(&run(&fixture.root).unwrap().stdout).unwrap()["capabilities"]
        .clone()
}

#[test]
fn experimental_enablement_does_not_promote_qualification() {
    let fixture = Fixture::new();
    // The template's entries as platform/capabilities.json has them.
    fixture.put_json(
        "/etc/y2linux/capabilities.json",
        &json!({"schema": "org.y2linux.capabilities/v1", "capabilities": {
            "bluetooth_aac": {"implemented": true, "enabled": false, "qualified": false,
                              "experimental": true, "fallback": "SBC"},
            "usb_host": {"implemented": true, "enabled": false, "qualified": false,
                         "experimental": true, "physical_gate": "vbus"}}}),
    );
    fixture.put_json(
        "/etc/y2linux/bluetooth-codecs.json",
        &json!({"schema": 1, "codecs": {"AAC": {"compiled_locally": true,
                "owner_private_experiment": true, "platform_qualified": false}}}),
    );
    assert_eq!(capabilities(&fixture)["bluetooth_aac"]["enabled"], false);
    fixture.put(
        "/data/bluetooth/codec-policy.json",
        r#"{"schema":1,"experimental":true}"#,
    );
    let caps = capabilities(&fixture);
    let cap = &caps["bluetooth_aac"];
    assert_eq!(cap["implemented"], true);
    assert_eq!(cap["enabled"], true);
    assert_eq!(cap["qualified"], false);
    assert_eq!(caps["usb_host"]["enabled"], false);
}
