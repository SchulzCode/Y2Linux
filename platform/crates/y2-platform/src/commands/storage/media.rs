//! The single explicit SD lifecycle, guarded by controller and filesystem identity
//! (`media mount|unmount|check|status|reconcile|verify|format|grow|repair|write`): here the cards the
//! controller offers, their admission and the insertion/removal work; the filesystem checks and
//! `verify` in `check`, write windows in `window`, `format`/`grow` in `change`, `repair` in `repair`.
// SPDX-License-Identifier: GPL-2.0-only
use super::observe::{media_identity, volumes, MEDIA_MOUNT};
use super::records::{Claim, Properties, ReadOnly};
use super::sys::{
    device_instance, exists, get, is_symlink, lock_now, mkdir, name, object, read, release_sd,
    resolve, to_object, Mount, Object,
};
use crate::reply::{atomic_json, unlink_missing_ok, Failure};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};
use y2p_core::{
    command,
    mounts::mounts_at,
    py::{exact_int, truthy, PyValue},
    Root,
};

mod change;
mod check;
mod repair;
mod window;

pub use change::change_card;
pub use check::verify;
use check::{fallback_filesystem, read_only_check, remember_admission, CHECK_SECONDS};
pub use repair::repair;
use window::{protect, PROTECT_OFF};
pub use window::{window_expired, write_window};

/// Whether the SD card takes writes now or on a window: our mount of this boot, read-write or a
/// protected card (never a damaged one or one being checked).
pub fn can_write(root: &Root) -> bool {
    let (claim, mount) = window::sd_state(root);
    mount.is_some_and(|m| {
        m.has_option("rw") || matches!(claim.read_only(), Some(ReadOnly::Protected))
    })
}

const SD: &str = "/media/sd";
const CONTROLLER: &str = "/11240000.mmc/";

fn set(map: &mut Object, key: &str, value: impl Into<Value>) {
    map.insert(key.into(), value.into());
}

fn set_state(map: &mut Object, state: &str, reason: impl Into<Value>) {
    set(map, "state", state);
    set(map, "reason", reason);
}

fn state_is(map: &Object, state: &str) -> bool {
    get(map, "state").as_str() == Some(state)
}

/// The first 12 characters of an id: records name a card by this prefix, never by its CID.
fn prefix(id: &str) -> String {
    id.chars().take(12).collect()
}

fn milliseconds(started: Instant) -> u64 {
    (started.elapsed().as_secs_f64() * 1000.0) as u64
}

fn runtime(root: &Root) -> Result<PathBuf, Failure> {
    let runtime = root.path("/run/y2");
    mkdir(&runtime, 0o700)?;
    Ok(runtime)
}

/// The start of every operation record.
fn begin(root: &Root, action: &str) -> Object {
    let mut state = Object::new();
    set(&mut state, "schema", 1);
    set(&mut state, "action", action);
    set_state(&mut state, "Starting", Value::Null);
    set(&mut state, "boot_id", Value::from(root.boot_id()));
    state
}

/// One card the SD controller offers: a supported filesystem with a stable identity (its fields
/// start the operation record of the action that found it).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub(crate) struct Card {
    pub device: String,
    pub filesystem: &'static str,
    pub uuid: Option<String>,
    pub cid: Option<String>,
    pub identity: Object,
    pub source_instance: String,
}

impl Card {
    /// The identity hash (`identity.id`).
    fn id(&self) -> &str {
        get(&self.identity, "id").as_str().unwrap_or_default()
    }
}

/// `list(entry.glob('*/partition'))` is not empty: the whole disk of a partitioned card.
fn has_partitions(entry: &Path) -> bool {
    fs::read_dir(entry)
        .into_iter()
        .flatten()
        .flatten()
        .any(|child| {
            child.path().is_dir() && fs::symlink_metadata(child.path().join("partition")).is_ok()
        })
}

pub(crate) fn candidates(root: &Root) -> Vec<Card> {
    let mut result = Vec::new();
    for entry in root.glob("/sys/class/block/*") {
        if !resolve(&entry).to_string_lossy().contains(CONTROLLER) {
            continue;
        }
        if !entry.join("partition").is_file() && has_partitions(&entry) {
            continue;
        }
        let device = format!("/dev/{}", name(&entry));
        let answer = command::run(
            root,
            &["blkid", "-p", "-o", "export", &device],
            command::TIMEOUT,
        );
        if !answer.ok {
            continue;
        }
        let props = Properties::parse(answer.output.as_deref());
        let Some(filesystem) = ["ext4", "vfat", "exfat"]
            .into_iter()
            .find(|t| props.get("TYPE") == Some(t))
        else {
            continue;
        };
        let uuid = props.get("UUID");
        let identity = media_identity(&entry, filesystem, uuid);
        let instance = device_instance(root, &read(&entry.join("dev")).unwrap_or_default());
        let (Some(identity), Some(source_instance)) = (identity, instance) else {
            continue;
        };
        result.push(Card {
            device,
            filesystem,
            uuid: uuid.map(str::to_owned),
            cid: get(&identity, "cid").as_str().map(str::to_owned),
            identity,
            source_instance,
        });
    }
    result
}

