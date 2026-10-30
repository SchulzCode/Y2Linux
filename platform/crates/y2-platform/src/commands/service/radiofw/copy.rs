//! The owner's system.img copy: where it may be (the Firmware folders), when it is complete, and the
//! scan that takes it, or deletes a later one once the firmware is in place.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    from_system_image, fs, install, installed, media, os_path, present, record, start, Path,
    PathBuf, Result, Reviewed, Root, SystemTime, Value, ENOSPC, FOLDER, INBOX, INTERNAL_INBOX,
    README, SD_INBOX, SOURCE, STATE, UNIX_EPOCH,
};
use serde::{Deserialize, Serialize};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::fs::MetadataExt;

/// The MTP responder's records read here: the roots it exports and its last copy error.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Responder {
    #[serde(deserialize_with = "y2p_core::lenient::some_texts")]
    roots: Option<Vec<String>>,
    #[serde(deserialize_with = "y2p_core::lenient::or_none")]
    error: Option<i64>,
}

fn responder(root: &Root, name: &str) -> Responder {
    serde_json::from_value(Value::Object(root.json_object(name))).unwrap_or_default()
}

/// The MTP roots the responder exports (its record), as device paths below the root.
pub(super) fn roots(root: &Root) -> Vec<PathBuf> {
    responder(root, "/run/y2/mtp-roots.json")
        .roots
        .unwrap_or_default()
        .iter()
        .filter(|r| r.starts_with('/'))
        .map(|r| root.path(r))
        .collect()
}

/// A copy's `[st_size, st_mtime_ns]`: one that changed is still arriving or another copy.
pub type Seen = (u64, i64);

/// `/run/y2/radio-firmware.json`: where the owner flow stands (Missing, Installing, Failed, Ready),
/// the copy it last saw, and copies it kept or is judging after the install. A record of another
/// shape reads as none.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Record {
    pub schema: i64,
    pub state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seen: Option<Seen>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kept: Option<Seen>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leftover: Option<Seen>,
}

impl Record {
    fn new(state: &str) -> Record {
        Record {
            schema: 1,
            state: Some(state.into()),
            ..Record::default()
        }
    }

