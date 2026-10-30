//! `sleep.status` as a journal row holds it: the suspend receipt's product, read without promoting a
//! reset into a resume.
// SPDX-License-Identifier: GPL-2.0-only
use super::Dropped;
use crate::commands::suspend::receipt::{Product, Receipt};
use y2p_core::Root;

/// The receipt and `sleep status`'s product of it; a receipt that does not read drops the row
/// (counted; the daemon goes on).
pub(super) fn sleep_status(
    root: &Root,
    boot_id: Option<&str>,
) -> Result<(Receipt, Product), Dropped> {
    let receipt = Receipt::read(root).map_err(|_| Dropped)?;
    let product = crate::commands::sleep::product(&receipt, boot_id);
    Ok((receipt, product))
}
