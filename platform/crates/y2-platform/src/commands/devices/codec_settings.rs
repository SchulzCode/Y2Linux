//! `y2-platform codec-settings [--{sbc,ldac,aac}-quality …] [--{sbc,ldac,aac}-abr on|off]`: the
//! owner's Bluetooth codec quality. Validated codec quality preferences, separated from the active
//! daemon contract: they take effect at the next bluealsad start (a player restart), never by
//! restarting the daemon here. Errors print `{"schema": "org.y2linux.codec-settings/v1", "failure": …}`.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::At;
use crate::{
    commands::codec::{inventory_codec, requested, validate},
    reply::{atomic_json, Failure, Outcome, Reply},
};
use serde::Serialize;
use serde_json::{json, Value};
use y2p_core::py::PyValue;
use y2p_core::{
    json::{sorted, Object, ObjectExt},
    py::{is_true, truthy},
    Root,
};

const SCHEMA: &str = "org.y2linux.codec-settings/v1";
/// The order `requested` is printed in (the defaults' order after schema and experimental).
const KEYS: [&str; 8] = [
    "schema",
    "experimental",
    "sbc_quality",
    "sbc_abr",
    "ldac_quality",
    "ldac_abr",
    "aac_quality",
    "aac_abr",
];

/// The options given (`None` for the others); `on`/`off` already as booleans.
pub struct Changes {
    pub sbc_quality: Option<String>,
    pub sbc_abr: Option<bool>,
    pub ldac_quality: Option<String>,
    pub ldac_abr: Option<bool>,
    pub aac_quality: Option<String>,
    pub aac_abr: Option<bool>,
}

impl Changes {
    /// The given options as policy keys, in the policy's key order.
    fn given(self) -> Vec<(&'static str, Value)> {
        [
            ("sbc_quality", self.sbc_quality.map(Value::from)),
            ("sbc_abr", self.sbc_abr.map(Value::from)),
            ("ldac_quality", self.ldac_quality.map(Value::from)),
            ("ldac_abr", self.ldac_abr.map(Value::from)),
            ("aac_quality", self.aac_quality.map(Value::from)),
            ("aac_abr", self.aac_abr.map(Value::from)),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.map(|v| (k, v)))
        .collect()
    }
}

/// The status after the changes, or `{"schema": …, "failure": …}` with exit 1 (Reborn shows it).
pub fn configure(root: &Root, changes: Changes) -> Outcome {
    match configure_inner(root, changes) {
        Ok(value) => Reply::unsorted(&value, 0),
        Err(error) => Reply::unsorted(
            &Object::new()
                .with("schema", SCHEMA)
                .with("failure", error.message()),
            1,
        ),
    }
}

fn configure_inner(root: &Root, changes: Changes) -> Result<Status, Failure> {
    let changes = changes.given();
    if changes.is_empty() {
        return status(root);
    }
    // /run/y2 is not created here: without it the lock fails (the runtime is not up).
    let path = root.path("/run/y2/codec-settings.lock");
    let _lock = y2p_core::lock::try_exclusive(&path)
        .at(&path)?
        .ok_or_else(|| Failure::Os {
            text: "[Errno 11] Resource temporarily unavailable".into(),
            class: "BlockingIOError",
        })?;
    let mut merged = requested(root)?;
    for (key, value) in &changes {
        merged.insert((*key).into(), value.clone());
    }
    let value = validate(merged)?;
    // The saved policy is validated again here (status), also when this change would repair it.
    let caps = status(root)?;
    if changes.iter().any(|(k, _)| k.starts_with("ldac_")) && !caps.listed("ldac_quality") {
        return Err(Failure::value("ldac_encoder_not_compiled"));
    }
    if changes.iter().any(|(k, _)| k.starts_with("aac_")) && !caps.listed("aac_quality") {
        return Err(Failure::value("aac_encoder_not_compiled"));
    }
    atomic_json(
        &root.path("/data/bluetooth/codec-policy.json"),
        &value,
        true,
    )?;
    status(root)
}

/// What `codec-settings` prints, in this key order (nested objects in theirs).
#[derive(Serialize)]
struct Status {
    schema: &'static str,
    requested: Object,
    effective: Value,
    effective_scope: &'static str,
    pending_restart: bool,
    apply_action: &'static str,
    supported: Object,
}

impl Status {
    /// Whether `key` lists any supported choice.
    fn listed(&self, key: &str) -> bool {
        self.supported
            .get(key)
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty())
    }
}

