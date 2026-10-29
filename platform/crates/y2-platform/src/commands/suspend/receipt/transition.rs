//! `sleep.transition`: the product state a receipt record leaves (requested, preparing, refused,
//! sleeping, waking, restored, restore_failed), proven only by the same boot's kernel journal and
//! Reborn's acknowledgement.
// SPDX-License-Identifier: GPL-2.0-only
use super::super::{is_text, py_gt, E};
use serde_json::{json, Map, Value};
use y2p_core::py::PyValue;
use y2p_core::{
    py::{is_true, truthy},
    suspend_text::fields,
};

pub(super) fn set(p: &mut Map<String, Value>, pairs: &[(&str, Value)]) {
    for (key, value) in pairs {
        p.insert((*key).to_owned(), value.clone());
    }
}

pub(super) fn state(p: &Map<String, Value>) -> &str {
    p.get("state").and_then(Value::as_str).unwrap_or_default()
}

/// `reason or fallback`.
fn either(reason: Option<&str>, fallback: Value) -> Value {
    match reason {
        Some(r) if !r.is_empty() => Value::String(r.to_owned()),
        _ => fallback,
    }
}

/// `name in container` for a receipt's IRQ counts (`value.get(key, {})`: a missing key is `{}`).
fn contains(container: Option<&Value>, name: &str) -> Result<bool, E> {
    match container {
        None => Ok(false),
        Some(Value::Object(map)) => Ok(map.contains_key(name)),
        Some(Value::Array(items)) => Ok(items.iter().any(|v| v.as_str() == Some(name))),
        Some(Value::String(text)) => Ok(text.contains(name)),
        Some(other) => Err(E::TypeError(format!(
            "argument of type '{}' is not iterable",
            other.type_name()
        ))),
    }
}

/// `container[name]` once `name in container` held.
fn index<'a>(container: Option<&'a Value>, name: &str) -> Result<&'a Value, E> {
    match container {
        Some(Value::Object(map)) => map
            .get(name)
            .ok_or_else(|| E::Invalid(format!("no {name} count"))),
        Some(Value::Array(_)) => Err(E::TypeError(
            "list indices must be integers or slices, not str".into(),
        )),
        _ => Err(E::TypeError(
            "string indices must be integers, not 'str'".into(),
        )),
    }
}

/// A `sleep.product` failure as the exception it is on this path.
pub fn raised(failure: crate::reply::Failure) -> E {
    use crate::reply::Failure;
    match failure {
        Failure::Value(text) => E::value(text),
        Failure::Crash {
            class: "TypeError",
            message,
        } => E::TypeError(message),
        other => E::Invalid(other.message()),
    }
}

/// `sleep.transition`: the product state after `stage`.
pub fn transition(
    value: &Map<String, Value>,
    stage: &str,
    result: &Value,
    boot_id: Option<&str>,
    reason: Option<&str>,
) -> Result<Map<String, Value>, E> {
    transition_keys(value, stage, result, boot_id, reason).map(|(p, _)| p)
}

