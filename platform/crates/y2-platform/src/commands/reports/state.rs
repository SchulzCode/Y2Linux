//! The error-report state and queue: `reports.json` (opt-in, install ID, limits, seen markers,
//! upload retries, the last report), `status`, `set_enabled`, `new_install_id`, `destinations`.
// SPDX-License-Identifier: GPL-2.0-only
use super::{media, Value};
use crate::commands::reports::{object, ValueExt};
use crate::reply::{mkdir_parents, Failure};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map};
use std::collections::BTreeMap;
use std::path::PathBuf;
use y2p_core::lenient;
pub use y2p_core::paths::name;
use y2p_core::Root;

pub const STATE: &str = "/data/system/platform/reports.json";
pub const QUEUE: &str = "/data/system/platform/reports";
pub const UPLOAD_KEY: &str = "/usr/share/y2linux/report-upload.json";
/// Storage a report can be saved to: its mount and the folder MTP shows as the storage's top level
/// (Internal Storage is the music folder, /run/y2/umtprd.conf).
pub const DESTINATIONS: [(&str, &str, &str); 2] = [
    ("internal", "/media/internal", "music"),
    ("sd", "/media/sd", ""),
];

/// reports.json: the opt-in, the install ID, the automatic reports per day and per boot, the seen
/// markers, upload retries and the last report. Each field reads leniently (a value of another
/// shape reads as its default, an entry of another shape is left out), so an odd field never loses
/// the opt-in; keys this build does not know are kept as read and written back.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Reports {
    #[serde(default = "schema")]
    pub schema: i64,
    /// None: the owner was not asked yet.
    #[serde(default, deserialize_with = "lenient::or_none")]
    pub enabled: Option<bool>,
    #[serde(default, deserialize_with = "lenient::text")]
    pub install_id: Option<String>,
    /// UTC day → automatic reports made that day.
    #[serde(default, deserialize_with = "lenient::entries")]
    pub daily: BTreeMap<String, u64>,
    /// Boot ID → the triggers reported this boot (only this boot's are kept).
    #[serde(default, deserialize_with = "lenient::entries")]
    pub boot_triggers: BTreeMap<String, Vec<String>>,
    /// Trigger → the marker it was last reported with.
    #[serde(default, deserialize_with = "lenient::entries")]
    pub seen: BTreeMap<String, String>,
    /// Queue file name → its retry record.
    #[serde(default, deserialize_with = "lenient::entries")]
    pub uploads: BTreeMap<String, Retry>,
    /// The last report made (Reborn's report screen).
    #[serde(
        default,
        deserialize_with = "lenient::or_none",
        skip_serializing_if = "Option::is_none"
    )]
    pub last: Option<Last>,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

fn schema() -> i64 {
    1
}

impl Default for Reports {
    fn default() -> Reports {
        Reports {
            schema: 1,
            enabled: None,
            install_id: None,
            daily: BTreeMap::new(),
            boot_triggers: BTreeMap::new(),
            seen: BTreeMap::new(),
            uploads: BTreeMap::new(),
            last: None,
            other: Map::new(),
        }
    }
}

/// A queued report's retry record (`uploads[name]`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Retry {
    /// Tries so far.
    pub attempts: u64,
    /// When it may go again (wall clock).
    pub next: f64,
    /// Why the last try failed (`HTTP_503`, an exception class).
    pub error: String,
}

/// The last report made (`last`, shown by Reborn's report screen): queued or saved here, then
/// settled by its upload.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Last {
    pub report_id: Option<String>,
    pub trigger: String,
    /// `queued`, `saved`, then `sent`, `failed` or `dropped`.
    pub state: String,
    /// Where a saved report went: the destination and the file below its top folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    pub created_utc: Option<String>,
    pub bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent_utc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_attempt: Option<f64>,
}

