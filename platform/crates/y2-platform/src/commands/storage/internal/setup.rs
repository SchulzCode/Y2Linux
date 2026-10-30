//! `internal-media setup` (STORAGE-SETUP): the automatic or confirmed change of the stock FAT into
//! Y2Media, resumed after a power loss from its record on Y2DATA.
// SPDX-License-Identifier: GPL-2.0-only
use super::super::observe::setup_status;
use super::super::records::Properties;
use super::super::sys::{
    exists, get, get_eq, is_symlink, mkdir, mounts as all_mounts, name, release_view, statvfs,
    to_object, Mount, Object,
};
use super::gate::automatic_blocked;
use super::provision::{copy_verified, makedirs, migrate, sha256, sync, tree};
use super::{
    lock_runtime, mkfs, mount_locked, partition, probe, valid, LEGACY_MUSIC, MUSIC, SIZE, START,
};
use crate::reply::At;
use crate::reply::{atomic_json, atomic_json_mode, write_text, Failure};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
use y2p_core::{
    command,
    py::{truthy, PyValue},
    storage_media::fat_benign_only,
    Root,
};

const SETUP_FOLDER: &str = "/data/system"; // Y2DATA: survives formatting p8
pub(super) const SETUP: &str = "/data/system/internal-media-setup.json"; // the resume record
pub(super) const KEPT_STAGING: &str = "/data/system/internal-media-kept"; // kept files while p8 is formatted
pub(super) const OLD: &str = "/run/y2/old-storage"; // the stock FAT, read-only
const STATUS: &str = "/run/y2/internal-media-setup.json";
pub(super) const KEPT: &str = "Old Player Files";
const KEEP_LIMIT: u64 = 400 * 1000 * 1000;
pub(super) const DATA_RESERVE: u64 = 150 * 1000 * 1000;
const SPLASH: &str = "/run/reborn-splash";
// Stock leftovers are never kept; everything else on the old storage is the listener's.
const LEFTOVER_DIRS: [&str; 7] = [
    ".android_secure",
    ".thumbnails",
    "system volume information",
    ".spotlight-v100",
    ".trashes",
    ".fseventsd",
    ".temporaryitems",
];
const LEFTOVER_FILES: [&str; 4] = [".nomedia", ".ds_store", "thumbs.db", "desktop.ini"];
const LEFTOVER_TREES: [&str; 1] = ["android/data/com.android.providers.media/albumthumbs"];

/// A stock leftover (cache, empty marker, computer clutter), never kept.
pub(super) fn leftover(relative: &str) -> bool {
    let path = relative.to_lowercase();
    let parts: Vec<&str> = path.split('/').collect();
    let last = parts.last().copied().unwrap_or_default();
    parts[..parts.len() - 1]
        .iter()
        .any(|p| LEFTOVER_DIRS.contains(p))
        || LEFTOVER_FILES.contains(&last)
        || last.starts_with("._")
        || LEFTOVER_TREES
            .iter()
            .any(|t| path.starts_with(&format!("{t}/")))
}

/// (relative path, size) of the listener's files on the old storage.
fn keepable(top: &Path) -> Result<Vec<(String, u64)>, Failure> {
    let (files, _) = tree(top)?;
    Ok(files
        .into_iter()
        .filter(|(relative, _)| !leftover(relative))
        .collect())
}

/// 1: nothing to keep; 2: few enough to keep on Y2DATA meanwhile; 3: ask the listener.
pub(super) fn decide(kept: &[(String, u64)], data_free: u64) -> u8 {
    let total: u64 = kept.iter().map(|(_, size)| size).sum();
    if kept.is_empty() {
        return 1;
    }
    if total <= KEEP_LIMIT && i128::from(data_free) - i128::from(total) >= i128::from(DATA_RESERVE)
    {
        return 2;
    }
    3
}

