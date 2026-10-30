//! `y2-platform capabilities` (`capabilities.status` and the tail of `cli.main`).
// SPDX-License-Identifier: GPL-2.0-only
use super::codec;
use crate::reply::{Failure, Outcome, Reply};
use serde_json::{json, Value};
use y2p_contract::Capabilities;
use y2p_core::Root;

/// The installed template is broken: no capability template of the contract's shape.
fn invalid_template() -> Failure {
    Failure::value("invalid_capabilities_template")
}

/// `y2-platform capabilities`: the installed template (`/etc/y2linux/capabilities.json`) with the
/// running release's identity, the owner's Wi-Fi/Bluetooth switches and what this build's codecs,
/// power save and DVFS ceiling allow. Without a template: only `schema`, `state` and
/// `platform_version`.
pub fn run(root: &Root) -> Outcome {
    let mut result = match root.json("/etc/y2linux/capabilities.json") {
        Some(template @ Value::Object(_)) => {
            serde_json::from_value::<Capabilities>(template).map_err(|_| invalid_template())?
        }
        _ => Capabilities {
            schema: y2p_contract::schema::CAPABILITIES.into(),
            platform_version: None,
            state: Some("Unavailable".into()),
            api_version: None,
            feature_contract_version: None,
            qualification_scope: None,
            capabilities: None,
        },
    };
    // Runtime identity always comes from the installed release, never the template.
    result.platform_version = root
        .json_object("/etc/y2linux/versions.json")
        .get("release_version")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let inventory = codec::Inventory::read(root)?;
    let policy = codec::requested(root)?;
    if let Some(caps) = result.capabilities.as_mut() {
        for (key, name) in [
            ("sbc", "SBC"),
            ("aac", "AAC"),
            ("aptx", "aptX"),
            ("aptx_hd", "aptX-HD"),
            ("ldac", "LDAC"),
        ] {
            let Some(cap) = caps.get_mut(&format!("bluetooth_{key}")) else {
                continue;
            };
            let actual = inventory.codec(name);
            cap.implemented = actual.compiled_locally;
            cap.enabled =
                actual.compiled_locally && (name == "SBC" || inventory.eligible(name, &policy));
            cap.qualified = actual.platform_qualified;
            cap.details.insert(
                "enablement_scope".into(),
                json!("daemon policy; active PCM reported separately"),
            );
        }
        if let Some(cap) = caps.get_mut("bluetooth_sbc_xq") {
            cap.enabled =
                policy.experimental && matches!(policy.sbc_quality.as_str(), "xq" | "xq+");
        }
        if let Some(cap) = caps.get_mut("wifi_power_save") {
            cap.enabled =
                super::connectivity::radio_policy::WifiPower::read(root).enabled == Some(true);
        }
        if let Some(cap) = caps.get_mut("dvfs") {
            let maximum = root.integer("/sys/module/cpu_dvfs/parameters/qualification_max_khz");
            cap.details.insert(
                "stock_high_opp_enabled".into(),
                json!(maximum.is_some_and(|m| m > 1_040_000)),
            );
            cap.details
                .insert("runtime_ceiling_khz".into(), json!(maximum));
        }
        // The owner's switches (a root overlay may retain an older template).
        for (name, path) in [
            ("wifi", "/data/network/enabled"),
            ("bluetooth", "/data/bluetooth/enabled"),
        ] {
            if let Some(cap) = caps.get_mut(name) {
                cap.enabled = root.read(path).as_deref() == Some("1");
            }
        }
    }
    Reply::sorted(&result, 0)
}

#[cfg(test)]
#[path = "capabilities_tests.rs"]
mod capabilities_tests;
