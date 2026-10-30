//! `y2-platform sleep status` (`sleep.status`/`sleep.product`): the product sleep receipt as this
//! boot reads it; a reset is never promoted into a resume.
// SPDX-License-Identifier: GPL-2.0-only
use super::suspend::receipt::{Product, ProductState, Receipt};
use crate::reply::{Outcome, Reply};
use y2p_core::Root;

/// `sleep.product`: the product state as this boot reads it (a receipt without a known product state
/// is idle), with the schema, the boot and whether the request was this boot's.
pub fn product(value: &Receipt, boot_id: Option<&str>) -> Product {
    use ProductState::*;
    let mut result = match &value.product {
        Some(product) if product.state.is_some_and(|s| s != Idle) => product.clone(),
        _ => Product::idle(),
    };
    result.schema = Some(y2p_contract::schema::SLEEP.into());
    result.boot_id = Some(boot_id.map(str::to_owned));
    let same = match (boot_id, result.request_boot_id.as_deref()) {
        (Some(boot), Some(request)) if !boot.is_empty() && !request.is_empty() => {
            Some(request == boot)
        }
        _ => None,
    };
    result.same_boot = same;
    if result
        .state
        .is_some_and(|s| matches!(s, Restored | Refused))
        && same == Some(false)
    {
        result.set(Idle, None);
        result.request_boot_id = None;
        result.same_boot = None;
        result.kernel_completed = false;
        result.wake_reason = Some(super::suspend::receipt::WakeSource::Unknown);
    }
    if result
        .state
        .is_some_and(|s| matches!(s, Requested | Preparing | Sleeping | Restoring | Waking))
        && same == Some(false)
    {
        result.set(
            RestoreFailed,
            Some("boot_changed_before_restoration".into()),
        );
        result.kernel_completed = false;
    }
    result
}

/// A receipt that does not read is the command's refusal record (exit 1).
pub fn status(root: &Root) -> Outcome {
    match Receipt::read(root) {
        Ok(receipt) => Reply::sorted(&product(&receipt, root.boot_id().as_deref()), 0),
        Err(error) => super::suspend::refusal(error.text()),
    }
}
