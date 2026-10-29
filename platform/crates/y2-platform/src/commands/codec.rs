//! The owner's Bluetooth codec policy and the BlueALSA arguments made from it (`codec-runtime`).
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::{atomic_json, Failure, Outcome, Reply};
use serde_json::{json, Map, Value};
use y2p_core::py::PyValue;
use y2p_core::{
    py::{eq_int, is_true},
    Root,
};

/// AAC "high": FDK afterburner and up to the peer's maximum rate where a frame still fits one packet
/// (AirPods: 256 kbit/s); "standard" is BlueALSA's tested 220 kbit/s. Adaptive rate steps CBR AAC
/// down while the link backs up, and the SBC bit-pool (53 -> 45 -> 35) the same way (BlueALSA patch
/// 0003).
fn defaults() -> [(&'static str, Value); 6] {
    [
        ("sbc_quality", json!("high")),
        ("sbc_abr", json!(true)),
        ("ldac_quality", json!("standard")),
        ("ldac_abr", json!(true)),
        ("aac_quality", json!("high")),
        ("aac_abr", json!(true)),
    ]
}

/// The owner's codec policy over the defaults (refused when its schema is not 1).
pub fn requested(root: &Root) -> Result<Map<String, Value>, Failure> {
    let inventory = root.json_object("/etc/y2linux/bluetooth-codecs.json");
    let policy = root.json_object("/data/bluetooth/codec-policy.json");
    let default = is_true(inventory.get("private_integration_enabled"));
    if !policy.is_empty() && !eq_int(policy.get("schema"), 1) {
        return Err(Failure::value("invalid_codec_policy"));
    }
    let mut result = Map::new();
    result.insert("schema".into(), json!(1));
    result.insert(
        "experimental".into(),
        policy
            .get("experimental")
            .cloned()
            .unwrap_or(json!(default)),
    );
    for (key, value) in defaults() {
        result.insert(key.into(), policy.get(key).cloned().unwrap_or(value));
    }
    Ok(result)
}

/// `validate(value)`.
pub fn validate(value: Map<String, Value>) -> Result<Map<String, Value>, Failure> {
    if ["experimental", "sbc_abr", "ldac_abr", "aac_abr"]
        .iter()
        .any(|k| !value.get(*k).is_some_and(Value::is_boolean))
    {
        return Err(Failure::value("invalid_codec_boolean"));
    }
    // SBC XQ is an owner choice (some SBC-only headphones sound much better, a few refuse it); the
    // default stays "high".
    let within = |key: &str, choices: &[&str]| {
        value
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|v| choices.contains(&v))
    };
    if !within("sbc_quality", &["high", "xq", "xq+"]) {
        return Err(Failure::value("invalid_sbc_quality"));
    }
    if !within("aac_quality", &["standard", "high"]) {
        return Err(Failure::value("invalid_aac_quality"));
    }
    if !within("ldac_quality", &["mobile", "standard", "high"]) {
        return Err(Failure::value("invalid_ldac_quality"));
    }
    Ok(value)
}

/// `inventory.get('codecs', {}).get(name, {})` and the `.get` callers make on it: a non-dict there
/// (a JSON null too) is Python's uncaught AttributeError.
pub fn inventory_codec<'a>(
    inventory: &'a Map<String, Value>,
    name: &str,
) -> Result<Option<&'a Map<String, Value>>, Failure> {
    match inventory.get("codecs") {
        None => Ok(None),
        Some(Value::Object(codecs)) => match codecs.get(name) {
            None => Ok(None),
            Some(Value::Object(codec)) => Ok(Some(codec)),
            Some(other) => Err(no_get(other)),
        },
        Some(other) => Err(no_get(other)),
    }
}

/// `value.get(...)` on a value that is not a dict: AttributeError.
pub fn no_get(value: &Value) -> Failure {
    Failure::attribute_error(format!(
        "'{}' object has no attribute 'get'",
        PyValue::type_name(value)
    ))
}

fn flag(codec: Option<&Map<String, Value>>, key: &str) -> bool {
    is_true(codec.and_then(|c| c.get(key)))
}

/// The inventory entry of codec `name` for `codec-runtime`: an inventory whose `codecs`, or whose
/// entry, is no object is refused (`invalid_codec_inventory`), so BlueALSA is never started with
/// arguments made from it.
fn runtime_codec<'a>(
    inventory: &'a Map<String, Value>,
    name: &str,
) -> Result<Option<&'a Map<String, Value>>, Failure> {
    inventory_codec(inventory, name).map_err(|_| Failure::value("invalid_codec_inventory"))
}

/// `y2-platform codec-runtime [--record]`: the BlueALSA arguments, space-separated, for
/// `usr/libexec/y2/connectivity` (which runs under `set -e`).
pub fn runtime(root: &Root, record: bool) -> Outcome {
    let inventory = root.json_object("/etc/y2linux/bluetooth-codecs.json");
    let policy = validate(requested(root)?)?;
    if !eq_int(inventory.get("schema"), 1) || !eq_int(policy.get("schema"), 1) {
        return Err(Failure::value("codec_policy_or_inventory_unavailable"));
    }
    let experimental = is_true(policy.get("experimental"));
    let mut args: Vec<String> = Vec::new();
    for name in ["AAC", "aptX", "aptX-HD", "LDAC"] {
        let cap = runtime_codec(&inventory, name)?;
        let eligible = flag(cap, "compiled_locally")
            && ((flag(cap, "distribution_approved") && flag(cap, "platform_qualified"))
                || (experimental && flag(cap, "owner_private_experiment")));
        if flag(cap, "compiled_locally") {
            args.push(format!("--codec={}{name}", if eligible { "" } else { "-" }));
        }
    }
    let text = |key: &str| {
        policy
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    args.push(format!("--sbc-quality={}", text("sbc_quality")));
    if is_true(policy.get("sbc_abr")) {
        args.push("--sbc-abr".into());
    }
    if args.iter().any(|a| a == "--codec=AAC") {
        // The patched encoder caps this by the peer and by one packet per frame.
        if text("aac_quality") == "high" {
            args.extend(["--aac-afterburner".into(), "--aac-bitrate=320000".into()]);
        }
        if is_true(policy.get("aac_abr")) {
            args.push("--aac-abr".into());
        }
    }
    if flag(runtime_codec(&inventory, "LDAC")?, "compiled_locally") {
        args.push(format!("--ldac-quality={}", text("ldac_quality")));
        if is_true(policy.get("ldac_abr")) {
            args.push("--ldac-abr".into());
        }
    }
    if record {
        let receipt = json!({"boot_id": root.boot_id(), "settings": policy,
                             "arguments": args});
        atomic_json(&root.path("/run/y2/codec-runtime.json"), &receipt, false)?;
    }
    Ok(Reply::text(args.join(" ")))
}

#[cfg(test)]
#[path = "codec_tests.rs"]
mod codec_tests;