/// internal-media-setup.json in /run: this boot's setup state, for Reborn (counts only, never file
/// names).
#[derive(Serialize)]
struct Note {
    schema: u8,
    state: &'static str,
    reason: Option<String>,
    boot_id: Option<String>,
    /// What p8 holds when it is not the stock FAT.
    #[serde(skip_serializing_if = "Option::is_none")]
    filesystem: Option<Value>,
    /// What is kept (Running) or waits for the listener (Waiting).
    #[serde(skip_serializing_if = "Option::is_none")]
    files: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes: Option<Value>,
    /// The old storage's serial the listener confirms with (Waiting).
    #[serde(skip_serializing_if = "Option::is_none")]
    serial: Option<Value>,
    /// The files moved to Old Player Files (Done).
    #[serde(skip_serializing_if = "Option::is_none")]
    kept: Option<Value>,
}

impl Note {
    fn new(root: &Root, state: &'static str, reason: Option<&str>) -> Note {
        Note {
            schema: 1,
            state,
            reason: reason.map(str::to_owned),
            boot_id: root.boot_id(),
            filesystem: None,
            files: None,
            bytes: None,
            serial: None,
            kept: None,
        }
    }
}

/// Publish the note (others read it: 0644); it is also the answer.
fn publish(root: &Root, note: Note) -> Result<Object, Failure> {
    mkdir(&root.path("/run/y2"), 0o700)?;
    atomic_json_mode(&root.path(STATUS), &note, false, 0o644)?;
    Ok(to_object(&note))
}

/// A note with no counts.
fn note(root: &Root, state: &'static str, reason: Option<&str>) -> Result<Object, Failure> {
    publish(root, Note::new(root, state, reason))
}

/// A progress step between `low` and `high`, rounded down to tens.
fn tens(low: u64, high: u64, done: u64, total: u64) -> u64 {
    low + (high - low) * done.min(total) / total.max(1) / 10 * 10
}

/// Progress on the start screen while the setup runs at boot (0-100 in tens).
pub(crate) fn splash(root: &Root, step: u64) {
    let folder = root.path(SPLASH);
    if folder.is_dir() && step <= 100 && step.is_multiple_of(10) {
        let phase = if step == 0 {
            "media_setup".to_owned()
        } else {
            format!("media_setup_{step}")
        };
        let _ = write_text(&folder.join("phase"), &format!("{phase}\n"));
    }
}

/// The stock FAT mounted read-only at OLD after its read-only check (#44/F12 rules): `None`, or
/// why not.
fn mount_old(root: &Root, device: &str) -> Result<Option<&'static str>, Failure> {
    if all_mounts(root)
        .iter()
        .any(|m| m.path == OLD && m.source == device)
    {
        return Ok(None);
    }
    let check = command::run_with(
        root,
        &["fsck.fat", "-n", device],
        180.0,
        command::LIMIT,
        true,
    );
    if !check.ok
        && !(check.reason.as_deref() == Some("command_failed")
            && fat_benign_only(check.output.as_deref()))
    {
        return Ok(Some("old_storage_check_failed"));
    }
    let target = root.path(OLD);
    if is_symlink(&target) {
        return Ok(Some("old_storage_path_unsafe"));
    }
    mkdir(&target, 0o700)?;
    let result = command::run(
        root,
        &[
            "mount",
            "-t",
            "vfat",
            "-o",
            "ro,nosuid,nodev,noexec,utf8=1,fmask=0177,dmask=0077",
            device,
            OLD,
        ],
        20.0,
    );
    let mounted: Vec<Mount> = all_mounts(root)
        .into_iter()
        .filter(|m| m.path == OLD)
        .collect();
    if !result.ok || mounted.len() != 1 || mounted[0].source != device {
        return Ok(Some("old_storage_mount_failed"));
    }
    Ok(None)
}

fn unmount_old(root: &Root) -> bool {
    let mounted = |root: &Root| all_mounts(root).iter().any(|m| m.path == OLD);
    if mounted(root) {
        let result = command::run(root, &["umount", OLD], 10.0);
        if !result.ok || mounted(root) {
            return false;
        }
    }
    true
}

