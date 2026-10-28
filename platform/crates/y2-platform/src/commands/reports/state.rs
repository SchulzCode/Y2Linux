//! report.py's state and queue: `reports.json` (opt-in, install ID, limits, seen markers, upload
//! retries, the last report), `status`, `set_enabled`, `new_install_id`, `destinations`.
// SPDX-License-Identifier: GPL-2.0-only
use super::{json_at, media, pyjson::Py};
use crate::reply::{mkdir_parents, Failure};
use std::path::PathBuf;
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

/// `state(ctx)`: reports.json with its defaults.
pub fn state(root: &Root) -> Py {
    let mut value = match json_at(root, STATE) {
        Some(v @ Py::Dict(_)) => v,
        _ => Py::dict(),
    };
    for (key, default) in [
        ("schema", Py::int(1)),
        ("enabled", Py::None), // None: not asked yet
        ("install_id", Py::None),
        ("daily", Py::dict()),
        ("boot_triggers", Py::dict()),
        ("seen", Py::dict()),
    ] {
        if value.get(key).is_none() {
            value.set(key, default);
        }
    }
    if !value.get("uploads").is_some_and(Py::is_dict) {
        value.set("uploads", Py::dict()); // upload retries: queue file name -> attempts, next, error
    }
    value
}

/// Shapes of reports.json and session.json Python's report code crashes on (TypeError, AttributeError):
/// those runs stay Python's.
pub fn prechecks(root: &Root, action: &str) -> Result<(), Failure> {
    if action == "status" || action == "new-id" || action == "disable" {
        return Ok(());
    }
    let value = state(root);
    for key in ["daily", "boot_triggers", "seen"] {
        if !value.get(key).is_some_and(Py::is_dict) {
            return Err(Failure::Fallback);
        }
    }
    let boot = super::boot_id(root).unwrap_or_default();
    if let Some(triggers) = value.get("boot_triggers").and_then(|t| t.get(&boot)) {
        if !matches!(triggers, Py::List(items) if items.iter().all(|t| matches!(t, Py::Str(_)))) {
            return Err(Failure::Fallback);
        }
    }
    if let Some(Py::Dict(daily)) = value.get("daily") {
        if daily.iter().any(|(_, n)| n.number().is_none()) {
            return Err(Failure::Fallback);
        }
    }
    if value
        .get("last")
        .is_some_and(|l| l.truthy() && !l.is_dict())
    {
        return Err(Failure::Fallback);
    }
    if let Some(Py::Dict(uploads)) = value.get("uploads") {
        let bad = |entry: &Py, key| entry.get(key).is_some_and(|v| v.number().is_none());
        if uploads
            .iter()
            .any(|(_, e)| e.is_dict() && (bad(e, "next") || bad(e, "attempts")))
        {
            return Err(Failure::Fallback);
        }
    }
    if !matches!(action, "upload") {
        let session = super::build::large_json(root, "/data/reborn/state/session.json");
        if let Some(model @ Py::Dict(_)) = session.as_ref().and_then(|s| s.get("model")) {
            let queue = model.get("queue");
            if queue.is_some_and(|q| q.truthy() && !matches!(q, Py::List(_) | Py::Str(_))) {
                return Err(Failure::Fallback);
            }
            let settings = model.get("settings");
            if settings.is_some_and(|s| s.truthy() && !s.is_dict()) {
                return Err(Failure::Fallback);
            }
        }
    }
    Ok(())
}

/// `save(ctx, value)`.
pub fn save(root: &Root, value: &Py) -> Result<(), Failure> {
    let path = root.path(STATE);
    mkdir_parents(path.parent().expect("parent"), 0o700)?;
    crate::reply::atomic_json(&path, value, true)
}

/// `ctx.glob(QUEUE + '/*.tar.gz')`.
pub fn queued(root: &Root) -> Vec<PathBuf> {
    root.glob(&format!("{QUEUE}/*.tar.gz"))
}

pub fn name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `upload_key(ctx)`: the build-wide upload key, or None when this build has none. It is public (it
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

/// `set_enabled(ctx, enabled)`.
pub fn set_enabled(root: &Root, enabled: bool) -> Result<Py, Failure> {
    let mut value = state(root);
    let was = value.get("enabled").cloned();
    value.set("enabled", Py::Bool(enabled));
    if enabled && !value.get("install_id").is_some_and(Py::truthy) {
        value.set("install_id", Py::str(uuid4()?));
    }
    if enabled && !matches!(was, Some(Py::Bool(true))) {
        // Only what happens after the user said yes is reported: record what is already there (an
        // old crash record, a finished rollback) as seen.
        save(root, &value)?;
        super::checks::check(root)?;
        value = state(root);
    }
    if !enabled {
        for item in queued(root) {
            crate::reply::unlink_missing_ok(&item)?;
        }
    }
    save(root, &value)?;
    status(root)
}

/// `new_install_id(ctx)`.
pub fn new_install_id(root: &Root) -> Result<Py, Failure> {
    let mut value = state(root);
    value.set("install_id", Py::str(uuid4()?));
    save(root, &value)?;
    status(root)
}

pub fn uuid4() -> Result<String, Failure> {
    y2p_core::py::uuid4().map_err(super::os_plain)
}

/// `status(ctx)`.
pub fn status(root: &Root) -> Result<Py, Failure> {
    let value = state(root);
    let queued: Vec<Py> = queued(root).iter().map(|p| Py::Str(name(p))).collect();
    Ok(Py::dict()
        .with("schema", Py::int(1))
        .with("enabled", value.get("enabled").cloned().unwrap_or(Py::None))
        .with(
            "install_id",
            value.get("install_id").cloned().unwrap_or(Py::None),
        )
        .with("queued", Py::List(queued))
        .with("last", value.get("last").cloned().unwrap_or(Py::None))
        .with("upload", Py::Bool(upload_key(root).is_some()))
        .with(
            "destinations",
            Py::List(destinations(root).into_iter().map(Py::from).collect()),
        ))
}

/// `destinations(ctx)`: the music storage mounted writable now, in DESTINATIONS order.
pub fn destinations(root: &Root) -> Vec<&'static str> {
    let table = media::mountinfo(
        root.read_limited("/proc/self/mountinfo", 1 << 20)
            .as_deref(),
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
