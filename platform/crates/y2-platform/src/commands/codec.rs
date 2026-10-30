//! The owner's Bluetooth codec policy, the installed codec inventory, the BlueALSA arguments made
//! from them (`codec-runtime`) and the codec settings `codec-settings` and `status bluetooth` show.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::{atomic_json, Failure, Outcome, Reply};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use y2p_core::Root;

/// The installed codec inventory (what this build compiled and qualified).
pub const INVENTORY: &str = "/etc/y2linux/bluetooth-codecs.json";
/// The owner's codec policy (written by `codec-settings`).
pub const POLICY: &str = "/data/bluetooth/codec-policy.json";
/// What BlueALSA was last started with (`codec-runtime --record`).
pub const RECEIPT: &str = "/run/y2/codec-runtime.json";
/// The codec settings record's schema.
pub const SETTINGS_SCHEMA: &str = y2p_contract::schema::CODEC_SETTINGS;

/// The owner's codec policy over the defaults. AAC "high": FDK afterburner and up to the peer's
/// maximum rate where a frame still fits one packet (AirPods: 256 kbit/s); "standard" is BlueALSA's
/// tested 220 kbit/s. Adaptive rate steps CBR AAC down while the link backs up, and the SBC bit-pool
/// (53 -> 45 -> 35) the same way (BlueALSA patch 0003).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Policy {
    pub schema: i64,
    pub experimental: bool,
    pub sbc_quality: String,
    pub sbc_abr: bool,
    pub ldac_quality: String,
    pub ldac_abr: bool,
    pub aac_quality: String,
    pub aac_abr: bool,
}

/// The qualities each encoder takes. SBC XQ is an owner choice (some SBC-only headphones sound much
/// better, a few refuse it); the default stays "high".
const SBC_QUALITIES: [&str; 3] = ["high", "xq", "xq+"];
const AAC_QUALITIES: [&str; 2] = ["standard", "high"];
const LDAC_QUALITIES: [&str; 3] = ["mobile", "standard", "high"];

impl Policy {
    /// The policy of a build whose inventory enables private integration (`experimental`) or not.
    fn defaults(experimental: bool) -> Policy {
        Policy {
            schema: 1,
            experimental,
            sbc_quality: "high".into(),
            sbc_abr: true,
            ldac_quality: "standard".into(),
            ldac_abr: true,
            aac_quality: "high".into(),
            aac_abr: true,
        }
    }

    /// Refused when a quality is none its encoder takes.
    pub fn validate(&self) -> Result<(), Failure> {
        for (quality, choices, reason) in [
            (&self.sbc_quality, &SBC_QUALITIES[..], "invalid_sbc_quality"),
            (&self.aac_quality, &AAC_QUALITIES[..], "invalid_aac_quality"),
            (
                &self.ldac_quality,
                &LDAC_QUALITIES[..],
                "invalid_ldac_quality",
            ),
        ] {
            if !choices.contains(&quality.as_str()) {
                return Err(Failure::value(reason));
            }
        }
        Ok(())
    }
}

/// The installed codec inventory; an inventory of another shape (codecs, or an entry, that is no
/// object) is refused as `invalid_codec_inventory`, so nothing is decided from it.
#[derive(Deserialize, Default, Debug)]
#[serde(default)]
pub struct Inventory {
    pub schema: Option<i64>,
    pub private_integration_enabled: bool,
    #[serde(deserialize_with = "codec_entries")]
    pub codecs: BTreeMap<String, CodecCaps>,
}

/// The inventory's codecs: an object of objects (a list where an entry belongs is no entry).
fn codec_entries<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, CodecCaps>, D::Error> {
    use serde::de::Error;
    Map::<String, Value>::deserialize(deserializer)?
        .into_iter()
        .map(|(name, entry)| match entry {
            Value::Object(_) => CodecCaps::deserialize(entry)
                .map(|caps| (name, caps))
                .map_err(D::Error::custom),
            _ => Err(D::Error::custom("codec entry is no object")),
        })
        .collect()
}

