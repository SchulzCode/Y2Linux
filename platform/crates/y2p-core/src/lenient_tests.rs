// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use serde_json::json;

#[derive(Deserialize, Default)]
#[serde(default)]
struct Stamp {
    boot_id: BootId,
    #[serde(deserialize_with = "number")]
    monotonic_s: Option<f64>,
}

fn stamp(value: Value) -> Stamp {
    read(value.as_object().cloned().unwrap_or_default())
}

#[test]
fn boot_id_compares_as_python_equality() {
    assert!(stamp(json!({})).boot_id.is(None));
    assert!(stamp(json!({"boot_id": null})).boot_id.is(None));
    assert!(!stamp(json!({"boot_id": null})).boot_id.is(Some("b")));
    assert!(stamp(json!({"boot_id": "b"})).boot_id.is(Some("b")));
    assert!(!stamp(json!({"boot_id": "b"})).boot_id.is(None));
    assert!(!stamp(json!({"boot_id": 7})).boot_id.is(None));
    assert!(!stamp(json!({"boot_id": ["b"]})).boot_id.is(Some("b")));
}

#[test]
fn number_takes_ints_floats_and_bools_only() {
    assert_eq!(stamp(json!({"monotonic_s": 3})).monotonic_s, Some(3.0));
    assert_eq!(stamp(json!({"monotonic_s": 2.5})).monotonic_s, Some(2.5));
    assert_eq!(stamp(json!({"monotonic_s": true})).monotonic_s, Some(1.0));
    assert_eq!(stamp(json!({"monotonic_s": "3"})).monotonic_s, None);
    assert_eq!(
        stamp(json!({"monotonic_s": 3, "boot_id": {}})).boot_id,
        BootId::Other
    );
}
