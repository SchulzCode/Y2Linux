//! The durable Wi-Fi journal (F52/F53): one compact row per self-heal try, recovery or interface
//! renewal, written only where /data is the platform's own verified mount.
// SPDX-License-Identifier: GPL-2.0-only
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};
use y2p_core::{
    connectivity_journal::append_bounded,
    json::{dumps, Separators},
    mounts::data_ready,
    Root,
};

/// Durable Wi-Fi self-heal rows (F52/F53); never a network name or address.
pub const JOURNAL: &str = "/data/system/platform/wifi-journal.jsonl";
pub const JOURNAL_LIMIT: u64 = 262_144;
fn wall() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// Python's `round(x, 1)`: the decimal rounding of the exact binary value, read back as a float.
pub fn round1(x: f64) -> f64 {
    format!("{x:.1}").parse().unwrap_or(x)
}

/// `journal(ctx, event, **fields)`: append one durable row (bounded, rotated once); false when /data
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
