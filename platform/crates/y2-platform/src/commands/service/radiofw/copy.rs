//! The owner's system.img copy: where it may be (the Firmware folders), when it is complete, and the
//! scan that takes it, or deletes a later one once the firmware is in place.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    from_system_image, fs, install, installed, json, media, mounts_at, os_path, present, py,
    record, start, Map, Object, Path, PathBuf, Result, Reviewed, Root, SystemTime, Value, ENOSPC,
    FOLDER, INBOX, INTERNAL_INBOX, README, SD_INBOX, SOURCE, STATE, UNIX_EPOCH,
};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::fs::MetadataExt;
use y2p_core::json::ObjectExt;
use y2p_core::py::PyValue;

/// The MTP roots the responder exports (its record), as device paths below the root.
pub(super) fn roots(root: &Root) -> Vec<PathBuf> {
    let value = root.json_object("/run/y2/mtp-roots.json");
    // `for r in value.get('roots', [])`: a list's items, a dict's keys, a string's characters.
    let items = value.get("roots").and_then(py::iterate).unwrap_or_default();
    items
        .iter()
        .filter_map(Value::as_str)
        .filter(|r| r.starts_with('/'))
        .map(|r| root.path(r))
        .collect()
}

fn get_or_null<'a>(map: &'a Map<String, Value>, key: &str) -> &'a Value {
    map.get(key).unwrap_or(&Value::Null)
}

/// True only while the recorded mount is still the live, exact Y2Media mount.
pub fn internal(root: &Root) -> bool {
    let claim = root.json_object("/run/y2/internal-media.json");
    let current = mounts_at(root, "/media/internal");
    claim.get("state") == Some(&json!("Ready"))
        && current.len() == 1
        && *get_or_null(&claim, "boot_id") == Value::from(root.boot_id())
        && py::eq_int(claim.get("mount_id"), current[0].mount_id)
        && claim.get("device_id") == Some(&json!(current[0].device_id))
}

/// `Path.mkdir(mode, exist_ok=True)`.
fn mkdir_exist_ok(path: &Path, mode: u32) -> Result<()> {
    match fs::DirBuilder::new().mode(mode).create(path) {
        Err(e) if !(e.kind() == std::io::ErrorKind::AlreadyExists && path.is_dir()) => {
            Err(os_path(e, path))
        }
        _ => Ok(()),
    }
}

/// Where the copy goes, created with its note when needed: the Firmware folder on
/// Internal Storage, else the Firmware drive's folder on Y2DATA.
pub(crate) fn inbox(root: &Root) -> Result<PathBuf> {
    let folder = if internal(root) {
        let folder = root.path(INTERNAL_INBOX);
        mkdir_exist_ok(&folder, 0o755)?;
        folder
    } else {
        let folder = root.path(INBOX);
        if let Some(parent) = folder.parent() {
            crate::reply::mkdir_parents(parent, 0o700)?;
        }
        mkdir_exist_ok(&folder, 0o700)?;
        folder
    };
    let note = folder.join("README.txt");
    if !note.exists() {
        fs::write(&note, README).map_err(|e| os_path(e, &note))?;
    }
    Ok(folder)
}

/// Where an owner's copy may be: the Firmware drive, or a Firmware folder on
/// Internal Storage or the SD card (also while no computer is connected).
pub fn candidates(root: &Root) -> Vec<PathBuf> {
    let mut places = vec![root.path(INBOX), root.path(SD_INBOX)];
    if internal(root) {
        places.insert(0, root.path(INTERNAL_INBOX));
    }
    let inbox = root.path(INBOX);
    places.extend(
        roots(root)
            .into_iter()
            .filter(|r| *r != inbox)
            .map(|r| r.join(FOLDER)),
    );
    let mut seen: Vec<PathBuf> = Vec::new();
    for folder in places {
        if !seen.contains(&folder) {
            seen.push(folder);
        }
    }
    seen.into_iter().map(|f| f.join(SOURCE)).collect()
}

/// Delete the used system.img; on the SD card inside a write window of its own (the
/// card is read-only between writes, storage::media).
fn remove(root: &Root, source: &Path) {
    let on_sd = source
        .to_string_lossy()
        .starts_with(&*root.path(SD_INBOX).to_string_lossy());
    if !on_sd {
        let _ = fs::remove_file(source);
        return;
    }
    let Ok(answer) = media::write_window(root, "radiofw", 30, false) else {
        return;
    };
    let _ = fs::remove_file(source);
    if answer.get("holders").is_some_and(py::truthy) {
        let _ = media::write_window(root, "radiofw", 30, true);
    }
}

/// After the install the Internal Storage Firmware folder goes away again (only its
/// note; anything the owner put there stays).
fn tidy(root: &Root) {
    let folder = root.path(INTERNAL_INBOX);
    if fs::remove_file(folder.join("README.txt")).is_ok() {
        let _ = fs::remove_dir(&folder);
    }
}

fn record_state(root: &Root, value: &Value) -> Result<()> {
    let path = root.path(STATE);
    record::atomic_json(&path, value, false, 0o644).map_err(|e| os_path(e, &path))
}

