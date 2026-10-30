//! Codec quality settings (`codec-settings`) against BlueALSA's start arguments (`codec-runtime`). A
//! non-boolean `*_abr` value cannot reach `configure` here (the parser's on|off choices; see
//! tests/connectivity.rs).
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::commands::codec::runtime;
use crate::testing::{repository, Fixture};
use serde_json::{json, Value};

const INVENTORY: &str = "/etc/y2linux/bluetooth-codecs.json";
const POLICY: &str = "/data/bluetooth/codec-policy.json";

/// An owner-private integration build: every optional encoder compiled.
fn private_integration() -> Fixture {
    let fixture = Fixture::new();
    let mut caps = json!({});
    for name in ["AAC", "aptX", "aptX-HD", "LDAC"] {
        caps[name] = json!({"compiled_locally": true, "owner_private_experiment": true});
    }
    fixture
        .put("/proc/sys/kernel/random/boot_id", "current-boot")
        .mkdir("/run/y2")
        .mkdir("/data/bluetooth")
        .put_json(
            INVENTORY,
            &json!({"schema": 1, "private_integration_enabled": true, "codecs": caps}),
        );
    fixture
}

fn none() -> Changes {
    Changes {
        sbc_quality: None,
        sbc_abr: None,
        ldac_quality: None,
        ldac_abr: None,
        aac_quality: None,
        aac_abr: None,
    }
}

/// `configure(root, changes)`: its exit code and printed record.
fn configure_with(fixture: &Fixture, changes: Changes) -> (u8, Value) {
    let reply = configure(&fixture.root, changes).expect("reply");
    (reply.code, serde_json::from_str(&reply.stdout).unwrap())
}

fn settings(fixture: &Fixture) -> Value {
    let (code, value) = configure_with(fixture, none());
    assert_eq!(code, 0, "{value}");
    value
}

fn arguments(fixture: &Fixture, record: bool) -> Vec<String> {
    let reply = runtime(&fixture.root, record).expect("arguments");
    reply.stdout.split(' ').map(str::to_owned).collect()
}

/// A running bluealsad started with `args` (its pid file and `/proc/<pid>/cmdline`).
fn running(fixture: &Fixture, args: &[String]) {
    let mut cmdline = String::from("/usr/bin/bluealsad\0");
    for arg in args {
        cmdline.push_str(arg);
        cmdline.push('\0');
    }
    fixture
        .put("/run/y2/bluealsa.pid", "123")
        .put("/proc/123/cmdline", &cmdline);
}

#[test]
fn private_integration_enables_all_endpoints_without_qualification_claim() {
    let fixture = private_integration();
    let args = arguments(&fixture, true);
    running(&fixture, &args);
    for name in ["AAC", "aptX", "aptX-HD", "LDAC"] {
        assert!(args.contains(&format!("--codec={name}")), "{name}");
    }
    assert_eq!(settings(&fixture)["pending_restart"], false);
    let (_, changed) = configure_with(
        &fixture,
        Changes {
            ldac_quality: Some("high".into()),
            ldac_abr: Some(false),
            ..none()
        },
    );
    assert_eq!(changed["pending_restart"], true);
    assert_eq!(changed["effective"]["ldac_quality"], "standard");
    assert_eq!(changed["effective"]["ldac_abr"], true);
    assert_eq!(changed["requested"]["ldac_abr"], false);
    let args = arguments(&fixture, true);
    assert!(!args.iter().any(|a| a == "--ldac-abr"));
    running(&fixture, &args);
    assert_eq!(settings(&fixture)["pending_restart"], false);
    fixture.put("/proc/sys/kernel/random/boot_id", "next-boot");
    assert!(settings(&fixture)["effective"].is_null());
}

#[test]
fn sbc_adaptive_bitpool_is_on_by_default_and_switchable() {
    let fixture = private_integration();
    let args = arguments(&fixture, false);
    let at = |arg: &str| args.iter().position(|a| a == arg).unwrap();
    assert!(args.iter().any(|a| a == "--sbc-abr")); // also for settings saved before 8.17
    assert!(at("--sbc-quality=high") < at("--sbc-abr"));
    let script = std::fs::read_to_string(
        repository().join("buildroot/board/y2/production-overlay/usr/libexec/y2/connectivity"),
    )
    .unwrap();
    assert!(script.contains("|--sbc-abr|"));
    let (_, value) = configure_with(
        &fixture,
        Changes {
            sbc_abr: Some(false),
            ..none()
        },
    );
    assert_eq!(
        json!([value["requested"]["sbc_abr"], value["supported"]["sbc_abr"]]),
        json!([false, true])
    );
    assert!(!arguments(&fixture, false).iter().any(|a| a == "--sbc-abr"));
}

#[test]
fn invalid_preferences_never_modify_saved_settings() {
    let fixture = private_integration();
    for value in ["unknown", "", ";reboot"] {
        for (changes, reason) in [
            (
                Changes {
                    ldac_quality: Some(value.into()),
                    ..none()
                },
                "invalid_ldac_quality",
            ),
            (
                Changes {
                    aac_quality: Some(value.into()),
                    ..none()
                },
                "invalid_aac_quality",
            ),
        ] {
            let (code, refused) = configure_with(&fixture, changes);
            assert_eq!(code, 1);
            assert_eq!(refused["failure"], reason);
        }
    }
    assert!(!fixture.exists(POLICY));
}

#[test]
fn aac_settings_are_saved_and_reported() {
    let fixture = private_integration();
    let value = settings(&fixture);
    assert_eq!(
        json!([
            value["requested"]["aac_quality"],
            value["requested"]["aac_abr"]
        ]),
        json!(["high", true])
    );
    assert_eq!(
        value["supported"]["aac_quality"],
        json!(["standard", "high"])
    );
    assert_eq!(
        value["supported"]["sbc_quality"],
        json!(["high", "xq", "xq+"])
    );
    let (_, value) = configure_with(
        &fixture,
        Changes {
            aac_quality: Some("standard".into()),
            aac_abr: Some(false),
            ..none()
        },
    );
    assert_eq!(
        json!([
            value["requested"]["aac_quality"],
            value["requested"]["aac_abr"]
        ]),
        json!(["standard", false])
    );
    assert_eq!(fixture.json(POLICY)["aac_quality"], "standard");
    let mut inventory = fixture.json(INVENTORY);
    inventory["codecs"]["AAC"]["compiled_locally"] = json!(false);
    fixture.put_json(INVENTORY, &inventory);
    let (code, refused) = configure_with(
        &fixture,
        Changes {
            aac_quality: Some("high".into()),
            ..none()
        },
    );
    assert_eq!(code, 1);
    assert_eq!(refused["failure"], "aac_encoder_not_compiled");
}