fn owned_mount(root: &Root, mounted: &Mount) -> bool {
    let device = root.path(&format!("/sys/dev/block/{}", mounted.device_id));
    if exists(&device) {
        return resolve(&device).to_string_lossy().contains(CONTROLLER);
    }
    let claim = Claim::load(root);
    // Removed cards no longer have a sysfs controller path. The retained claim
    // permits only the exact mount we created, never an arbitrary mountpoint.
    claim.holds(root, mounted.mount_id)
        && claim.get("device_id").py_eq(&json!(mounted.device_id))
        && claim.get("device").py_eq(&json!(mounted.source))
}

/// SD presence by stable kernel identity, never by kernfs inode/ctime.
///
/// Page-cache drops and ordinary memory pressure reclaim kernfs inodes, so a
/// recreated inode is not a new card. dev_t and the disk sequence number stay
/// fixed for one gendisk; every re-enumeration allocates a new diskseq, which
/// also distinguishes a reinserted identical card (same CID).
pub(crate) fn inventory(root: &Root) -> Value {
    let mut result = Vec::new();
    for entry in root.glob("/sys/class/block/mmcblk*") {
        let device = resolve(&entry);
        if device.to_string_lossy().contains(CONTROLLER) {
            let disk = if read(&entry.join("partition")).is_some() {
                device.parent().map(Path::to_path_buf).unwrap_or(device)
            } else {
                device
            };
            result.push(json!([
                name(&entry),
                read(&entry.join("dev")),
                read(&disk.join("diskseq")),
                read(&disk.join("device/cid"))
            ]));
        }
    }
    Value::Array(result)
}

/// Insertion/removal work; no automatic retry storm or remount after eject.
pub fn reconcile(root: &Root) -> Result<Object, Failure> {
    let runtime = runtime(root)?;
    let current = inventory(root);
    let previous = object(root, "/run/y2/media-lifecycle.json");
    let same = get(&previous, "inventory").py_eq(&current);
    if same && !truthy(get(&previous, "retry")) {
        protect(root)?;
        return Ok(previous);
    }
    let attempts = if same {
        // A flag counts as 0 or 1, anything else that is no integer as 0.
        match previous.get("attempts") {
            Some(Value::Bool(b)) => i64::from(*b),
            other => exact_int(other).unwrap_or(0),
        }
    } else {
        0
    };
    let mut result = Object::new();
    set(&mut result, "schema", 1);
    set(&mut result, "inventory", current.clone());
    set(&mut result, "attempts", attempts + 1);
    set(&mut result, "retry", false);
    let lifecycle = runtime.join("media-lifecycle.json");
    if !mounts_at(root, SD).is_empty() {
        result.extend(operation(root, "unmount")?);
        if !mounts_at(root, SD).is_empty() {
            set(&mut result, "retry", attempts < 4);
            set(&mut result, "reason", "source_change_unmount_failed");
            atomic_json(&lifecycle, &result, false)?;
            return Ok(result);
        }
    }
    if current.as_array().is_some_and(|c| !c.is_empty()) {
        result.extend(operation(root, "mount")?);
    } else {
        set_state(&mut result, "Unavailable", "no_card");
    }
    atomic_json(&lifecycle, &result, false)?;
    Ok(result)
}

pub fn operation(root: &Root, action: &str) -> Result<Object, Failure> {
    if action == "status" {
        let [_, _, sd, _] = volumes(root);
        return Ok(sd);
    }
    let runtime = runtime(root)?;
    let _lock = lock_now(&runtime.join("media.lock"))?;
    let record = runtime.join("media.json");
    let mut state = begin(root, action);
    let old = mounts_at(root, SD);
    if !old.is_empty() && old.iter().any(|m| !owned_mount(root, m)) {
        set_state(&mut state, "Failed", "mounted_source_not_sd");
        atomic_json(&record, &state, false)?;
        return Ok(state);
    }
    if action == "unmount" {
        if old.len() > 1 {
            set_state(&mut state, "Failed", "ambiguous_mount_stack");
        } else if let Some(mounted) = old.first() {
            if !release_sd(root, mounted.mount_id, false)? {
                set_state(&mut state, "Failed", "usb_transfer_connected_or_busy");
                atomic_json(&record, &state, false)?;
                return Ok(state);
            }
            let result = command::run(root, &["umount", SD], 3.0);
            if !result.ok || !mounts_at(root, SD).is_empty() {
                set_state(
                    &mut state,
                    "Failed",
                    result.reason.unwrap_or_else(|| "unmount_busy".into()),
                );
            } else {
                set_state(&mut state, "Unavailable", "ejected");
            }
        } else {
            set_state(&mut state, "Unavailable", "not_mounted");
        }
    } else if !old.is_empty() {
        // Never check/repair an active filesystem or stack a new card on it.
        set_state(
            &mut state,
            "Failed",
            if action == "check" {
                "already_mounted"
            } else {
                "mount_requires_unmounted_path"
            },
        );
    } else {
        let available = candidates(root);
        if let [card] = &available[..] {
            state.extend(to_object(card));
            if action == "check" {
                let check = read_only_check(root, card, true, Some(CHECK_SECONDS));
                set(&mut state, "filesystem_check", check.value());
                if check.ok {
                    set_state(&mut state, "Ready", "read_only_check_passed");
                } else {
                    set_state(&mut state, "Degraded", check.reason);
                }
            } else if let Some(early) = admit(root, &mut state, card, &available, &record)? {
                return Ok(early);
            }
        } else {
            set_state(&mut state, "Unavailable", "no_unique_supported_filesystem");
        }
    }
    atomic_json(&record, &state, false)?;
    Ok(state)
}