/// `firmware status`: Ready, the scan's Installing or Failed, or Missing with where the copy goes
/// (and `no_space` when the computer's copy did not fit, #39).
pub(super) fn state(root: &Root) -> Value {
    let value = root.json_object(STATE);
    if present(root) {
        return json!({"state": "Ready"});
    }
    if matches!(
        value.get("state").and_then(Value::as_str),
        Some("Installing" | "Failed")
    ) {
        return json!({"state": get_or_null(&value, "state"), "reason": get_or_null(&value, "reason")});
    }
    let mut missing = json!({"state": "Missing", "folder": FOLDER, "file": SOURCE,
                             "location": if internal(root) { "internal" } else { "drive" }});
    // The responder refuses a copy that does not fit before writing anything (mtp-safe space()), so
    // nothing is left behind; only say why (#39).
    if py::eq_int(
        root.json_object("/run/y2/mtp-events.json").get("error"),
        ENOSPC,
    ) {
        missing["reason"] = json!("no_space");
    }
    missing
}

/// `[st_size, st_mtime_ns]` and whether the copy is still arriving (changed within 5 s).
pub(super) fn seen(meta: &fs::Metadata) -> (Value, bool) {
    let mtime_ns = meta.mtime() as i128 * 1_000_000_000 + meta.mtime_nsec() as i128;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    let mtime = meta.mtime() as f64 + meta.mtime_nsec() as f64 / 1e9;
    (json!([meta.len(), mtime_ns as i64]), now - mtime < 5.0)
}

fn regular(path: &Path) -> Option<fs::Metadata> {
    path.symlink_metadata()
        .ok()
        .filter(|m| m.file_type().is_file())
}

/// One pass of the owner flow: make the Firmware folder visible, then take a finished
/// system.img copy from it. Returns the new state.
pub(super) fn scan(root: &Root, reviewed: &Reviewed) -> Result<Value> {
    if installed(root, reviewed) {
        return leftover(root, reviewed);
    }
    let previous = root.json_object(STATE);
    let _ = inbox(root);
    for source in candidates(root) {
        let Some(info) = regular(&source) else {
            continue;
        };
        let (seen, arriving) = seen(&info);
        // A copy still arriving over USB changes size or time between passes.
        if !get_or_null(&previous, "seen").py_eq(&seen) || arriving {
            record_state(
                root,
                &json!({"schema": 1, "state": "Missing", "seen": seen}),
            )?;
            return Ok(json!({"state": "Missing"}));
        }
        if previous.get("state") == Some(&json!("Failed")) {
            return Ok(json!({"state": "Failed", "reason": get_or_null(&previous, "reason")}));
        }
        record_state(
            root,
            &json!({"schema": 1, "state": "Installing", "seen": seen}),
        )?;
        if let Err(error) =
            from_system_image(reviewed, &source).and_then(|files| install(root, reviewed, files))
        {
            let reason: String = error.text().chars().take(80).collect();
            record_state(
                root,
                &json!({"schema": 1, "state": "Failed", "reason": reason, "seen": seen}),
            )?;
            return Ok(json!({"state": "Failed", "reason": reason}));
        }
        remove(root, &source);
        tidy(root);
        record_state(
            root,
            &json!({"schema": 1, "state": "Ready", "installed_from": "system.img"}),
        )?;
        start(root);
        return Ok(json!({"state": "Ready"}));
    }
    if previous.get("state") == Some(&json!("Failed")) {
        // The rejected copy is gone: back to asking for it.
        record_state(root, &json!({"schema": 1, "state": "Missing"}))?;
    }
    Ok(json!({"state": "Missing"}))
}

/// With the firmware in place, a finished system.img copy that arrives later
/// (copied again, or after the recovery kept the files) holds 520–860 MB of the owner's storage for
/// nothing: delete it like the first one. Only the firmware package's image goes; any other file of
/// that name stays.
pub(super) fn leftover(root: &Root, reviewed: &Reviewed) -> Result<Value> {
    let previous = root.json_object(STATE);
    let mut value = Object::new()
        .with("schema", 1)
        .with("state", "Ready")
        .with("kept", get_or_null(&previous, "kept").clone());
    for source in candidates(root) {
        let Some(info) = regular(&source) else {
            continue;
        };
        let (seen, arriving) = seen(&info);
        if seen.py_eq(get_or_null(&previous, "kept")) {
            continue;
        }
        // A copy still arriving changes size or time between passes.
        if !get_or_null(&previous, "leftover").py_eq(&seen) || arriving {
            value.set("leftover", seen);
            break;
        }
        if from_system_image(reviewed, &source).is_err() {
            value.set("kept", seen);
            break;
        }
        remove(root, &source);
        tidy(root);
    }
    let differs = value
        .iter()
        .any(|(k, v)| !v.py_eq(get_or_null(&previous, k)))
        || previous.keys().any(|k| value.get(k).is_none());
    if differs {
        record_state(root, &Value::Object(value))?;
    }
    Ok(json!({"state": "Ready"}))
}

/// Cheap check for the service: a copy leftover() has not judged yet.
pub fn leftover_waiting(root: &Root) -> bool {
    let kept = root
        .json_object(STATE)
        .get("kept")
        .cloned()
        .unwrap_or(Value::Null);
    candidates(root)
        .iter()
        .any(|source| regular(source).is_some_and(|info| !seen(&info).0.py_eq(&kept)))
}
