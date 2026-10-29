//! The lease a long transfer holds (commands::workload).
// SPDX-License-Identifier: GPL-2.0-only
use crate::{commands::workload::Lease, reply::Failure};
use y2p_core::Root;

/// Run `work` as workload `class` under the activity lease and its QoS hint.
pub(super) fn workload<T>(
    root: &Root,
    class: &str,
    work: impl FnOnce() -> Result<T, Failure>,
) -> Result<T, Failure> {
    let _lease = Lease::take(root, class)?;
    work()
}
