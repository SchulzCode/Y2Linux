//! The durable Wi-Fi journal (F52/F53): one compact row per self-heal try, recovery or interface
//! renewal, written only where /data is the platform's own verified mount.
// SPDX-License-Identifier: GPL-2.0-only
use serde::Serialize;
use y2p_core::clock::wall;
use y2p_core::{
    json::{dumps, Separators},
    mounts::data_ready,
    power_journal::append_bounded,
    Root,
};

/// Durable Wi-Fi self-heal rows (F52/F53); never a network name or address.
pub const JOURNAL: &str = "/data/system/platform/wifi-journal.jsonl";
pub const JOURNAL_LIMIT: u64 = 262_144;
/// One decimal, rounded from the exact binary value (`round(x, 1)`), so the rows' ages and dwell times
/// read as the report analyses have always parsed them.
pub fn round1(x: f64) -> f64 {
    format!("{x:.1}").parse().unwrap_or(x)
}

/// Append one durable row (bounded, rotated once); false when /data
/// is not ours or the row could not be written.
pub fn journal<T: Serialize>(root: &Root, event: &str, fields: &T) -> bool {
    if !data_ready(root) {
        return false;
    }
    let row = Row {
        schema: 1,
        event,
        wall_time: wall(),
        monotonic_s: root.clock().monotonic(),
        boot_id: root.boot_id(),
        fields,
    };
    let Ok(text) = dumps(&row, Separators::Compact) else {
        return false;
    };
    append_bounded(
        root,
        JOURNAL,
        format!("{text}\n").as_bytes(),
        JOURNAL_LIMIT,
        true,
    )
    .is_ok()
}

/// One Wi-Fi journal row: these fields first, then the event's own, in their order (the rows are
/// read as written, never sorted).
#[derive(Serialize)]
struct Row<'a, T> {
    schema: u8,
    event: &'a str,
    wall_time: f64,
    monotonic_s: f64,
    boot_id: Option<String>,
    #[serde(flatten)]
    fields: &'a T,
}