/// One codec of the inventory.
#[derive(Serialize, Deserialize, Default, Clone, Copy, Debug)]
#[serde(default)]
pub struct CodecCaps {
    pub compiled_locally: bool,
    pub distribution_approved: bool,
    pub platform_qualified: bool,
    pub owner_private_experiment: bool,
}

impl Inventory {
    /// The installed inventory (empty when there is none).
    pub fn read(root: &Root) -> Result<Inventory, Failure> {
        match root.read(INVENTORY) {
            None => Ok(Inventory::default()),
            Some(text) => serde_json::from_str(&text).map_err(|_| invalid_inventory()),
        }
    }

    /// The entry of codec `name` (all false when the build does not list it).
    pub fn codec(&self, name: &str) -> CodecCaps {
        self.codecs.get(name).copied().unwrap_or_default()
    }

    /// Whether the daemon may offer codec `name` under `policy`: compiled, and approved and qualified
    /// or an owner's private experiment.
    pub fn eligible(&self, name: &str, policy: &Policy) -> bool {
        let caps = self.codec(name);
        caps.compiled_locally
            && ((caps.distribution_approved && caps.platform_qualified)
                || (policy.experimental && caps.owner_private_experiment))
    }
}

fn invalid_inventory() -> Failure {
    Failure::value("invalid_codec_inventory")
}

/// A policy field as `T` (absent: none); one of another type is refused with `reason`.
fn field<T: DeserializeOwned>(
    policy: &Map<String, Value>,
    key: &str,
    reason: &str,
) -> Result<Option<T>, Failure> {
    match policy.get(key) {
        None => Ok(None),
        Some(value) => T::deserialize(value)
            .map(Some)
            .map_err(|_| Failure::value(reason)),
    }
}

/// The owner's codec policy over the defaults: refused when its schema is not 1, a switch is no
/// bool or a quality none its encoder takes.
pub fn requested(root: &Root) -> Result<Policy, Failure> {
    let inventory = Inventory::read(root)?;
    let file = root.json_object(POLICY);
    let mut policy = Policy::defaults(inventory.private_integration_enabled);
    if file.is_empty() {
        return Ok(policy);
    }
    if file.get("schema").and_then(Value::as_i64) != Some(1) {
        return Err(Failure::value("invalid_codec_policy"));
    }
    let switch = |key: &str| field::<bool>(&file, key, "invalid_codec_boolean");
    let quality = |key: &str, reason: &str| field::<String>(&file, key, reason);
    policy.experimental = switch("experimental")?.unwrap_or(policy.experimental);
    policy.sbc_abr = switch("sbc_abr")?.unwrap_or(policy.sbc_abr);
    policy.ldac_abr = switch("ldac_abr")?.unwrap_or(policy.ldac_abr);
    policy.aac_abr = switch("aac_abr")?.unwrap_or(policy.aac_abr);
    if let Some(q) = quality("sbc_quality", "invalid_sbc_quality")? {
        policy.sbc_quality = q;
    }
    if let Some(q) = quality("aac_quality", "invalid_aac_quality")? {
        policy.aac_quality = q;
    }
    if let Some(q) = quality("ldac_quality", "invalid_ldac_quality")? {
        policy.ldac_quality = q;
    }
    policy.validate()?;
    Ok(policy)
}

/// What BlueALSA was started with (`codec-runtime --record`): this receipt records startup
/// arguments, never endpoint or negotiation success.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Receipt {
    pub boot_id: Option<String>,
    pub settings: Policy,
    pub arguments: Vec<String>,
}

/// The codec settings: the owner's policy, what BlueALSA was started with this boot (its effective
/// settings) and which qualities this build supports.
#[derive(Serialize, Clone, Debug)]
pub struct Settings {
    pub schema: &'static str,
    pub requested: Policy,
    pub effective: Option<Policy>,
    pub effective_scope: &'static str,
    pub pending_restart: bool,
    pub apply_action: &'static str,
    pub supported: Supported,
}

