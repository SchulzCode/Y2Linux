//! Group `reports` of the port (docs/planning/PLATFORM-RUST-PORT-PLAN.md, "Remaining port groups").
// SPDX-License-Identifier: GPL-2.0-only
use super::Command;

/// The Rust implementation of these exact arguments, if this group has one.
pub fn route(_args: &[&str]) -> Option<Command> {
    None
}
