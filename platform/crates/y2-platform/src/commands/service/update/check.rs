//! `update auto-check`: the service's daily look for a release (never a download).
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    atomic_json, channel, download_record, journal_state, json, private_directory, shortened,
    stage, truthy, Io, Map, Raise, Result, Root, Value, CHECK_INTERVAL, IN_PROGRESS, LAST_CHECK,
};
use crate::commands::connectivity::published::Published;

/// The manifest URL `stage` gets from the channel: none for a missing or empty one, refused for one
/// that is not text (an owner's channel.json naming a number, say).
pub(super) fn channel_url(root: &Root) -> Result<Option<String>> {
    match channel(root).manifest_url {
        Some(Value::String(url)) => Ok(Some(url)),
        Some(value) if truthy(&value) => Err(Raise::value("update_channel_unreadable")),
        _ => Ok(None),
    }
}

/// Daily check on Wi-Fi: only reports that an update exists, never downloads
/// a payload or installs. Called by the maintenance loop; cheap when not due.
pub(super) fn automatic_check(root: &Root, io: &dyn Io) -> Result<Value> {
    let now = io.now();
    let directory = root.path("/data/updates");
    private_directory(&directory)?;
    let last = LastCheck::read(root);
    if within(last.checked(), now, CHECK_INTERVAL) {
        return Ok(skipped("not_due"));
    }
    // Any attempt is recorded, so an unready clock retries hourly, not every tick.
    let mut attempt = last.record.clone();
    attempt.insert("schema".into(), json!(1));
    attempt.insert("attempt".into(), json!(now));
    atomic_json(&directory.join("last-check.json"), &attempt, false)?;
    if !journal_state(root).terminal() {
        return Ok(skipped("update_pending"));
    }
    // `not channel(ctx).get('manifest_url')`: truthiness only; its type matters at the check below.
    if !channel(root).manifest_url.is_some_and(|url| truthy(&url)) {
        return Ok(skipped("no_channel"));
    }
    if !Published::read(root).online() {
        return Ok(skipped("offline"));
    }
    if !io.time_status(root).get("tls_ready").is_some_and(truthy) {
        return Ok(skipped("clock_not_ready"));
    }
    // Only a check that reached the server starts the next day; a failed one is tried again with the
    // hourly attempt (it waited a whole day).
    let mut record = json!({"schema": 1, "attempt": now});
    if let Some(time) = last.record.get("time").filter(|t| t.is_number()) {
        record["time"] = time.clone();
    }
    // Never an interrupted download's leftover: a failed check would write it back.
    let previous = download_record(root);
    match channel_url(root).and_then(|url| stage(root, io, None, url.as_deref(), true)) {
        Ok(manifest) => {
            record["time"] = json!(now);
            record["result"] = json!("checked");
            record["release_version"] = manifest
                .get("release_version")
                .cloned()
                .unwrap_or(Value::Null);
        }
        Err(error) => {
            record["result"] = json!("failed");
            record["failure"] = json!(shortened(&error.text(), 120));
            // A failed background check is not the owner's failed update: keep what the Software
            // Update page showed before. A busy lock wrote nothing, and the owner's check or download
            // that holds it keeps its record.
            let path = root.path("/data/updates/download.json");
            let busy = error.text() == IN_PROGRESS;
            match (&previous, busy) {
                (_, true) => {}
                (None, false) => {
                    let _ = crate::reply::unlink_missing_ok(&path);
                }
                (Some(previous), false) => {
                    let _ = atomic_json(&path, previous, true);
                }
            }
        }
    }
    atomic_json(&directory.join("last-check.json"), &record, true)?;
    Ok(record)
}

/// last-check.json as the daily check reads it: when a check last reached the server (`time`, only
/// with `result` "checked": ≤ 8.14 also stamped a failed one, which held off every automatic check
/// for a day, report ef22a23d) and when one was last tried (`attempt`).
pub(crate) struct LastCheck {
    /// The record without a `time` that no successful check wrote.
    record: Map<String, Value>,
}

impl LastCheck {
    pub(crate) fn read(root: &Root) -> Self {
        let mut record = root.json_object(LAST_CHECK);
        if record.get("result") != Some(&json!("checked")) {
            record.shift_remove("time");
        }
        Self { record }
    }

    /// When a check last reached the server (seconds since the epoch; a number only).
    pub(crate) fn checked(&self) -> Option<f64> {
        self.record.get("time").and_then(Value::as_f64)
    }

    /// When a check was last tried.
    pub(crate) fn attempted(&self) -> Option<f64> {
        self.record.get("attempt").and_then(Value::as_f64)
    }
}

/// Whether `stamp` lies less than `period` seconds before `now` (one in the future does not: a clock
/// set back must not hold checks off).
pub(crate) fn within(stamp: Option<f64>, now: f64, period: f64) -> bool {
    stamp.is_some_and(|stamp| (0.0..period).contains(&(now - stamp)))
}

/// What `update auto-check` prints when it did not check (the service ignores it; `null` before).
pub(super) fn skipped(reason: &str) -> Value {
    json!({"schema": 1, "result": "skipped", "reason": reason})
}
