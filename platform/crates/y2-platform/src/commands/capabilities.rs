//! `y2-platform capabilities` (`capabilities.status` and the tail of `cli.main`).
// SPDX-License-Identifier: GPL-2.0-only
use super::codec;
use crate::reply::{Failure, Outcome, Reply};
use serde_json::{json, Map, Value};
use y2p_core::{
    py::{eq_int, is_true},
    Root,
};

/// The installed template is broken: its capabilities, or one of the entries this fills in, are no
/// object.
fn invalid_template() -> Failure {
    Failure::value("invalid_capabilities_template")
}

/// The entry `caps[name]` as an object to fill in; one that is anything else (a JSON null too)
/// refuses the template.
fn entry<'a>(
    caps: &'a mut Map<String, Value>,
    name: &str,
) -> Result<Option<&'a mut Map<String, Value>>, Failure> {
    match caps.get_mut(name) {
        None => Ok(None),
        Some(Value::Object(map)) => Ok(Some(map)),
        Some(_) => Err(invalid_template()),
    }
}

pub fn run(root: &Root) -> Outcome {
    let mut result = match root.json("/etc/y2linux/capabilities.json") {
        Some(Value::Object(map)) => map,
        _ => serde_json::from_value(
            json!({"schema": y2p_contract::schema::CAPABILITIES, "state": "Unavailable"}),
        )
        .expect("literal"),
    };
    let version = root
        .json_object("/etc/y2linux/versions.json")
        .get("release_version")
        .cloned()
        .unwrap_or(Value::Null);
    result.insert("platform_version".into(), version.clone());
    let mut caps = match result.shift_remove("capabilities") {
        None => None,
        Some(Value::Object(map)) => Some(map),
        // Capabilities that are no object refuse the template. A list or text naming neither radio is
        // read only after the codec policy, whose own refusal comes first then.
        Some(other) => {
            let names_a_radio = ["wifi", "bluetooth"].iter().any(|name| match &other {
                Value::Array(items) => items.iter().any(|item| item.as_str() == Some(name)),
                Value::String(text) => text.contains(name),
                _ => true,
            });
            if !names_a_radio {
                codec::requested(root)?;
            }
            return Err(invalid_template());
        }
    };
    if let Some(caps) = caps.as_mut() {
        for (name, path) in [
            ("wifi", "/data/network/enabled"),
            ("bluetooth", "/data/bluetooth/enabled"),
        ] {
            if let Some(cap) = entry(caps, name)? {
                cap.insert("enabled".into(), json!(root.integer(path) == Some(1)));
            }
        }
    }
    let inventory = root.json_object("/etc/y2linux/bluetooth-codecs.json");
    let policy = codec::requested(root)?;
    if let Some(caps) = caps.as_mut() {
        for (key, name) in [
            ("sbc", "SBC"),
            ("aac", "AAC"),
            ("aptx", "aptX"),
            ("aptx_hd", "aptX-HD"),
            ("ldac", "LDAC"),
        ] {
            // `cap = caps.get(...)`; `if cap is None: continue` (also for a JSON null).
            if caps
                .get(&format!("bluetooth_{key}"))
                .is_none_or(Value::is_null)
            {
                continue;
            }
            let actual = codec::inventory_codec(&inventory, name)
                .map_err(|_| Failure::value("invalid_codec_inventory"))?;
            let Some(cap) = entry(caps, &format!("bluetooth_{key}"))? else {
                continue;
            };
            let flag = |k: &str| is_true(actual.and_then(|a| a.get(k)));
            let implemented = flag("compiled_locally");
            let enabled = implemented
                && (name == "SBC"
                    || (flag("distribution_approved") && flag("platform_qualified"))
                    || (eq_int(policy.get("schema"), 1)
                        && is_true(policy.get("experimental"))
                        && flag("owner_private_experiment")));
            cap.insert("implemented".into(), json!(implemented));
            cap.insert("enabled".into(), json!(enabled));
            cap.insert("qualified".into(), json!(flag("platform_qualified")));
            cap.insert(
                "enablement_scope".into(),
                json!("daemon policy; active PCM reported separately"),
            );
        }
        if let Some(cap) = entry(caps, "bluetooth_sbc_xq")? {
            let xq = eq_int(policy.get("schema"), 1)
                && is_true(policy.get("experimental"))
                && matches!(
                    policy.get("sbc_quality").and_then(Value::as_str),
                    Some("xq" | "xq+")
                );
            cap.insert("enabled".into(), json!(xq));
        }
        if let Some(cap) = entry(caps, "wifi_power_save")? {
            cap.insert(
                "enabled".into(),
                json!(is_true(
                    root.json_object("/run/y2/wifi-power.json").get("enabled")
                )),
            );
        }
        if let Some(cap) = entry(caps, "dvfs")? {
            let maximum = root.integer("/sys/module/cpu_dvfs/parameters/qualification_max_khz");
            cap.insert(
                "stock_high_opp_enabled".into(),
                json!(maximum.is_some_and(|m| m > 1_040_000)),
            );
            cap.insert("runtime_ceiling_khz".into(), json!(maximum));
        }
        // cli.main: a root overlay may retain an older capability template. Runtime identity always
        // comes from the installed release, never the template; the owner's switches as text.
        for (name, path) in [
            ("wifi", "/data/network/enabled"),
            ("bluetooth", "/data/bluetooth/enabled"),
        ] {
            if let Some(cap) = entry(caps, name)? {
                cap.insert(
                    "enabled".into(),
                    json!(root.read(path).as_deref() == Some("1")),
                );
            }
        }
    }
    if let Some(caps) = caps {
        result.insert("capabilities".into(), Value::Object(caps));
    }
    result.insert("platform_version".into(), version);
    Reply::sorted(&result, 0)
}

#[cfg(test)]
#[path = "capabilities_tests.rs"]
mod capabilities_tests;
