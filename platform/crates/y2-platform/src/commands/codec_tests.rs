//! BlueALSA's codec arguments (`codec-runtime`, radio_policy.codec_runtime): what
//! tests/test_radio_policy.py checked of the Python platform.
// SPDX-License-Identifier: GPL-2.0-only
use super::runtime;
use crate::reply::Failure;
use crate::testing::{repository, Fixture};
use serde_json::json;

const INVENTORY: &str = "/etc/y2linux/bluetooth-codecs.json";
const POLICY: &str = "/data/bluetooth/codec-policy.json";

fn arguments(fixture: &Fixture) -> Vec<String> {
    let reply = runtime(&fixture.root, false).expect("arguments");
    reply.stdout.split(' ').map(str::to_owned).collect()
}

fn has(args: &[String], arg: &str) -> bool {
    args.iter().any(|a| a == arg)
}

/// The production start script (`usr/libexec/y2/connectivity`), which accepts each argument by name.
fn start_script() -> String {
    std::fs::read_to_string(
        repository().join("buildroot/board/y2/production-overlay/usr/libexec/y2/connectivity"),
    )
    .unwrap()
}

#[test]
fn experimental_gate_is_explicit_and_sbc_fallback_is_retained() {
    let fixture = Fixture::new();
    let mut caps = json!({});
    for name in ["SBC", "AAC", "aptX", "aptX-HD", "LDAC"] {
        caps[name] = json!({"compiled_locally": true, "distribution_approved": name == "SBC",
                            "platform_qualified": false, "owner_private_experiment": name != "SBC"});
    }
    fixture.put_json(INVENTORY, &json!({"schema": 1, "codecs": caps}));
    let args = arguments(&fixture);
    assert!(has(&args, "--codec=-LDAC"));
    assert!(has(&args, "--codec=-AAC"));
    fixture.put_json(
        POLICY,
        &json!({"schema": 1, "experimental": true, "sbc_quality": "xq"}),
    );
    let args = arguments(&fixture);
    assert!(has(&args, "--codec=LDAC"));
    assert!(has(&args, "--codec=AAC"));
    assert!(has(&args, "--sbc-quality=xq"));
    assert!(!has(&args, "--codec=-SBC"));
    caps["AAC"]["compiled_locally"] = json!(false);
    fixture.put_json(INVENTORY, &json!({"schema": 1, "codecs": caps}));
    assert!(!has(&arguments(&fixture), "--codec=AAC"));
    // SBC XQ is an owner choice now, not an experiment.
    fixture.put_json(
        POLICY,
        &json!({"schema": 1, "experimental": false, "sbc_quality": "xq"}),
    );
    assert!(has(&arguments(&fixture), "--sbc-quality=xq"));
    fixture.put_json(POLICY, &json!({"schema": 1, "sbc_quality": "max"}));
    assert!(matches!(
        runtime(&fixture.root, false),
        Err(Failure::Value(_))
    ));
}

#[test]
fn aac_quality_and_adaptive_rate_arguments() {
    let fixture = Fixture::new();
    let mut caps = json!({});
    for name in ["SBC", "AAC", "LDAC"] {
        caps[name] = json!({"compiled_locally": true, "distribution_approved": true,
                            "platform_qualified": true});
    }
    fixture.put_json(INVENTORY, &json!({"schema": 1, "codecs": caps}));
    let args = arguments(&fixture);
    assert!(has(&args, "--codec=AAC"));
    let defaults = ["--aac-afterburner", "--aac-bitrate=320000", "--aac-abr"];
    for arg in defaults {
        assert!(has(&args, arg), "{arg}"); // the defaults
    }
    // The arguments are exactly the ones the start script accepts.
    let script = start_script();
    for arg in args.iter().filter(|a| a.starts_with("--aac-")) {
        assert!(script.contains(&format!("|{arg}")), "{arg}");
    }
    fixture.put_json(
        POLICY,
        &json!({"schema": 1, "aac_quality": "standard", "aac_abr": false}),
    );
    assert!(!arguments(&fixture).iter().any(|a| a.starts_with("--aac-")));
    caps["AAC"]["distribution_approved"] = json!(false);
    fixture.put_json(INVENTORY, &json!({"schema": 1, "codecs": caps}));
    fixture.put_json(POLICY, &json!({"schema": 1, "experimental": false}));
    let args = arguments(&fixture);
    assert!(has(&args, "--codec=-AAC"));
    assert!(!args.iter().any(|a| a.starts_with("--aac-")));
}

#[test]
fn an_inventory_whose_codecs_are_no_objects_is_refused() {
    let fixture = Fixture::new();
    fixture.mkdir("/run/y2");
    for codecs in [
        json!(null),
        json!("AAC"),
        json!({"AAC": null}),
        json!({"AAC": {}, "LDAC": [1]}),
    ] {
        fixture.put_json(INVENTORY, &json!({"schema": 1, "codecs": codecs}));
        let refused = runtime(&fixture.root, true).err().expect("refused");
        assert_eq!(refused.message(), "invalid_codec_inventory", "{codecs}");
        assert!(refused.caught(), "a refusal record, not a traceback");
        assert!(!fixture.exists("/run/y2/codec-runtime.json"));
    }
}