fn status(root: &Root) -> Result<Status, Failure> {
    let policy = validate(requested(root)?)?;
    let inventory = root.json_object("/etc/y2linux/bluetooth-codecs.json");
    let applied = root.json_object("/run/y2/codec-runtime.json");
    let boot = root.boot_id();
    // This receipt records startup arguments, never endpoint/negotiation success.
    let pid = root.integer("/run/y2/bluealsa.pid");
    let command = match pid {
        Some(pid) if pid > 1 => root.read_limited(&format!("/proc/{pid}/cmdline"), 8192),
        _ => None,
    };
    let argv: Vec<&str> = match command.as_deref() {
        Some(text) if !text.is_empty() => text.split('\0').collect(),
        _ => Vec::new(),
    };
    let live = argv
        .first()
        .is_some_and(|program| program.rsplit('/').next() == Some("bluealsad"))
        && recorded_arguments_present(applied.get("arguments"), &argv)?;
    let same_boot =
        applied.value("boot_id") == json!(boot) && boot.as_deref().is_some_and(|b| !b.is_empty());
    let effective = if live && same_boot {
        // Printed with its keys sorted, as the runtime record is written.
        sorted(applied.get("settings").cloned().unwrap_or(Value::Null))
    } else {
        Value::Null
    };
    let mut requested = Object::new();
    for key in KEYS {
        requested.set(key, policy.get(key).cloned().unwrap_or(Value::Null));
    }
    let pending_restart = !effective.py_eq(&Value::Object(policy));
    // The build's codec inventory: `codecs` and each codec in it must be objects.
    let compiled = |name: &str| -> Result<Option<Value>, Failure> {
        let codec = inventory_codec(&inventory, name)?;
        Ok(codec.and_then(|c| c.get("compiled_locally").cloned()))
    };
    let (aac, ldac) = (compiled("AAC")?, compiled("LDAC")?);
    let listed = |v: &Option<Value>, choices: Value| {
        if v.as_ref().is_some_and(truthy) {
            choices
        } else {
            json!([])
        }
    };
    let supported = Object::new()
        .with("sbc_quality", json!(["high", "xq", "xq+"]))
        .with("sbc_abr", true)
        .with("aac_quality", listed(&aac, json!(["standard", "high"])))
        .with("aac_abr", is_true(aac.as_ref()))
        .with(
            "ldac_quality",
            listed(&ldac, json!(["mobile", "standard", "high"])),
        )
        .with("ldac_abr", is_true(ldac.as_ref()));
    Ok(Status {
        schema: SCHEMA,
        requested,
        effective,
        effective_scope: "daemon_start_arguments",
        pending_restart,
        apply_action: "restart_player",
        supported,
    })
}

/// Whether every argument the runtime record lists is in the daemon's argv: a list's items, a
/// string's characters, an object's keys (none listed: all present). Any other JSON value is a broken
/// record, refused as `invalid_codec_runtime_record`.
fn recorded_arguments_present(arguments: Option<&Value>, argv: &[&str]) -> Result<bool, Failure> {
    let has = |text: &str| argv.contains(&text);
    Ok(match arguments {
        None => true,
        Some(Value::Array(items)) => items.iter().all(|item| item.as_str().is_some_and(has)),
        Some(Value::String(text)) => text.chars().all(|c| has(c.encode_utf8(&mut [0; 4]))),
        Some(Value::Object(map)) => map.keys().all(|key| has(key)),
        Some(_) => return Err(Failure::value("invalid_codec_runtime_record")),
    })
}

#[cfg(test)]
#[path = "codec_settings_tests.rs"]
mod codec_settings_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_arguments_are_matched_by_shape() {
        let argv = ["/usr/bin/bluealsad", "-p", "a2dp-source", ""];
        assert!(recorded_arguments_present(Some(&json!(["-p", ""])), &argv).unwrap());
        assert!(!recorded_arguments_present(Some(&json!(["-p", 3])), &argv).unwrap());
        assert!(recorded_arguments_present(Some(&json!("p")), &argv).is_ok_and(|v| !v));
        assert!(recorded_arguments_present(None, &argv).unwrap());
        assert!(matches!(
            recorded_arguments_present(Some(&json!(null)), &argv),
            Err(Failure::Value(reason)) if reason == "invalid_codec_runtime_record"
        ));
    }
}
