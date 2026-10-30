//! `update auto-check`: the service's daily look for a release (never a download).
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    atomic_json, channel, download_record, journal_state, json, private_directory, shortened,
    stage, truthy, Io, Raise, Result, Root, Value, CHECK_INTERVAL, IN_PROGRESS, LAST_CHECK,
};
use crate::commands::connectivity::published::Published;
use serde::{Deserialize, Serialize};

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

/// last-check.json: when a check last reached the server (`time`, kept from the last successful
/// check) and when one was last tried (`attempt`), with the last check's result. Each field reads
/// leniently.
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub(crate) struct LastCheck {
    #[serde(deserialize_with = "y2p_core::lenient::or_none")]
    pub(crate) schema: Option<i64>,
    #[serde(
        deserialize_with = "y2p_core::lenient::or_none",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) attempt: Option<f64>,
    #[serde(
        deserialize_with = "y2p_core::lenient::or_none",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) time: Option<f64>,
    #[serde(
        deserialize_with = "y2p_core::lenient::text",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) result: Option<String>,
    /// The release the check found, as the signed manifest names it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) release_version: Option<Value>,
    #[serde(
        deserialize_with = "y2p_core::lenient::text",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) failure: Option<String>,
}

impl LastCheck {
    /// The record as written (none when missing or of another shape).
    pub(crate) fn load(root: &Root) -> Option<LastCheck> {
        serde_json::from_str(&root.read(LAST_CHECK)?).ok()
    }

    /// The record as the daily check reads it: a `time` that no successful check wrote does not
    /// count (≤ 8.14 also stamped a failed one, which held off every automatic check for a day,
    /// report ef22a23d).
    pub(crate) fn read(root: &Root) -> LastCheck {
        let mut record = LastCheck::load(root).unwrap_or_default();
        if record.result.as_deref() != Some("checked") {
            record.time = None;
        }
        record
    }

    /// When a check last reached the server (seconds since the epoch).
    pub(crate) fn checked(&self) -> Option<f64> {
        self.time
    }

    /// When a check was last tried.
    pub(crate) fn attempted(&self) -> Option<f64> {
        self.attempt
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