/// `sleep.transition`, and whether the product holds a key that is no str (built from a list of
/// pairs): such a product is kept until the receipt is written, which then fails.
pub fn transition_keys(
    value: &Map<String, Value>,
    stage: &str,
    result: &Value,
    boot_id: Option<&str>,
    reason: Option<&str>,
) -> Result<(Map<String, Value>, bool), E> {
    let (mut p, odd) = crate::commands::sleep::product_keys(value, boot_id).map_err(raised)?;
    let boot = boot_id.map_or(Value::Null, |b| json!(b));
    let reason_value = reason.map_or(Value::Null, |r| json!(r));
    if stage == "requested" || stage == "quiescing_radios" || stage.starts_with("refused_") {
        let same = if boot_id.is_some_and(|b| !b.is_empty()) {
            json!(true)
        } else {
            Value::Null
        };
        set(
            &mut p,
            &[
                ("state", json!("requested")),
                ("reason", Value::Null),
                ("request_boot_id", boot),
                ("same_boot", same),
                ("kernel_completed", json!(false)),
                ("wake_reason", json!("unknown")),
            ],
        );
    }
    if matches!(stage, "reborn_prepared" | "quiescing_radios" | "syncing") {
        set(&mut p, &[("state", json!("preparing"))]);
    }
    if let Some(rest) = stage.strip_prefix("refused_") {
        let reason = either(reason, json!(rest));
        set(&mut p, &[("state", json!("refused")), ("reason", reason)]);
    } else if stage == "kernel_suspend" {
        // Durable intent immediately before the blocking kernel write. Actual SPM entry is proven
        // only by the kernel journal after wake/reset.
        set(
            &mut p,
            &[
                ("state", json!("sleeping")),
                ("reason", Value::Null),
                ("kernel_completed", json!(false)),
            ],
        );
    } else if stage == "kernel_resumed" || stage == "restoring_radios" {
        let text = match value.get("persistent_kernel") {
            Some(Value::String(s)) => s.clone(),
            Some(other) if truthy(other) => {
                return Err(E::TypeError("expected string or bytes-like object".into()))
            }
            _ => String::new(),
        };
        let kernel = fields(Some(&text));
        let k = |key: &str| kernel.get(key).and_then(Value::as_str);
        let completed = k("valid") == Some("1")
            && k("stage") == Some("EXIT")
            && k("error") == Some("0")
            && is_true(p.get("same_boot"))
            && pm_test_is(value.get("pm_test"), "none")
            && k("reset_resume_entry") == Some("0x59325253")
            && k("pcm_length") == Some("596")
            && !matches!(k("wake"), None | Some("0") | Some("0x0"));
        p.insert("kernel_completed".into(), json!(completed));
        let (before, after) = (value.get("wake_irqs_before"), value.get("wake_irqs_after"));
        if completed {
            let mut sources = Vec::new();
            for name in ["rtc", "power"] {
                if contains(before, name)?
                    && contains(after, name)?
                    && py_gt(index(after, name)?, index(before, name)?)?
                {
                    sources.push(name);
                }
            }
            let wake = match sources.as_slice() {
                [one] => *one,
                [] => "unknown",
                _ => "multiple",
            };
            p.insert("wake_reason".into(), json!(wake));
        }
        if state(&p) != "refused" {
            set(
                &mut p,
                &[("state", json!("waking")), ("reason", reason_value)],
            );
        }
        if k("failed_stage") == Some("DPM_PREPARED") && k("error") == Some("-16") {
            set(
                &mut p,
                &[
                    ("state", json!("refused")),
                    ("reason", json!("device_prepare_refused")),
                ],
            );
        }
    } else if stage == "product_refused" {
        set(
            &mut p,
            &[("state", json!("refused")), ("reason", reason_value)],
        );
    } else if stage == "product_failure" {
        set(
            &mut p,
            &[("state", json!("restore_failed")), ("reason", reason_value)],
        );
    } else if stage.starts_with("complete_result_") {
        let error = value.get("error").cloned().unwrap_or(Value::Null);
        if reason.is_some_and(|r| !r.is_empty()) || truthy(result) || truthy(&error) {
            if state(&p) != "refused" {
                let why = either(
                    reason,
                    if truthy(&error) {
                        error
                    } else {
                        json!("platform_restoration_failed")
                    },
                );
                set(
                    &mut p,
                    &[("state", json!("restore_failed")), ("reason", why)],
                );
            }
        } else if !pm_test_none(value.get("pm_test"))? {
            set(
                &mut p,
                &[
                    ("state", json!("idle")),
                    ("reason", json!("pm_test_completed")),
                    ("kernel_completed", json!(false)),
                ],
            );
        } else if !is_true(p.get("same_boot")) {
            set(
                &mut p,
                &[
                    ("state", json!("restore_failed")),
                    ("reason", json!("same_boot_not_proven")),
                ],
            );
        } else if !p.get("kernel_completed").is_some_and(truthy) {
            set(
                &mut p,
                &[
                    ("state", json!("restore_failed")),
                    ("reason", json!("kernel_exit_not_proven")),
                ],
            );
        } else if !value.get("reborn_restore").is_some_and(truthy) {
            set(
                &mut p,
                &[
                    ("state", json!("restore_failed")),
                    ("reason", json!("reborn_resume_probe_failed")),
                ],
            );
        } else {
            set(
                &mut p,
                &[("state", json!("restored")), ("reason", Value::Null)],
            );
        }
    }
    Ok((p, odd))
}

/// `value.get('pm_test') == text`.
fn pm_test_is(value: Option<&Value>, text: &str) -> bool {
    value.is_some_and(|v| v.py_eq(&json!(text)))
}

/// `value.get('pm_test') in {None, 'none'}` (a list or dict is unhashable).
fn pm_test_none(value: Option<&Value>) -> Result<bool, E> {
    match value {
        None | Some(Value::Null) => Ok(true),
        Some(Value::Array(_) | Value::Object(_)) => Err(E::TypeError("unhashable type".into())),
        Some(other) => Ok(is_text(Some(other), "none")),
    }
}
