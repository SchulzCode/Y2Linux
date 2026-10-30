//! The power journal row every suspend receipt writes (`PowerJournal(ctx).observe('Suspend', battery,
//! event='suspend_<stage>')` in `suspend_record.record`): bounded incident history on Y2DATA, compact
//! JSON in insertion order (not sorted), one durable row per stage.
//!
//! A fresh `PowerJournal` changes nothing in `observe_usb` (its first call only takes the UDC
//! signature) and always writes, because an event is given. Every failure is counted and dropped:
//! a journal failure must never delay or disable battery protection.
// SPDX-License-Identifier: GPL-2.0-only
use super::E;
use serde_json::Value;
use y2p_core::Root;

/// A plain value in a row: its objects sorted (a dict that came from a sorted record).
pub fn plain(value: Value) -> Value {
    y2p_core::json::sorted(value)
}

/// One `suspend_<stage>` row. `battery` is `/run/y2/power.json`'s `battery` (or `{}`).
pub fn suspend(root: &Root, battery: &Value, event: &str) -> Result<bool, E> {
    let now = super::monotonic();
    // signature = (state, battery.get('status'), battery.get('charging')): outside the try.
    if !battery.is_object() {
        return Err(E::Invalid("invalid_battery_record".to_owned()));
    }
    match row(root, battery, event, now) {
        Ok(written) => Ok(written),
        Err(error) if error.is_recorded_failure() => Ok(false),
        Err(error) => Err(error),
    }
}

fn row(root: &Root, battery: &Value, event: &str, now: f64) -> Result<bool, E> {
    // Do not create persistent state on an unverified/missing data mount.
    if !y2p_core::mounts::data_ready(root) {
        return Ok(false);
    }
    use crate::commands::power::journal;
    // A row that cannot be made or written is counted and dropped, as an OS error is.
    let written = journal::row(root, event, "Suspend", battery, None, now)
        .and_then(|row| journal::append(root, &row, true));
    written
        .map(|()| true)
        .map_err(|_| E::value("power_journal_row_dropped"))
}
