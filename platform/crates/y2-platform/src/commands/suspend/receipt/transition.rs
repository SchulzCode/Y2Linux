//! `sleep.transition`: the product state a receipt record leaves (requested, preparing, refused,
//! sleeping, waking, restored, restore_failed), proven only by the same boot's kernel journal and
//! Reborn's acknowledgement.
// SPDX-License-Identifier: GPL-2.0-only
use super::{Product, ProductState, Receipt, WakeSource};
use y2p_core::suspend_text::fields;

/// `reason or fallback`.
fn either(reason: Option<&str>, fallback: Option<String>) -> Option<String> {
    match reason {
        Some(r) if !r.is_empty() => Some(r.to_owned()),
        _ => fallback,
    }
}

/// The product state after `stage` (`result` the record's, `boot_id` the boot that wrote it).
pub fn transition(
    value: &Receipt,
    stage: &str,
    result: Option<i64>,
    boot_id: Option<&str>,
    reason: Option<&str>,
) -> Product {
    use ProductState::*;
    let mut p = crate::commands::sleep::product(value, boot_id);
    let reason_value = reason.map(str::to_owned);
    if stage == "requested" || stage == "quiescing_radios" || stage.starts_with("refused_") {
        p.set(Requested, None);
        p.request_boot_id = boot_id.map(str::to_owned);
        p.same_boot = boot_id.is_some_and(|b| !b.is_empty()).then_some(true);
        p.kernel_completed = false;
        p.wake_reason = Some(WakeSource::Unknown);
    }
    if matches!(stage, "reborn_prepared" | "quiescing_radios" | "syncing") {
        p.state = Some(Preparing);
    }
    if let Some(rest) = stage.strip_prefix("refused_") {
        p.set(Refused, either(reason, Some(rest.to_owned())));
    } else if stage == "kernel_suspend" {
        // Durable intent immediately before the blocking kernel write. Actual SPM entry is proven
        // only by the kernel journal after wake/reset.
        p.set(Sleeping, None);
        p.kernel_completed = false;
    } else if stage == "kernel_resumed" || stage == "restoring_radios" {
        let kernel = fields(value.persistent_kernel.as_deref());
        let k = |key: &str| kernel.get(key).and_then(serde_json::Value::as_str);
        let completed = k("valid") == Some("1")
            && k("stage") == Some("EXIT")
            && k("error") == Some("0")
            && p.same_boot == Some(true)
            && matches!(&value.pm_test, Some(Some(mode)) if mode == "none")
            && k("reset_resume_entry") == Some("0x59325253")
            && k("pcm_length") == Some("596")
            && !matches!(k("wake"), None | Some("0") | Some("0x0"));
        p.kernel_completed = completed;
        if completed {
            let (before, after) = (
                value.wake_irqs_before.as_ref(),
                value.wake_irqs_after.as_ref(),
            );
            let sources: Vec<&str> = ["rtc", "power"]
                .into_iter()
                .filter(|name| {
                    match (
                        before.and_then(|b| b.get(*name)),
                        after.and_then(|a| a.get(*name)),
                    ) {
                        (Some(b), Some(a)) => a > b,
                        _ => false,
                    }
                })
                .collect();
            let wake = match sources.as_slice() {
                ["rtc"] => WakeSource::Rtc,
                ["power"] => WakeSource::Power,
                [] => WakeSource::Unknown,
                _ => WakeSource::Multiple,
            };
            p.wake_reason = Some(wake);
        }
        if !p.state_is(Refused) {
            p.set(Waking, reason_value);
        }
        if k("failed_stage") == Some("DPM_PREPARED") && k("error") == Some("-16") {
            p.set(Refused, Some("device_prepare_refused".into()));
        }
    } else if stage == "product_refused" {
        p.set(Refused, reason_value);
    } else if stage == "product_failure" {
        p.set(RestoreFailed, reason_value);
    } else if stage.starts_with("complete_result_") {
        let error = value.error().map(str::to_owned);
        let failed = result.is_some_and(|r| r != 0);
        if reason.is_some_and(|r| !r.is_empty()) || failed || error.is_some() {
            if !p.state_is(Refused) {
                let why = either(
                    reason,
                    Some(error.unwrap_or_else(|| "platform_restoration_failed".into())),
                );
                p.set(RestoreFailed, why);
            }
        } else if !pm_test_none(value) {
            p.set(Idle, Some("pm_test_completed".into()));
            p.kernel_completed = false;
        } else if p.same_boot != Some(true) {
            p.set(RestoreFailed, Some("same_boot_not_proven".into()));
        } else if !p.kernel_completed {
            p.set(RestoreFailed, Some("kernel_exit_not_proven".into()));
        } else if value.reborn_restore != Some(true) {
            p.set(RestoreFailed, Some("reborn_resume_probe_failed".into()));
        } else {
            p.set(Restored, None);
        }
    }
    p
}

/// `value.get('pm_test') in {None, 'none'}`.
fn pm_test_none(value: &Receipt) -> bool {
    match &value.pm_test {
        None | Some(None) => true,
        Some(Some(mode)) => mode == "none",
    }
}