    fn read(root: &Root) -> Record {
        root.read(STATE)
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn failed(&self) -> bool {
        self.state.as_deref() == Some("Failed")
    }
}

/// What `firmware status|scan|install` answers: the state, why, and for a missing copy where it goes.
#[derive(Serialize, Debug, PartialEq)]
pub struct Status {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<&'static str>,
}

impl Status {
    pub fn of(state: &str, reason: Option<String>) -> Status {
        Status {
            state: state.into(),
            reason,
            folder: None,
            file: None,
            location: None,
        }
    }
}

/// True only while the recorded mount is still the live, exact Y2Media mount.
pub fn internal(root: &Root) -> bool {
    crate::commands::storage::internal::ready(root)
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
    if answer
        .get("holders")
        .and_then(Value::as_object)
        .is_some_and(|holders| !holders.is_empty())
    {
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

fn record_state(root: &Root, record: &Record) -> Result<()> {
    let path = root.path(STATE);
    record::atomic_json(&path, record, false, 0o644).map_err(|e| os_path(e, &path))
}

/// `firmware status`: Ready, the scan's Installing or Failed, or Missing with where the copy goes
/// (and `no_space` when the computer's copy did not fit, #39).
pub fn state(root: &Root) -> Status {
    if present(root) {
        return Status::of("Ready", None);
    }
    let record = Record::read(root);
    if let Some(state @ ("Installing" | "Failed")) = record.state.as_deref() {
        return Status::of(state, record.reason);
    }
    // The responder refuses a copy that does not fit before writing anything (mtp-safe space()), so
    // nothing is left behind; only say why (#39).
    let full = responder(root, "/run/y2/mtp-events.json").error == Some(ENOSPC);
    Status {
        state: "Missing".into(),
        reason: full.then(|| "no_space".into()),
        folder: Some(FOLDER),
        file: Some(SOURCE),
        location: Some(if internal(root) { "internal" } else { "drive" }),
    }
}

/// `[st_size, st_mtime_ns]` and whether the copy is still arriving (changed within 5 s).
pub(super) fn seen(meta: &fs::Metadata) -> (Seen, bool) {
    let mtime_ns = meta.mtime() as i128 * 1_000_000_000 + meta.mtime_nsec() as i128;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    let mtime = meta.mtime() as f64 + meta.mtime_nsec() as f64 / 1e9;
    ((meta.len(), mtime_ns as i64), now - mtime < 5.0)
}

fn regular(path: &Path) -> Option<fs::Metadata> {
    path.symlink_metadata()
        .ok()
        .filter(|m| m.file_type().is_file())
}

/// One pass of the owner flow: make the Firmware folder visible, then take a finished
/// system.img copy from it. Returns the new state.
pub(super) fn scan(root: &Root, reviewed: &Reviewed) -> Result<Status> {
    if installed(root, reviewed) {
        return leftover(root, reviewed);
    }
    let previous = Record::read(root);
    let _ = inbox(root);
    for source in candidates(root) {
        let Some(info) = regular(&source) else {
            continue;
        };
        let (seen, arriving) = seen(&info);
        // A copy still arriving over USB changes size or time between passes.
        if previous.seen != Some(seen) || arriving {
            record_state(
                root,
                &Record {
                    seen: Some(seen),
                    ..Record::new("Missing")
                },
            )?;
            return Ok(Status::of("Missing", None));
        }
        if previous.failed() {
            return Ok(Status::of("Failed", previous.reason));
        }
        record_state(
            root,
            &Record {
                seen: Some(seen),
                ..Record::new("Installing")
            },
        )?;
        if let Err(error) =
            from_system_image(reviewed, &source).and_then(|files| install(root, reviewed, files))
        {
            let reason: String = error.text().chars().take(80).collect();
            record_state(
                root,
                &Record {
                    reason: Some(reason.clone()),
                    seen: Some(seen),
                    ..Record::new("Failed")
                },
            )?;
            return Ok(Status::of("Failed", Some(reason)));
        }
        remove(root, &source);
        tidy(root);
        record_state(
            root,
            &Record {
                installed_from: Some("system.img".into()),
                ..Record::new("Ready")
            },
        )?;
        start(root);
        return Ok(Status::of("Ready", None));
    }
    if previous.failed() {
        // The rejected copy is gone: back to asking for it.
        record_state(root, &Record::new("Missing"))?;
    }
    Ok(Status::of("Missing", None))
}

/// With the firmware in place, a finished system.img copy that arrives later
/// (copied again, or after the recovery kept the files) holds 520–860 MB of the owner's storage for
/// nothing: delete it like the first one. Only the firmware package's image goes; any other file of
/// that name stays.
pub(super) fn leftover(root: &Root, reviewed: &Reviewed) -> Result<Status> {
    let previous = Record::read(root);
    let mut value = Record {
        kept: previous.kept,
        ..Record::new("Ready")
    };
    for source in candidates(root) {
        let Some(info) = regular(&source) else {
            continue;
        };
        let (seen, arriving) = seen(&info);
        if Some(seen) == previous.kept {
            continue;
        }
        // A copy still arriving changes size or time between passes.
        if previous.leftover != Some(seen) || arriving {
            value.leftover = Some(seen);
            break;
        }
        if from_system_image(reviewed, &source).is_err() {
            value.kept = Some(seen);
            break;
        }
        remove(root, &source);
        tidy(root);
    }
    if value != previous {
        record_state(root, &value)?;
    }
    Ok(Status::of("Ready", None))
}

/// Cheap check for the service: a copy leftover() has not judged yet.
pub fn leftover_waiting(root: &Root) -> bool {
    let kept = Record::read(root).kept;
    candidates(root)
        .iter()
        .any(|source| regular(source).is_some_and(|info| Some(seen(&info).0) != kept))
}