/// The mount branch of `operation` after the unique card was found; `Some` when it returns early
/// (its record already written).
fn admit(
    root: &Root,
    state: &mut Object,
    card: &Card,
    available: &[Card],
    record: &Path,
) -> Result<Option<Object>, Failure> {
    let filesystem = card.filesystem;
    let device = card.device.as_str();
    // Read-only admission never authorizes checker repairs.
    // Large/unhealthy media may time out;
    // the owner must see that failure rather than a RW mount.
    let check = read_only_check(root, card, false, None);
    set(state, "filesystem_check", check.value());
    remember_admission(root, card, &check);
    let mut read_only = ReadOnly::ReadWrite;
    if check.timed_out() && fallback_filesystem(filesystem) {
        // Not finished is not damaged (F49): it plays read-only
        // now and `verify` finishes the check in the background.
        read_only = ReadOnly::Checking;
    } else if !check.ok {
        set_state(state, "Degraded", "filesystem_check_failed");
        // FAT/exFAT with a finding mounts read-only: reading never changes a card,
        // writing into a damaged table can overwrite another file.
        // The music plays; Repair (or a computer) makes it writable.
        // ext4 stays out: even a read-only mount replays its journal.
        if !fallback_filesystem(filesystem) {
            atomic_json(record, state, false)?;
            return Ok(Some(state.clone()));
        }
        read_only = ReadOnly::FilesystemErrors;
    }
    if candidates(root) != available {
        set_state(state, "Failed", "source_changed_during_check");
        atomic_json(record, state, false)?;
        return Ok(Some(state.clone()));
    }
    let target = root.path(SD);
    if is_symlink(&target) {
        return Err(Failure::value("mountpoint_symlink"));
    }
    mkdir(&target, 0o777)?;
    // POWER-FINAL-30: noatime on every card format. relatime still
    // wrote an access time the first time each song was read every
    // day: a metadata write to the card during playback.
    // A healthy card is protected too: read-only until something
    // writes to it (write_window). Pulled during playback it stays
    // clean; only an interrupted write can damage a FAT card.
    if read_only == ReadOnly::ReadWrite && !exists(&root.path(PROTECT_OFF)) {
        read_only = ReadOnly::Protected;
    }
    let mut options = format!(
        "{},nosuid,nodev,noexec,noatime",
        if read_only == ReadOnly::ReadWrite {
            "rw"
        } else {
            "ro"
        }
    );
    if fallback_filesystem(filesystem) {
        options += ",fmask=0177,dmask=0077";
    }
    if filesystem == "vfat" {
        options += ",utf8=1";
    }
    let result = command::run(
        root,
        &["mount", "-t", filesystem, "-o", &options, device, SD],
        3.0,
    );
    let current = mounts_at(root, SD);
    match &current[..] {
        [mounted] if result.ok && mounted.source == device => {
            let damaged = read_only == ReadOnly::FilesystemErrors;
            set(state, "state", if damaged { "Degraded" } else { "Ready" });
            set(state, "mount_id", mounted.mount_id);
            set(state, "device_id", mounted.device_id.clone());
            set(state, "read_only", read_only.value());
            set(
                state,
                "reason",
                if damaged { Some("read_only") } else { None },
            );
            set(
                state,
                "source_instance",
                device_instance(root, &mounted.device_id),
            );
            if candidates(root) != available {
                set_state(state, "Failed", "source_changed_during_mount");
            } else {
                atomic_json(&root.path(MEDIA_MOUNT), state, false)?;
                // The kernel recycles mount ids, so a release marker for a card that was
                // ejected or reformatted must not hide the next mount that reuses its id.
                unlink_missing_ok(&root.path("/run/y2/usb-release-sd.json"))?;
            }
        }
        _ => set_state(
            state,
            "Failed",
            result
                .reason
                .unwrap_or_else(|| "mount_identity_failed".into()),
        ),
    }
    Ok(None)
}

#[cfg(test)]
#[path = "media_tests.rs"]
mod media_tests;
