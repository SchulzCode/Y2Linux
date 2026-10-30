//! `update auto-check`: the service's daily look for a release (never a download).
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    atomic_json, channel, download_record, journal_state, json, private_directory, shortened,
    stage, Io, ManifestUrl, Raise, Result, Root, Value, CHECK_INTERVAL, IN_PROGRESS, LAST_CHECK,
};
use crate::commands::connectivity::published;
use y2p_contract::update::LastCheck;

/// The manifest URL `stage` gets from the channel: none for a missing or empty one, refused for one
/// that is not text (an owner's channel.json naming a number, say).
pub(super) fn channel_url(root: &Root) -> Result<Option<String>> {
    match channel(root).manifest_url {
        ManifestUrl::Text(url) => Ok(Some(url)),
        ManifestUrl::Other(true) => Err(Raise::value("update_channel_unreadable")),
        _ => Ok(None),
    }
}

/// Daily check on Wi-Fi: only reports that an update exists, never downloads
/// a payload or installs. Called by the maintenance loop; cheap when not due.
pub(super) fn automatic_check(root: &Root, io: &dyn Io) -> Result<Value> {
    let now = io.now();
    let directory = root.path("/data/updates");
    private_directory(&directory)?;
    let last = read_last_check(root);
    if within(last.checked(), now, CHECK_INTERVAL) {
        return Ok(skipped("not_due"));
    }
    // Any attempt is recorded, so an unready clock retries hourly, not every tick.
    let attempt = LastCheck {
        schema: Some(1),
        attempt: Some(now),
        ..last.clone()
    };
    atomic_json(&directory.join("last-check.json"), &attempt, false)?;
    if !journal_state(root).terminal() {
        return Ok(skipped("update_pending"));
    }
    // `not channel(ctx).get('manifest_url')`: truthiness only; its type matters at the check below.
    if !channel(root).manifest_url.set() {
        return Ok(skipped("no_channel"));
    }
    if !published::read(root).online() {
        return Ok(skipped("offline"));
    }
    if !io.time_status(root).tls_ready {
        return Ok(skipped("clock_not_ready"));
    }
    // Only a check that reached the server starts the next day; a failed one is tried again with the
    // hourly attempt (it waited a whole day).
    let mut record = LastCheck {
        schema: Some(1),
        attempt: Some(now),
        time: last.time,
        ..LastCheck::default()
    };
    // Never an interrupted download's leftover: a failed check would write it back.
    let previous = download_record(root);
    match channel_url(root).and_then(|url| stage(root, io, None, url.as_deref(), true)) {
        Ok(manifest) => {
            record.time = Some(now);
            record.result = Some("checked".into());
            record.release_version = Some(
                manifest
                    .get("release_version")
                    .cloned()
                    .unwrap_or(Value::Null),
            );
        }
        Err(error) => {
            record.result = Some("failed".into());
            record.failure = Some(shortened(&error.text(), 120));
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
    Ok(json!(record))
}

/// last-check.json as written (none when missing or of another shape).
pub(crate) fn load_last_check(root: &Root) -> Option<LastCheck> {
    serde_json::from_str(&root.read(LAST_CHECK)?).ok()
}

/// last-check.json as the daily check reads it: a `time` that no successful check wrote does not
/// count (≤ 8.14 also stamped a failed one, which held off every automatic check for a day, report
/// ef22a23d).
pub(crate) fn read_last_check(root: &Root) -> LastCheck {
    let mut record = load_last_check(root).unwrap_or_default();
    if record.result.as_deref() != Some("checked") {
        record.time = None;
    }
    record
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
