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
fn number_takes_ints_and_floats_only() {
    assert_eq!(stamp(json!({"monotonic_s": 3})).monotonic_s, Some(3.0));
    assert_eq!(stamp(json!({"monotonic_s": 2.5})).monotonic_s, Some(2.5));
    assert_eq!(stamp(json!({"monotonic_s": true})).monotonic_s, None);
    assert_eq!(stamp(json!({"monotonic_s": "3"})).monotonic_s, None);
    assert_eq!(
        stamp(json!({"monotonic_s": 3, "boot_id": {}})).boot_id,
        BootId::Other
    );
}

#[test]
fn this_boot_fields_match_as_boot_id_is() {
    use crate::root::tests::tempdir::Dir;
    let dir = Dir::new();
    let root = crate::Root::fixture(dir.path());
    // Unreadable boot id: only a missing or null field is this boot.
    assert!(root.is_this_boot(None) && root.is_this_boot(Some(&json!(null))));
    assert!(!root.is_this_boot(Some(&json!("a"))));
    std::fs::create_dir_all(dir.path().join("proc/sys/kernel/random")).unwrap();
    std::fs::write(dir.path().join("proc/sys/kernel/random/boot_id"), "a\n").unwrap();
    assert!(root.is_this_boot(Some(&json!("a"))));
    for other in [json!(null), json!("b"), json!(1), json!(true), json!(["a"])] {
        assert!(!root.is_this_boot(Some(&other)), "{other}");
    }
    assert!(!root.is_this_boot(None));
    for (field, boot) in [
        (json!("a"), Some("a")),
        (json!(null), None),
        (json!(7), Some("a")),
    ] {
        let parsed: BootId = serde_json::from_value(field.clone()).unwrap();
        assert_eq!(
            parsed.is(boot),
            field == json!("a") && boot == Some("a") || field.is_null() && boot.is_none()
        );
    }
}