/// The qualities and adaptive rates this build's encoders take (none for one not compiled).
#[derive(Serialize, Clone, Debug)]
pub struct Supported {
    pub sbc_quality: &'static [&'static str],
    pub sbc_abr: bool,
    pub aac_quality: &'static [&'static str],
    pub aac_abr: bool,
    pub ldac_quality: &'static [&'static str],
    pub ldac_abr: bool,
}

/// The settings of this boot.
pub fn settings(root: &Root) -> Result<Settings, Failure> {
    let requested = requested(root)?;
    let inventory = Inventory::read(root)?;
    let receipt: Option<Receipt> = root
        .read(RECEIPT)
        .and_then(|text| serde_json::from_str(&text).ok());
    let boot = root.boot_id().filter(|b| !b.is_empty());
    let pid = root.integer("/run/y2/bluealsa.pid");
    let command = match pid {
        Some(pid) if pid > 1 => root.read_limited(&format!("/proc/{pid}/cmdline"), 8192),
        _ => None,
    };
    let argv: Vec<&str> = command
        .as_deref()
        .filter(|c| !c.is_empty())
        .map_or_else(Vec::new, |c| c.split('\0').collect());
    let running = argv
        .first()
        .is_some_and(|program| program.rsplit('/').next() == Some("bluealsad"));
    let effective = receipt
        .filter(|r| running && boot.is_some() && r.boot_id == boot)
        .filter(|r| r.arguments.iter().all(|a| argv.contains(&a.as_str())))
        .map(|r| r.settings);
    let (aac, ldac) = (
        inventory.codec("AAC").compiled_locally,
        inventory.codec("LDAC").compiled_locally,
    );
    let listed = |compiled: bool, choices: &'static [&'static str]| {
        if compiled {
            choices
        } else {
            &[]
        }
    };
    Ok(Settings {
        schema: SETTINGS_SCHEMA,
        pending_restart: effective.as_ref() != Some(&requested),
        requested,
        effective,
        effective_scope: "daemon_start_arguments",
        apply_action: "restart_player",
        supported: Supported {
            sbc_quality: &SBC_QUALITIES,
            sbc_abr: true,
            aac_quality: listed(aac, &AAC_QUALITIES),
            aac_abr: aac,
            ldac_quality: listed(ldac, &LDAC_QUALITIES),
            ldac_abr: ldac,
        },
    })
}

/// `y2-platform codec-runtime [--record]`: the BlueALSA arguments, space-separated, for
/// `usr/libexec/y2/connectivity` (which runs under `set -e`).
pub fn runtime(root: &Root, record: bool) -> Outcome {
    let inventory = Inventory::read(root)?;
    let policy = requested(root)?;
    if inventory.schema != Some(1) {
        return Err(Failure::value("codec_policy_or_inventory_unavailable"));
    }
    let mut args: Vec<String> = Vec::new();
    for name in ["AAC", "aptX", "aptX-HD", "LDAC"] {
        if inventory.codec(name).compiled_locally {
            let sign = if inventory.eligible(name, &policy) {
                ""
            } else {
                "-"
            };
            args.push(format!("--codec={sign}{name}"));
        }
    }
    args.push(format!("--sbc-quality={}", policy.sbc_quality));
    if policy.sbc_abr {
        args.push("--sbc-abr".into());
    }
    if args.iter().any(|a| a == "--codec=AAC") {
        // The patched encoder caps this by the peer and by one packet per frame.
        if policy.aac_quality == "high" {
            args.extend(["--aac-afterburner".into(), "--aac-bitrate=320000".into()]);
        }
        if policy.aac_abr {
            args.push("--aac-abr".into());
        }
    }
    if inventory.codec("LDAC").compiled_locally {
        args.push(format!("--ldac-quality={}", policy.ldac_quality));
        if policy.ldac_abr {
            args.push("--ldac-abr".into());
        }
    }
    if record {
        let receipt = Receipt {
            boot_id: root.boot_id(),
            settings: policy,
            arguments: args.clone(),
        };
        atomic_json(&root.path(RECEIPT), &receipt, false)?;
    }
    Ok(Reply::text(args.join(" ")))
}

#[cfg(test)]
#[path = "codec_tests.rs"]
mod codec_tests;