/// The resume record, durable: the file and then its folder are synced before a phase starts.
pub(super) fn save(root: &Root, value: &Object) -> Result<(), Failure> {
    let folder = root.path(SETUP_FOLDER);
    mkdir(&folder, 0o700)?;
    atomic_json(&root.path(SETUP), value, true)?;
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open(&folder)
        .at(&folder)?;
    directory.sync_all().at(&folder)
}

/// The setup's resume record on Y2DATA (it survives formatting p8): the partition, the old
/// storage's serial, the phase reached and what is kept. A phase moves on in place: fields this
/// version does not know stay as they are.
struct Resume(Object);

impl Resume {
    fn new(device: &str, serial: Value, kept: &[(String, u64)]) -> Resume {
        let total: u64 = kept.iter().map(|(_, size)| size).sum();
        let phase = if kept.is_empty() {
            "formatting"
        } else {
            "staging"
        };
        let fields = [
            ("schema", json!(1)),
            ("device", json!(device)),
            ("start", json!(START)),
            ("size", json!(SIZE)),
            ("serial", serial),
            ("phase", json!(phase)),
            ("files", json!(kept.len())),
            ("bytes", json!(total)),
        ];
        Resume(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
    }

    /// Made for this partition.
    fn names(&self, device: &str) -> bool {
        get_eq(&self.0, "device", &json!(device))
            && get_eq(&self.0, "start", &json!(START))
            && get_eq(&self.0, "size", &json!(SIZE))
    }

    /// The phase reached; a record without one (or with one no version writes) stops the setup
    /// (`setup_record_phase`).
    fn phase(&self) -> Option<&str> {
        get(&self.0, "phase").as_str()
    }

    /// The old storage's serial; without it the record cannot tell the old storage is unchanged
    /// (`setup_record_incomplete`).
    fn serial(&self) -> Option<&Value> {
        self.0.get("serial")
    }

    /// A count as recorded (0 when missing).
    fn count(&self, name: &str) -> Value {
        self.0.get(name).cloned().unwrap_or(json!(0))
    }

    /// The next phase, saved before its work starts.
    fn advance(&mut self, root: &Root, phase: &str) -> Result<(), Failure> {
        self.0.insert("phase".into(), json!(phase));
        save(root, &self.0)
    }
}

/// Turn the stock FAT into Y2Media (see the module note). `automatic` at start decides by itself;
/// otherwise the listener confirmed with the old storage's serial (erases what is too much to keep).
/// Resumes an interrupted run from the Y2DATA record.
pub fn setup(root: &Root, automatic: bool, confirm_serial: &Value) -> Result<Object, Failure> {
    let _lock = lock_runtime(root)?;
    setup_locked(root, automatic, confirm_serial).or_else(|error| {
        let reason: String = error.message().chars().take(100).collect();
        note(root, "Failed", Some(&format!("setup_error:{reason}")))
    })
}

fn setup_locked(root: &Root, automatic: bool, confirm_serial: &Value) -> Result<Object, Failure> {
    let Some(entry) = partition(root) else {
        return note(root, "Unavailable", Some("partition_geometry_not_found"));
    };
    let device = format!("/dev/{}", name(&entry));
    let props = probe(root, &entry);
    let stored = root.json(SETUP);
    if exists(&root.path(SETUP)) && !matches!(stored, Some(Value::Object(_))) {
        return note(root, "Failed", Some("setup_record_unreadable")); // never guess: leave p8 alone
    }
    let mut saved = match stored {
        Some(Value::Object(map)) if !map.is_empty() => {
            let saved = Resume(map);
            if !saved.names(&device) {
                return note(root, "Failed", Some("setup_record_mismatch"));
            }
            if automatic && saved.phase() == Some("staging") {
                if let Some(blocked) = automatic_blocked(root) {
                    return note(root, "Pending", Some(blocked));
                }
            }
            saved
        }
        _ => match begin_setup(root, automatic, confirm_serial, &device, &props)? {
            Start::Run(fresh) => fresh,
            Start::Answer(answer) => return Ok(answer),
        },
    };
    let running = Note {
        files: Some(saved.count("files")),
        bytes: Some(saved.count("bytes")),
        ..Note::new(root, "Running", None)
    };
    publish(root, running)?;

    if saved.phase() == Some("staging") {
        // The old storage must still be the FAT the record was made for.
        if props.get("TYPE") != Some("vfat") {
            return note(root, "Failed", Some("old_storage_changed"));
        }
        let Some(serial) = saved.serial() else {
            return note(root, "Failed", Some("setup_record_incomplete"));
        };
        if !props.value("UUID").py_eq(serial) {
            return note(root, "Failed", Some("old_storage_changed"));
        }
        if let Some(problem) = mount_old(root, &device)? {
            return note(root, "Failed", Some(problem));
        }
        let (source, staging) = (root.path(OLD), root.path(KEPT_STAGING));
        let kept = keepable(&source)?;
        let total: u64 = kept.iter().map(|(_, size)| size).sum();
        let mut done = 0u64;
        for (relative, size) in &kept {
            let target = staging.join(relative);
            if let Some(folder) = target.parent() {
                makedirs(folder, 0o700)?;
            }
            copy_verified(&source.join(relative), &target, *size, sha256)?;
            done += size;
            splash(root, tens(10, 50, done, total));
        }
        sync();
        saved.advance(root, "formatting")?;
    }

    if saved.phase() == Some("formatting") {
        // Old Storage leaves the computer first (its view ended with 'Running' above).
        if !release_view(root, OLD) || !unmount_old(root) {
            return note(root, "Failed", Some("old_storage_busy"));
        }
        let props = probe(root, &entry);
        if !valid(&props) {
            // A FAT there must be the recorded one.
            if props.get("TYPE") == Some("vfat") {
                let Some(serial) = saved.serial() else {
                    return note(root, "Failed", Some("setup_record_incomplete"));
                };
                if !props.value("UUID").py_eq(serial) {
                    return note(root, "Failed", Some("old_storage_changed"));
                }
            }
            // The recorded stock FAT, or what an interrupted format left: only this record
            // (same partition, serial, every kept file verified on Y2DATA) allows formatting.
            if all_mounts(root).iter().any(|m| m.source == device) {
                return note(root, "Failed", Some("partition_is_mounted"));
            }
            splash(root, 50);
            let made = command::run(root, &mkfs(&device), 300.0);
            if !made.ok {
                let reason = format!("mkfs_failed:{}", made.reason.as_deref().unwrap_or("None"));
                return note(root, "Failed", Some(&reason));
            }
        }
        saved.advance(root, "migrating")?;
    }

    if saved.phase() == Some("migrating") {
        splash(root, 60);
        let state = mount_locked(root)?;
        if !state.ready() {
            let reason = state.reason.as_deref().unwrap_or("None");
            return note(root, "Failed", Some(&format!("mount_after_setup:{reason}")));
        }
        mkdir(&root.path(MUSIC), 0o755)?;
        let staging = root.path(KEPT_STAGING);
        if staging.is_dir() {
            let target = format!("{MUSIC}/{KEPT}");
            let progress = &mut |done, total| splash(root, tens(60, 90, done, total));
            migrate(root, KEPT_STAGING, &target, progress, sha256)?;
            remove_empty_below(&staging);
            let _ = fs::remove_dir(&staging);
        }
        migrate(root, LEGACY_MUSIC, MUSIC, &mut |_, _| {}, sha256)?; // the old /data/music library, as provision does
        sync();
        let path = root.path(SETUP);
        fs::remove_file(&path).at(&path)?;
        splash(root, 100);
        let done = Note {
            kept: Some(saved.count("files")),
            ..Note::new(root, "Done", None)
        };
        return publish(root, done);
    }
    note(root, "Failed", Some("setup_record_phase"))
}

/// What a setup without a resume record does: run with the record it saved, or answer at once.
enum Start {
    Run(Resume),
    Answer(Object),
}

/// A setup without a resume record decides from a read-only look at the old storage.
fn begin_setup(
    root: &Root,
    automatic: bool,
    confirm_serial: &Value,
    device: &str,
    props: &Properties,
) -> Result<Start, Failure> {
    if valid(props) {
        return note(root, "NotNeeded", None).map(Start::Answer);
    }
    if props.get("TYPE") != Some("vfat") {
        let other = Note {
            filesystem: Some(props.value("TYPE")),
            ..Note::new(root, "Unavailable", Some("not_stock_storage"))
        };
        return publish(root, other).map(Start::Answer);
    }
    let serial = props.value("UUID");
    if automatic {
        if let Some(blocked) = automatic_blocked(root) {
            return note(root, "Pending", Some(blocked)).map(Start::Answer);
        }
    } else if !truthy(confirm_serial) || !confirm_serial.py_eq(&serial) {
        let reason = "confirmation_does_not_match_stock_filesystem";
        return note(root, "Failed", Some(reason)).map(Start::Answer);
    }
    if all_mounts(root)
        .iter()
        .any(|m| m.source == device && m.path != OLD)
    {
        return note(root, "Failed", Some("partition_is_mounted")).map(Start::Answer);
    }
    splash(root, 0);
    let problem = mount_old(root, device)?;
    let mut kept = match problem {
        Some(_) => Vec::new(),
        None => keepable(&root.path(OLD))?,
    };
    let case = match problem {
        Some(_) => 3,
        None => {
            let data = root.path("/data");
            let free = statvfs(&data).at(&data)?;
            decide(&kept, free.available())
        }
    };
    if case == 3 {
        if automatic {
            // Nothing changes; the old files stay readable (Old Storage) until the listener decides.
            let total: u64 = kept.iter().map(|(_, size)| size).sum();
            let waiting = Note {
                files: Some(json!(kept.len())),
                bytes: Some(json!(total)),
                serial: Some(serial),
                ..Note::new(
                    root,
                    "Waiting",
                    Some(problem.unwrap_or("old_storage_has_files")),
                )
            };
            return publish(root, waiting).map(Start::Answer);
        }
        // confirmed: what was too much to keep is erased
        kept.clear();
    }
    let fresh = Resume::new(device, serial, &kept);
    save(root, &fresh.0)?;
    Ok(Start::Run(fresh))
}

/// Remove every directory below `top` that is empty once the ones below it are gone (symlinks are
/// not followed).
fn remove_empty_below(top: &Path) {
    let Ok(entries) = fs::read_dir(top) else {
        return;
    };
    let directories: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    for directory in directories {
        if !is_symlink(&directory) {
            remove_empty_below(&directory);
        }
        let _ = fs::remove_dir(&directory);
    }
}

/// `setup --confirm-shown`: the serial this boot reported as waiting, else `null`.
pub fn shown_serial(root: &Root) -> Value {
    let shown = setup_status(root);
    if get(&shown, "state").as_str() == Some("Waiting") {
        get(&shown, "serial").clone()
    } else {
        Value::Null
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leftovers_and_steps() {
        assert!(leftover("DCIM/.thumbnails/x.jpg") && leftover("Thumbs.db") && leftover("a/._x"));
        assert!(leftover(
            "Android/data/com.android.providers.media/albumthumbs/1"
        ));
        assert!(!leftover("Music/a.mp3") && !leftover(".thumbnails"));
        assert_eq!(tens(10, 50, 3, 4), 40);
        assert_eq!(tens(60, 90, 0, 0), 60);
        assert_eq!(tens(10, 50, 5, 4), 50);
        assert_eq!(decide(&[], 0), 1);
        assert_eq!(decide(&[("a".into(), 10)], 200_000_000), 2);
        assert_eq!(decide(&[("a".into(), 10)], 100), 3);
    }
}
