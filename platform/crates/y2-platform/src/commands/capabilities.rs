//! `y2-platform capabilities` (`capabilities.status` and the tail of `cli.main`).
// SPDX-License-Identifier: GPL-2.0-only
use super::codec;
use crate::reply::{Failure, Outcome, Reply};
use serde_json::{json, Map, Value};
use y2p_core::{
    py::{eq_int, is_true},
    Root,
};

/// The entry `caps[name]` as a dict, for `caps[name][key] = …`; anything else (a JSON null too) is
/// Python's uncaught TypeError.
fn entry<'a>(
    caps: &'a mut Map<String, Value>,
    name: &str,
) -> Result<Option<&'a mut Map<String, Value>>, Failure> {
    match caps.get_mut(name) {
        None => Ok(None),
        Some(Value::Object(map)) => Ok(Some(map)),
        Some(other) => Err(codec::no_setitem(other)),
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
    let mut caps = match result.remove("capabilities") {
        None => None,
        Some(Value::Object(map)) => Some(map),
        // Not a dict (a JSON null too): `name in caps` then `caps[name]` raise TypeError; a list or
        // str without those names reaches `caps.get(...)` after the policy (AttributeError).
        Some(other) => {
            for name in ["wifi", "bluetooth"] {
                if codec::contains(&other, name)? {
                    return Err(codec::no_getitem(&other));
                }
            }
            codec::requested(root)?;
            return Err(codec::no_get(&other));
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
            let actual = codec::inventory_codec(&inventory, name)?;
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
