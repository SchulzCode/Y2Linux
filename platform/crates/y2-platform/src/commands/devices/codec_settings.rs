//! `codec_controls.configure` and `status`: `y2-platform codec-settings [--{sbc,ldac,aac}-quality …]
//! [--{sbc,ldac,aac}-abr on|off]`. Validated codec quality preferences, separated from the active
//! daemon contract: they take effect at the next bluealsad start (a player restart), never by
//! restarting the daemon here. Errors print `{"schema": "org.y2linux.codec-settings/v1", "failure": …}`.
// SPDX-License-Identifier: GPL-2.0-only
use crate::{
    commands::codec::{inventory_codec, not_iterable, requested, validate},
    reply::{atomic_json, Failure, Outcome, Reply},
};
use serde::Serialize;
use serde_json::{json, Value};
use y2p_core::{
    json::Ordered,
    py::{is_true, truthy},
    Root,
};

const SCHEMA: &str = "org.y2linux.codec-settings/v1";
/// requested()'s key order (the defaults' order after schema and experimental).
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

/// The options given (argparse's None for the others); `on`/`off` already as booleans.
pub struct Changes {
    pub sbc_quality: Option<String>,
    pub sbc_abr: Option<bool>,
    pub ldac_quality: Option<String>,
    pub ldac_abr: Option<bool>,
    pub aac_quality: Option<String>,
    pub aac_abr: Option<bool>,
}

impl Changes {
    /// `{k: v for k, v in changes.items() if v is not None}` in the keyword order.
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

pub fn configure(root: &Root, changes: Changes) -> Outcome {
    match configure_inner(root, changes) {
        Ok(value) => Reply::unsorted(&value, 0),
        // `except (ValueError, OSError)`: anything else leaves the command.
        Err(error) if !error.caught() => Err(error),
        Err(error) => Reply::unsorted(
            &Ordered::new()
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
    // No private_directory: a missing /run/y2 is the lock's OSError.
    let path = root.path("/run/y2/codec-settings.lock");
    let _lock = y2p_core::lock::try_exclusive(&path)
        .map_err(|e| Failure::os(e, &path))?
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

/// `codec_controls.status(ctx)` in its key order (nested dicts too).
#[derive(Serialize)]
struct Status {
    schema: &'static str,
    requested: Ordered,
    effective: Value,
    effective_scope: &'static str,
    pending_restart: bool,
    apply_action: &'static str,
    supported: Ordered,
}

impl Status {
    /// `bool(caps[key])` of a supported list.
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
    let boot = root.read("/proc/sys/kernel/random/boot_id");
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
    let live = !argv.is_empty()
        && argv[0].rsplit('/').next() == Some("bluealsad")
        && recorded_arguments_present(applied.get("arguments"), &argv)?;
    let same_boot = applied.get("boot_id").cloned().unwrap_or(Value::Null) == json!(boot)
        && boot.as_deref().is_some_and(|b| !b.is_empty());
    let effective = if live && same_boot {
        applied.get("settings").cloned().unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let mut requested = Ordered::new();
    for key in KEYS {
        requested.set(key, policy.get(key).cloned().unwrap_or(Value::Null));
    }
    let pending_restart = !python_equal(&effective, &Value::Object(policy));
    let compiled = |name: &str| -> Result<Option<Value>, Failure> {
        Ok(inventory_codec(&inventory, name)?.and_then(|c| c.get("compiled_locally").cloned()))
    };
    let (aac, ldac) = (compiled("AAC")?, compiled("LDAC")?);
    let listed = |v: &Option<Value>, choices: Value| {
        if v.as_ref().is_some_and(truthy) {
            choices
        } else {
            json!([])
        }
    };
    let supported = Ordered::new()
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

/// `all(arg in argv for arg in applied.get('arguments', []))`: a string iterates its characters, a
/// dict its keys; anything else that is not iterable is a TypeError in Python (its traceback).
fn recorded_arguments_present(arguments: Option<&Value>, argv: &[&str]) -> Result<bool, Failure> {
    let has = |text: &str| argv.contains(&text);
    Ok(match arguments {
        None => true,
        Some(Value::Array(items)) => items.iter().all(|item| item.as_str().is_some_and(has)),
        Some(Value::String(text)) => text.chars().all(|c| has(c.encode_utf8(&mut [0; 4]))),
        Some(Value::Object(map)) => map.keys().all(|key| has(key)),
        Some(other) => return Err(not_iterable(other)),
    })
}

/// Python `==` of two parsed JSON values: numbers by value (`1 == 1.0`, `True == 1`), dicts without
/// regard to key order.
fn python_equal(a: &Value, b: &Value) -> bool {
    let numeric = |v: &Value| match v {
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        Value::Number(n) => n.as_f64(),
        _ => None,
    };
    match (a, b) {
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| python_equal(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| python_equal(v, w)))
        }
        _ => match (numeric(a), numeric(b)) {
            (Some(x), Some(y)) => x == y,
            _ => a == b,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equality_and_membership_follow_python() {
        assert!(python_equal(
            &json!({"a": 1, "b": [true]}),
            &json!({"b": [1.0], "a": 1.0})
        ));
        assert!(!python_equal(&json!(null), &json!({})));
        assert!(!python_equal(&json!("1"), &json!(1)));
        let argv = ["/usr/bin/bluealsad", "-p", "a2dp-source", ""];
        assert!(recorded_arguments_present(Some(&json!(["-p", ""])), &argv).unwrap());
        assert!(!recorded_arguments_present(Some(&json!(["-p", 3])), &argv).unwrap());
        assert!(recorded_arguments_present(Some(&json!("p")), &argv).is_ok_and(|v| !v));
        assert!(recorded_arguments_present(None, &argv).unwrap());
        assert!(matches!(
            recorded_arguments_present(Some(&json!(null)), &argv),
            Err(Failure::Crash {
                class: "TypeError",
                ..
            })
        ));
    }
}