impl Reports {
    /// The record (its defaults where it is missing or no object).
    pub fn load(root: &Root) -> Reports {
        root.read(STATE)
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Write the record (sorted keys, durably).
    pub fn save(&self, root: &Root) -> Result<(), Failure> {
        let path = root.path(STATE);
        if let Some(parent) = path.parent() {
            mkdir_parents(parent, 0o700)?;
        }
        crate::reply::atomic_json(&path, self, true)
    }

    /// Opted in (`enabled` is `true`; `null` until the owner was asked).
    pub fn enabled(&self) -> bool {
        self.enabled == Some(true)
    }

    /// The triggers reported on `boot`.
    pub fn reported(&self, boot: &str) -> &[String] {
        self.boot_triggers.get(boot).map_or(&[], Vec::as_slice)
    }

    /// Forget the automatic reports of other days.
    pub fn keep_day(&mut self, day: &str) {
        self.daily.retain(|d, _| d == day);
    }

    /// The automatic reports of `day` so far.
    pub fn count(&self, day: &str) -> u64 {
        self.daily.get(day).copied().unwrap_or(0)
    }
}

/// reports.json with its defaults (the service reads it too).
pub fn state(root: &Root) -> Reports {
    Reports::load(root)
}

/// Write a whole record (tests write odd ones).
#[cfg(test)]
pub fn save(root: &Root, value: &Value) -> Result<(), Failure> {
    let path = root.path(STATE);
    if let Some(parent) = path.parent() {
        mkdir_parents(parent, 0o700)?;
    }
    crate::reply::atomic_json(&path, value, true)
}

/// The queued reports (`QUEUE/*.tar.gz`).
pub fn queued(root: &Root) -> Vec<PathBuf> {
    root.glob(&format!("{QUEUE}/*.tar.gz"))
}

/// The build-wide upload key, or None when this build has none. It is public (it
/// is in every image) and only keeps random internet traffic away from the receiver; the receiver's
/// limits are the protection.
pub fn upload_key(root: &Root) -> Option<String> {
    let value = super::json_dict(root, UPLOAD_KEY);
    let key = value.get("upload_key")?.as_str()?;
    let valid = (32..=128).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    valid.then(|| key.to_owned())
}

/// `report enable|disable`: opting in marks what is already there as seen, opting out empties the
/// queue.
pub fn set_enabled(root: &Root, enabled: bool) -> Result<Value, Failure> {
    let mut value = Reports::load(root);
    let was = value.enabled();
    value.enabled = Some(enabled);
    if enabled && value.install_id.as_deref().is_none_or(str::is_empty) {
        value.install_id = Some(uuid4()?);
    }
    if enabled && !was {
        // Only what happens after the user said yes is reported: record what is already there (an
        // old crash record, a finished rollback) as seen.
        value.save(root)?;
        super::checks::check(root)?;
        value = Reports::load(root);
    }
    if !enabled {
        for item in queued(root) {
            crate::reply::unlink_missing_ok(&item)?;
        }
    }
    value.save(root)?;
    status(root)
}

/// `report new-id`: the receiver sees this player as a new one from now on.
pub fn new_install_id(root: &Root) -> Result<Value, Failure> {
    let mut value = Reports::load(root);
    value.install_id = Some(uuid4()?);
    value.save(root)?;
    status(root)
}

pub fn uuid4() -> Result<String, Failure> {
    y2p_core::py::uuid4().map_err(Failure::from)
}

/// `report status`: what Reborn's report screen shows.
pub fn status(root: &Root) -> Result<Value, Failure> {
    let value = Reports::load(root);
    let queued: Vec<Value> = queued(root)
        .iter()
        .map(|p| Value::String(name(p)))
        .collect();
    Ok(object()
        .with("schema", Value::from(1))
        .with("enabled", json!(value.enabled))
        .with("install_id", json!(value.install_id))
        .with("queued", Value::Array(queued))
        .with("last", json!(value.last))
        .with("upload", Value::Bool(upload_key(root).is_some()))
        .with(
            "destinations",
            Value::Array(destinations(root).into_iter().map(Value::from).collect()),
        ))
}

/// The music storage mounted writable now (or on a write window), in DESTINATIONS order.
pub fn destinations(root: &Root) -> Vec<&'static str> {
    let table = media::mountinfo(
        &root
            .read_limited("/proc/self/mountinfo", 1 << 20)
            .unwrap_or_default(),
    );
    let mounted = |path: &str| table.iter().rev().find(|m| m.path == path);
    DESTINATIONS
        .iter()
        .filter(|(_, path, top)| {
            mounted(path).is_some_and(|m| {
                m.options.iter().any(|o| o == "rw")
                    || (*path == "/media/sd" && media::can_write(root))
            }) && root.path(path).join(top).is_dir()
        })
        .map(|(name, _, _)| *name)
        .collect()
}
