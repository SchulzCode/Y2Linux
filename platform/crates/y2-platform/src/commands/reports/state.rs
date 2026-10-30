//! The error-report state and queue: `reports.json` (opt-in, install ID, limits, seen markers,
//! upload retries, the last report), `status`, `set_enabled`, `new_install_id`, `destinations`.
// SPDX-License-Identifier: GPL-2.0-only
use super::{json_at, media, pyops::OrRefuse, Value};
use crate::commands::reports::{filled, object, ValueExt};
use crate::reply::{mkdir_parents, Failure};
use std::path::PathBuf;
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
/// markers, upload retries and the last report. It is rewritten whole, so every key and value is
/// kept as read; the fields the code reads have accessors, and a field of another shape is refused
/// where it is used (`report_state_unreadable`), not when the file is read.
#[derive(Clone, Debug)]
pub struct Reports(Value);

impl Reports {
    /// The record with its defaults (anything but an object reads as `{}`).
    pub fn load(root: &Root) -> Reports {
        let mut value = match json_at(root, STATE) {
            Some(v @ Value::Object(_)) => v,
            _ => object(),
        };
        for (key, default) in [
            ("schema", Value::from(1)),
            ("enabled", Value::Null), // None: not asked yet
            ("install_id", Value::Null),
            ("daily", object()),
            ("boot_triggers", object()),
            ("seen", object()),
        ] {
            if value.get(key).is_none() {
                value.set(key, default);
            }
        }
        if !value.get("uploads").is_some_and(Value::is_object) {
            value.set("uploads", object());
        }
        Reports(value)
    }

    /// Write the record (sorted keys, durably).
    pub fn save(&self, root: &Root) -> Result<(), Failure> {
        let path = root.path(STATE);
        if let Some(parent) = path.parent() {
            mkdir_parents(parent, 0o700)?;
        }
        crate::reply::atomic_json(&path, &self.0, true)
    }

    /// Opted in: `enabled` is `true` (`null` until the owner was asked).
    pub fn enabled(&self) -> bool {
        matches!(self.0.get("enabled"), Some(Value::Bool(true)))
    }

    /// A field as read (`null` when absent).
    pub fn field(&self, key: &str) -> Value {
        self.0.get(key).cloned().unwrap_or(Value::Null)
    }

    pub fn set(&mut self, key: &str, value: Value) {
        self.0.set(key, value);
    }

    /// `seen`: the marker of each automatic trigger last reported.
    pub fn seen(&self) -> Result<&Value, Failure> {
        let seen = self.0.get("seen").unwrap_or(&Value::Null);
        if seen.is_object() {
            Ok(seen)
        } else {
            Err(Failure::value(super::STATE_UNREADABLE))
        }
    }

    /// Record `current` as the marker `name` was last reported with.
    pub fn mark_seen(&mut self, name: &str, current: Value) {
        if let Some(seen) = self.0.get_mut("seen") {
            seen.set(name, current);
        }
    }

    /// `boot_triggers.get(boot, [])`: the triggers reported this boot (a list, or what was read).
    pub fn reported(&self, boot: &str) -> Result<Value, Failure> {
        let triggers = self.0.get("boot_triggers").unwrap_or(&Value::Null);
        Ok(super::pyops::get(triggers, boot)
            .or_refuse(super::STATE_UNREADABLE)?
            .cloned()
            .unwrap_or(Value::Array(Vec::new())))
    }

    /// Forget the automatic reports of other days (`daily`: UTC day -> count).
    pub fn keep_day(&mut self, day: &str) -> Result<(), Failure> {
        let Some(Value::Object(items)) = self.0.get("daily") else {
            return Err(Failure::value(super::STATE_UNREADABLE));
        };
        let today = items
            .iter()
            .filter(|(k, _)| k.as_str() == day)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        self.0.set("daily", Value::Object(today));
        Ok(())
    }

    /// The automatic reports of `day` so far (`daily.get(day, 0)`: a number, or what was read).
    pub fn count(&self, day: &str) -> Value {
        self.0
            .get("daily")
            .and_then(|d| d.get(day))
            .cloned()
            .unwrap_or(Value::from(0))
    }

    pub fn set_count(&mut self, day: &str, count: Value) {
        if let Some(daily) = self.0.get_mut("daily") {
            daily.set(day, count);
        }
    }

    /// The upload retries (`uploads`: queue file name -> attempts, next, error).
    pub fn uploads(&self) -> &Value {
        self.0.get("uploads").unwrap_or(&Value::Null)
    }

    /// The whole record (the service reads it too).
    pub fn into_py(self) -> Value {
        self.0
    }
}

/// reports.json with its defaults, as read (the service reads it too).
pub fn state(root: &Root) -> Value {
    Reports::load(root).into_py()
}

/// Write a whole record (tests write odd ones).
#[cfg(test)]
pub fn save(root: &Root, value: &Value) -> Result<(), Failure> {
    Reports(value.clone()).save(root)
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

/// The last report made (`last` in reports.json, shown by Reborn's report screen): queued or saved
/// here, then settled by its upload.
pub struct Last<'a> {
    pub report_id: Value,
    pub trigger: &'a str,
    /// `queued` or `saved`.
    pub state: &'static str,
    /// Where a saved report went: the destination and the file below its top folder.
    pub saved: Option<(&'a str, &'a str)>,
    pub created_utc: Value,
    pub bytes: usize,
}

impl From<Last<'_>> for Value {
    fn from(last: Last) -> Value {
        let mut value = object()
            .with("report_id", last.report_id)
            .with("trigger", Value::from(last.trigger))
            .with("state", Value::from(last.state));
        if let Some((destination, file)) = last.saved {
            value.set("destination", Value::from(destination));
            value.set("file", Value::from(file));
        }
        value
            .with("created_utc", last.created_utc)
            .with("bytes", Value::from(last.bytes))
    }
}

/// `report enable|disable`: opting in marks what is already there as seen, opting out empties the
/// queue.
pub fn set_enabled(root: &Root, enabled: bool) -> Result<Value, Failure> {
    let mut value = Reports::load(root);
    let was = value.enabled();
    value.set("enabled", Value::Bool(enabled));
    if enabled && !filled(&value.field("install_id")) {
        value.set("install_id", Value::from(uuid4()?));
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
    value.set("install_id", Value::from(uuid4()?));
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
        .with("enabled", value.field("enabled"))
        .with("install_id", value.field("install_id"))
        .with("queued", Value::Array(queued))
        .with("last", value.field("last"))
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
