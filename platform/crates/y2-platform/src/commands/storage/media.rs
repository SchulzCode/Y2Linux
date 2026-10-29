//! `media.py`: the single explicit SD lifecycle, guarded by controller and filesystem identity
//! (`media mount|unmount|check|status|reconcile|verify|format|grow|repair|write`).
// SPDX-License-Identifier: GPL-2.0-only
use super::observe::{media_identity, refused_card, volumes, MEDIA_MOUNT, MEDIA_STATE};
use super::sys::{
    available, boot_id, device_instance, exists, get, get_eq, get_in, is_symlink, isdigit,
    lock_now, lock_within, maintenance, mkdir, mounts as all_mounts, name, no_attribute, object,
    py_eq, read, release_sd, resolve, statvfs, try_lock, wall_seconds, write_record, Mount, Object,
};
use crate::reply::{atomic_json, unlink_missing_ok, Failure};
use serde_json::{json, Value};
use std::{
    ffi::CStr,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    os::unix::{
        fs::{FileExt, OpenOptionsExt},
        io::AsRawFd,
    },
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
use y2p_core::{
    clock, command,
    py::{exact_int, is_true, number, truthy},
    storage_media::{
        fat_benign_only, fat_findings, fat_other_shapes, repair_summary, splitlines, tables_bytes,
    },
    Root,
};

const SD: &str = "/media/sd";
const CONTROLLER: &str = "/11240000.mmc/";

// F49 (d32f8f3c, 8.41): fsck.exfat -n took 59 s on a 1 TB card with 71 400
// songs and ran out of the old 60 s at a few more; the card then stayed
// read-only behind a Repair that found nothing. A FAT/exFAT card whose check
// does not finish within ADMISSION_CHECK_SECONDS is mounted read-only at once
// ('checking') and checked again in the background (verify) for CHECK_SECONDS.
const ADMISSION_CHECK_SECONDS: f64 = 20.0;
const EXT4_CHECK_SECONDS: f64 = 60.0; // no background check: a read-only ext4 mount replays its journal
const CHECK_SECONDS: f64 = 1800.0;

// Why the last cards were refused or let in, across boots (F12: a card refused
// at every insertion left no reason in a report). No checker output, no names.
const ADMISSIONS: &str = "/data/system/platform/sd-admissions.json";
const ADMISSIONS_KEPT: usize = 10;
const READ_ONLY_FALLBACK: [&str; 2] = ["vfat", "exfat"];

fn fallback_filesystem(filesystem: &str) -> bool {
    READ_ONLY_FALLBACK.contains(&filesystem)
}

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

fn card_text<'a>(card: &'a Object, key: &str) -> &'a str {
    get(card, key).as_str().unwrap_or_default()
}

fn card_id(card: &Object) -> &str {
    match card.get("identity") {
        Some(Value::Object(identity)) => get(identity, "id").as_str().unwrap_or_default(),
        _ => "",
    }
}

/// The first 12 characters of an id, as `id[:12]`.
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
    set(&mut state, "boot_id", boot_id(root));
    state
}

/// The admission check, never a repair. The record keeps no checker output from a failed run (it
/// can name files); benign findings pass with `finding`, and every run names its classes in
/// `findings`.
fn read_only_check(root: &Root, card: &Object, force: bool, timeout: Option<f64>) -> Object {
    let filesystem = card_text(card, "filesystem");
    let mut argv: Vec<&str> = match filesystem {
        "ext4" if force => vec!["e2fsck", "-f", "-n"],
        "ext4" => vec!["e2fsck", "-n"],
        "vfat" => vec!["fsck.fat", "-n"],
        _ => vec!["fsck.exfat", "-n"],
    };
    let timeout = timeout.unwrap_or(if fallback_filesystem(filesystem) {
        ADMISSION_CHECK_SECONDS
    } else {
        EXT4_CHECK_SECONDS
    });
    argv.push(card_text(card, "device"));
    let started = Instant::now();
    let result = command::run_with(root, &argv, timeout, command::LIMIT, filesystem == "vfat");
    let mut check = Object::new();
    set(&mut check, "ok", result.ok);
    set(&mut check, "reason", json!(result.reason));
    set(
        &mut check,
        "output",
        if result.ok {
            json!(result.output)
        } else {
            Value::Null
        },
    );
    set(&mut check, "elapsed_ms", milliseconds(started));
    let output = result.output.as_deref();
    if result.reason.as_deref() == Some("command_timeout") {
        set(&mut check, "findings", json!(["timeout"]));
    } else if !result.ok
        && result.reason.as_deref() == Some("command_failed")
        && filesystem == "vfat"
    {
        let findings = fat_findings(output);
        if findings.iter().any(|f| f == "other") {
            set(&mut check, "other", fat_other_shapes(output, 5));
        }
        let only_dirty = findings == ["dirty"];
        set(&mut check, "findings", json!(findings));
        if fat_benign_only(output) {
            // 'unclean_unmount' is the #44 name for the dirty bit alone.
            set(&mut check, "ok", true);
            set(
                &mut check,
                "finding",
                if only_dirty {
                    "unclean_unmount"
                } else {
                    "benign"
                },
            );
        }
    } else if !result.ok {
        set(&mut check, "findings", json!(["other"]));
    }
    check
}

fn remember_admission(root: &Root, card: &Object, check: &Object) {
    let old = object(root, ADMISSIONS);
    let mut entries: Vec<Value> = match old.get("admissions") {
        None => Vec::new(),
        Some(Value::Array(items)) => items.iter().filter(|e| e.is_object()).cloned().collect(),
        // A text or an object iterates to no dicts; anything else is not iterable (TypeError).
        Some(Value::String(_)) | Some(Value::Object(_)) => Vec::new(),
        Some(_) => return,
    };
    let mut entry = Object::new();
    set(&mut entry, "time", wall_seconds());
    set(&mut entry, "card", prefix(card_id(card)));
    set(&mut entry, "filesystem", get(card, "filesystem").clone());
    set(&mut entry, "ok", truthy(get(check, "ok")));
    set(&mut entry, "reason", get(check, "reason").clone());
    set(&mut entry, "finding", get(check, "finding").clone());
    set(
        &mut entry,
        "findings",
        check.get("findings").cloned().unwrap_or(json!([])),
    );
    set(&mut entry, "elapsed_ms", get(check, "elapsed_ms").clone());
    if truthy(get(check, "other")) {
        set(&mut entry, "other", get(check, "other").clone());
    }
    if truthy(get(check, "background")) {
        set(&mut entry, "background", true);
    }
    entries.push(Value::Object(entry));
    let path = root.path(ADMISSIONS);
    let directory = path.parent().expect("parent").to_path_buf();
    if mkdir(&directory, 0o700).is_err() {
        return;
    }
    let kept = entries.split_off(entries.len().saturating_sub(ADMISSIONS_KEPT));
    let _ = atomic_json(&path, &json!({"schema": 1, "admissions": kept}), false);
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

/// `dict(line.split('=', 1) for line in output.splitlines() if '=' in line)`.
pub fn properties(output: Option<&str>) -> Object {
    let mut props = Object::new();
    for line in splitlines(output.unwrap_or_default()) {
        if let Some((key, value)) = line.split_once('=') {
            props.insert(key.into(), json!(value));
        }
    }
    props
}

pub fn candidates(root: &Root) -> Vec<Object> {
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
        let props = properties(answer.output.as_deref());
        let Some(kind) = get(&props, "TYPE")
            .as_str()
            .filter(|t| ["ext4", "vfat", "exfat"].contains(t))
        else {
            continue;
        };
        let uuid = get(&props, "UUID").clone();
        let identity = media_identity(&entry, kind, uuid.as_str());
        let instance = device_instance(root, &read(&entry.join("dev")).unwrap_or_default());
        let (Some(identity), Some(instance)) = (identity, instance) else {
            continue;
        };
        let mut card = Object::new();
        set(&mut card, "device", device);
        set(&mut card, "filesystem", kind);
        set(&mut card, "uuid", uuid);
        set(&mut card, "cid", get(&identity, "cid").clone());
        set(&mut card, "identity", Value::Object(identity));
        set(&mut card, "source_instance", instance);
        result.push(card);
    }
    result
}

fn mounts(root: &Root) -> Vec<Mount> {
    all_mounts(root)
        .into_iter()
        .filter(|m| m.path == SD)
        .collect()
}

fn owned_mount(root: &Root, mounted: &Mount) -> bool {
    let device = root.path(&format!("/sys/dev/block/{}", mounted.device_id));
    if exists(&device) {
        return resolve(&device).to_string_lossy().contains(CONTROLLER);
    }
    let claim = object(root, MEDIA_MOUNT);
    // Removed cards no longer have a sysfs controller path. The retained claim
    // permits only the exact mount we created, never an arbitrary mountpoint.
    get_eq(&claim, "boot_id", &boot_id(root))
        && get_eq(&claim, "mount_id", &json!(mounted.mount_id))
        && get_eq(&claim, "device_id", &json!(mounted.device_id))
        && get_eq(&claim, "device", &json!(mounted.source))
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
    let same = py_eq(get(&previous, "inventory"), &current);
    if same && !truthy(get(&previous, "retry")) {
        protect(root)?;
        return Ok(previous);
    }
    let attempts = if same {
        previous.get("attempts").cloned().unwrap_or(json!(0))
    } else {
        json!(0)
    };
    // isinstance(attempts, int): a bool is one too.
    let attempts = match attempts {
        Value::Bool(b) => i64::from(b),
        other => exact_int(Some(&other)).unwrap_or(0),
    };
    let mut result = Object::new();
    set(&mut result, "schema", 1);
    set(&mut result, "inventory", current.clone());
    set(&mut result, "attempts", attempts + 1);
    set(&mut result, "retry", false);
    let lifecycle = runtime.join("media-lifecycle.json");
    if !mounts(root).is_empty() {
        result.extend(operation(root, "unmount")?);
        if !mounts(root).is_empty() {
            set(&mut result, "retry", attempts < 4);
            set(&mut result, "reason", "source_change_unmount_failed");
            write_record(&lifecycle, &result)?;
            return Ok(result);
        }
    }
    if current.as_array().is_some_and(|c| !c.is_empty()) {
        result.extend(operation(root, "mount")?);
    } else {
        set_state(&mut result, "Unavailable", "no_card");
    }
    write_record(&lifecycle, &result)?;
    Ok(result)
}

pub fn operation(root: &Root, action: &str) -> Result<Object, Failure> {
    if action == "status" {
        return Ok(volumes(root).swap_remove(2));
    }
    let runtime = runtime(root)?;
    let _lock = lock_now(&runtime.join("media.lock"))?;
    let record = runtime.join("media.json");
    let mut state = begin(root, action);
    let old = mounts(root);
    if !old.is_empty() && old.iter().any(|m| !owned_mount(root, m)) {
        set_state(&mut state, "Failed", "mounted_source_not_sd");
        write_record(&record, &state)?;
        return Ok(state);
    }
    if action == "unmount" {
        if old.len() > 1 {
            set_state(&mut state, "Failed", "ambiguous_mount_stack");
        } else if let Some(mounted) = old.first() {
            if !release_sd(root, mounted.mount_id, false)? {
                set_state(&mut state, "Failed", "usb_transfer_connected_or_busy");
                write_record(&record, &state)?;
                return Ok(state);
            }
            let result = command::run(root, &["umount", SD], 3.0);
            if !result.ok || !mounts(root).is_empty() {
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
        if available.len() != 1 {
            set_state(&mut state, "Unavailable", "no_unique_supported_filesystem");
        } else {
            let card = available[0].clone();
            state.extend(card.clone());
            if action == "check" {
                let result = read_only_check(root, &card, true, Some(CHECK_SECONDS));
                let ok = truthy(get(&result, "ok"));
                let reason = get(&result, "reason").clone();
                set(&mut state, "filesystem_check", Value::Object(result));
                if ok {
                    set_state(&mut state, "Ready", "read_only_check_passed");
                } else {
                    set_state(&mut state, "Degraded", reason);
                }
            } else if let Some(early) = admit(root, &mut state, &card, &available, &record)? {
                return Ok(early);
            }
        }
    }
    write_record(&record, &state)?;
    Ok(state)
}

/// The mount branch of `operation` after the unique card was found; `Some` when it returns early
/// (its record already written).
fn admit(
    root: &Root,
    state: &mut Object,
    card: &Object,
    available: &[Object],
    record: &Path,
) -> Result<Option<Object>, Failure> {
    let filesystem = card_text(card, "filesystem");
    let device = card_text(card, "device");
    // Read-only admission never authorizes checker repairs.
    // Large/unhealthy media may time out;
    // the owner must see that failure rather than a RW mount.
    let result = read_only_check(root, card, false, None);
    set(state, "filesystem_check", Value::Object(result.clone()));
    remember_admission(root, card, &result);
    let mut read_only: Option<&str> = None;
    if get(&result, "findings") == &json!(["timeout"]) && fallback_filesystem(filesystem) {
        // Not finished is not damaged (F49): it plays read-only
        // now and `verify` finishes the check in the background.
        read_only = Some("checking");
    } else if !truthy(get(&result, "ok")) {
        set_state(state, "Degraded", "filesystem_check_failed");
        // FAT/exFAT with a finding mounts read-only: reading never changes a card,
        // writing into a damaged table can overwrite another file.
        // The music plays; Repair (or a computer) makes it writable.
        // ext4 stays out: even a read-only mount replays its journal.
        if !fallback_filesystem(filesystem) {
            write_record(record, state)?;
            return Ok(Some(state.clone()));
        }
        read_only = Some("filesystem_errors");
    }
    if candidates(root) != available {
        set_state(state, "Failed", "source_changed_during_check");
        write_record(record, state)?;
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
    let protect = read_only.is_none() && !exists(&root.path(PROTECT_OFF));
    if protect {
        read_only = Some("protected");
    }
    let mut options = format!(
        "{},nosuid,nodev,noexec,noatime",
        if read_only.is_some() { "ro" } else { "rw" }
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
    let current = mounts(root);
    if result.ok && current.len() == 1 && current[0].source == device {
        let damaged = !matches!(read_only, None | Some("protected" | "checking"));
        set(state, "state", if damaged { "Degraded" } else { "Ready" });
        set(state, "mount_id", current[0].mount_id);
        set(state, "device_id", current[0].device_id.clone());
        set(state, "read_only", json!(read_only));
        set(
            state,
            "reason",
            if damaged {
                json!("read_only")
            } else {
                Value::Null
            },
        );
        set(
            state,
            "source_instance",
            json!(device_instance(root, &current[0].device_id)),
        );
        if candidates(root) != available {
            set_state(state, "Failed", "source_changed_during_mount");
        } else {
            write_record(&root.path(MEDIA_MOUNT), state)?;
            // The kernel recycles mount ids, so a release marker for a card that was
            // ejected or reformatted must not hide the next mount that reuses its id.
            unlink_missing_ok(&root.path("/run/y2/usb-release-sd.json"))?;
        }
    } else {
        set_state(
            state,
            "Failed",
            result
                .reason
                .unwrap_or_else(|| "mount_identity_failed".into()),
        );
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// Explicit owner operations on the removable card: format and grow.
// Never automatic: a card with a foreign or undersized filesystem is reported,
// and only a command that names the card by CID may change it.

const MIN_CARD_SECTORS: u64 = 16384; // 8 MiB; anything smaller is not a storage card
const PARTITION_START: u64 = 2048; // 1 MiB alignment
const WIPE_TAIL_SECTORS: u64 = 64; // a stale GPT backup header lives in the last sectors
const BLKRRPART: libc::c_ulong = 0x125F;

/// The unique whole-disk entry on the removable-card controller, else None.
fn sd_disk(root: &Root) -> Option<PathBuf> {
    let found: Vec<PathBuf> = root
        .glob("/sys/class/block/mmcblk*")
        .into_iter()
        .filter(|entry| {
            resolve(entry).to_string_lossy().contains(CONTROLLER)
                && read(&entry.join("partition")).is_none()
        })
        .collect();
    (found.len() == 1).then(|| found[0].clone())
}

/// One Linux partition from sector 2048 to the end of the card.
fn mbr_for(sectors: u64) -> Vec<u8> {
    let size = (sectors - PARTITION_START).min(0xFFFF_FFFF) as u32;
    let mut mbr = vec![0u8; 446];
    mbr.extend_from_slice(&[0, 0xfe, 0xff, 0xff, 0x83, 0xfe, 0xff, 0xff]);
    mbr.extend_from_slice(&(PARTITION_START as u32).to_le_bytes());
    mbr.extend_from_slice(&size.to_le_bytes());
    mbr.extend_from_slice(&[0u8; 48]);
    mbr.extend_from_slice(&[0x55, 0xaa]);
    mbr
}

/// Release the card from MTP and unmount it, for an explicit owner command.
fn detach_for_change(root: &Root, state: &mut Object) -> Result<bool, Failure> {
    let old = mounts(root);
    if old.is_empty() {
        return Ok(true);
    }
    if old.len() > 1 || old.iter().any(|m| !owned_mount(root, m)) {
        set_state(state, "Failed", "ambiguous_or_foreign_mount");
        return Ok(false);
    }
    if !release_sd(root, old[0].mount_id, true)? {
        set_state(state, "Failed", "usb_release_failed");
        return Ok(false);
    }
    let result = command::run(root, &["umount", SD], 10.0);
    if !result.ok || !mounts(root).is_empty() {
        set_state(
            state,
            "Failed",
            result.reason.unwrap_or_else(|| "unmount_busy".into()),
        );
        return Ok(false);
    }
    Ok(true)
}

/// action: 'format' (new table + ext4 over the whole card) or 'grow' (ext4 to the partition).
pub fn change_card(
    root: &Root,
    action: &str,
    confirm_cid: Option<&str>,
) -> Result<Object, Failure> {
    let mut state = apply_change(root, action, confirm_cid)?;
    if state_is(&state, "Starting") {
        // Mount through the normal admission path, after the change released the media lock.
        state.extend(operation(root, "mount")?);
        set(&mut state, "action", action);
    }
    Ok(state)
}

// ---------------------------------------------------------------------------
// Protected card: a healthy card is mounted read-only and becomes writable only
// while something writes to it — the computer over USB, a playlist change, a
// report saved to the card, the firmware import removing its system.img. Each
// writer holds a named window with an end time; the reconcile tick (3 s) turns
// the card read-only again when none is left. A remount to read-only flushes the
// card and clears its dirty bit, so a card pulled out during playback is clean.

const PROTECT_OFF: &str = "/data/system/platform/sd-always-writable"; // owner switch: the old rw mount
const SD_WRITE: &str = "/run/y2/sd-write.json";
const WRITE_HOLD_MAX: i64 = 600;
pub const WRITE_HOLDERS: [&str; 4] = ["usb", "reborn", "report", "radiofw"];

/// (claim, mount) of our current SD mount, else (claim, None).
fn sd_state(root: &Root) -> (Object, Option<Mount>) {
    let claim = object(root, MEDIA_MOUNT);
    let old = mounts(root);
    if old.len() == 1
        && owned_mount(root, &old[0])
        && get_eq(&claim, "mount_id", &json!(old[0].mount_id))
        && get_eq(&claim, "boot_id", &boot_id(root))
    {
        let mount = old[0].clone();
        return (claim, Some(mount));
    }
    (claim, None)
}

fn writers(root: &Root, mount: &Mount) -> Result<Object, Failure> {
    let value = object(root, SD_WRITE);
    if !get_eq(&value, "boot_id", &boot_id(root))
        || !get_eq(&value, "mount_id", &json!(mount.mount_id))
    {
        return Ok(Object::new());
    }
    let holders = match value.get("holders") {
        Some(Value::Object(holders)) => holders.clone(),
        // `(value.get('holders') or {}).items()`: not a dict is Python's AttributeError.
        Some(other) if truthy(other) => return Err(no_attribute(other, "items")),
        _ => Object::new(),
    };
    let now = clock::monotonic();
    Ok(holders
        .into_iter()
        .filter(|(k, v)| {
            WRITE_HOLDERS.contains(&k.as_str()) && number(Some(v)).is_some_and(|v| v > now)
        })
        .collect())
}

/// Remount to match the holders; records them. Returns writable now.
fn apply_window(root: &Root, mount: &Mount, holders: &Object) -> Result<bool, Failure> {
    atomic_json(
        &root.path(SD_WRITE),
        &json!({"schema": 1, "boot_id": boot_id(root), "mount_id": mount.mount_id, "holders": holders}),
        false,
    )?;
    let mut writable = mount.has_option("rw");
    if !holders.is_empty() != writable {
        let mode = if holders.is_empty() { "ro" } else { "rw" };
        command::run(root, &["mount", "-o", &format!("remount,{mode}"), SD], 10.0);
        let now = mounts(root);
        writable = now.first().is_some_and(|m| m.has_option("rw"));
    }
    Ok(writable)
}

fn answer(state: &str, reason: Option<&str>, writable: bool) -> Object {
    let mut value = Object::new();
    set_state(&mut value, state, json!(reason));
    set(&mut value, "writable", writable);
    value
}

/// Open (or end) the named holder's window on a protected card.
pub fn write_window(
    root: &Root,
    holder: &str,
    seconds: i64,
    release: bool,
) -> Result<Object, Failure> {
    if !WRITE_HOLDERS.contains(&holder) {
        return Err(Failure::value("unknown_write_holder"));
    }
    let runtime = runtime(root)?;
    // A check or a repair holds the card; a short admission passes.
    let Some(_lock) = lock_within(&runtime.join("media.lock"), 5.0)? else {
        return Ok(answer("Failed", Some("media_operation_in_progress"), false));
    };
    let (claim, mount) = sd_state(root);
    let Some(mount) = mount else {
        return Ok(answer("Unavailable", Some("not_mounted"), false));
    };
    let read_only = get(&claim, "read_only");
    if read_only.as_str() == Some("checking") {
        // Writable once its background check passes (USB asks again every 10 s).
        return Ok(answer("Failed", Some("card_being_checked"), false));
    }
    if !read_only.is_null() && read_only.as_str() != Some("protected") {
        // Errors found or a check unfinished: Repair first, never a window.
        return Ok(answer("Degraded", Some("card_has_errors"), false));
    }
    if read_only.is_null() {
        return Ok(answer("Ready", None, mount.has_option("rw")));
    }
    let mut holders = writers(root, &mount)?;
    if release {
        holders.remove(holder);
    } else {
        let hold = seconds.clamp(1, WRITE_HOLD_MAX);
        holders.insert(holder.into(), json!(clock::monotonic() + hold as f64));
    }
    let writable = apply_window(root, &mount, &holders)?;
    let ok = writable == !holders.is_empty() || release;
    let mut value = answer(
        if ok { "Ready" } else { "Failed" },
        if ok { None } else { Some("remount_failed") },
        writable,
    );
    // sorted(holders): a Map's keys are sorted.
    set(
        &mut value,
        "holders",
        json!(holders.keys().collect::<Vec<_>>()),
    );
    Ok(value)
}

/// Reconcile tick: a protected card with no live window goes read-only.
fn protect(root: &Root) -> Result<(), Failure> {
    let (claim, mount) = sd_state(root);
    match mount {
        Some(m) if get(&claim, "read_only").as_str() == Some("protected") && m.has_option("rw") => {
        }
        _ => return Ok(()),
    }
    // A writer or an operation holds the card; the next tick tries again.
    if let Some(_lock) = try_lock(&root.path("/run/y2/media.lock"))? {
        let (claim, mount) = sd_state(root);
        if let Some(mount) = mount {
            if get(&claim, "read_only").as_str() == Some("protected") {
                let holders = writers(root, &mount)?;
                apply_window(root, &mount, &holders)?;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Background check (F49) of a card mounted read-only ('checking') because its
// check did not finish within the admission budget. The service starts it
// (`media verify`) while the claim says 'checking'. No lock is held while the
// checker reads; the result applies only to the same mount of the same card.

/// CLOCK_BOOTTIME − CLOCK_MONOTONIC: grows only while the system sleeps.
fn slept_ns() -> i128 {
    clock::boottime_ns() as i128 - clock::monotonic_ns() as i128
}

/// `(claim.get('identity') or {}).get('id')`; a truthy identity that is not a dict is Python's
/// AttributeError.
fn identity_id(record: &Object) -> Result<Value, Failure> {
    match record.get("identity") {
        Some(Value::Object(identity)) => Ok(get(identity, "id").clone()),
        Some(other) if truthy(other) => Err(no_attribute(other, "get")),
        _ => Ok(Value::Null),
    }
}

fn card_keys(cards: &[Object]) -> Vec<(Value, Value)> {
    cards
        .iter()
        .map(|c| (json!(card_id(c)), get(c, "source_instance").clone()))
        .collect()
}

fn same_card(cards: &[Object], key: &(Value, Value)) -> bool {
    let keys = card_keys(cards);
    keys.len() == 1 && py_eq(&keys[0].0, &key.0) && py_eq(&keys[0].1, &key.1)
}

pub fn verify(root: &Root) -> Result<Object, Failure> {
    let mut state = Object::new();
    set(&mut state, "schema", 1);
    set(&mut state, "action", "verify");
    set_state(&mut state, "Starting", Value::Null);
    let (claim, mount) = sd_state(root);
    if mount.is_none() || get(&claim, "read_only").as_str() != Some("checking") {
        set_state(&mut state, "Unavailable", "nothing_to_check");
        return Ok(state);
    }
    let key = (identity_id(&claim)?, get(&claim, "source_instance").clone());
    let available = candidates(root);
    if !same_card(&available, &key) {
        set_state(&mut state, "Failed", "card_changed");
        return Ok(state);
    }
    let card = &available[0];
    let slept = slept_ns();
    let mut result = read_only_check(root, card, false, Some(CHECK_SECONDS));
    set(&mut result, "background", true);
    let ok = truthy(get(&result, "ok"));
    if !ok && slept_ns() - slept > 1_000_000_000 {
        // A check read across a system sleep is no verdict; the service runs it again.
        set_state(&mut state, "Failed", "slept_during_check");
        return Ok(state);
    }
    remember_admission(root, card, &result);
    let runtime = root.path("/run/y2");
    let Some(_lock) = lock_within(&runtime.join("media.lock"), 30.0)? else {
        set_state(&mut state, "Failed", "media_operation_in_progress");
        return Ok(state);
    };
    let (mut now, mount) = sd_state(root);
    if mount.is_none()
        || !py_eq(get(&now, "mount_id"), get(&claim, "mount_id"))
        || get(&now, "read_only").as_str() != Some("checking")
        || !same_card(&candidates(root), &key)
    {
        set_state(&mut state, "Failed", "card_changed");
        return Ok(state);
    }
    if ok {
        let mut read_only = json!("protected");
        if exists(&root.path(PROTECT_OFF)) {
            command::run(root, &["mount", "-o", "remount,rw", SD], 10.0);
            let current = mounts(root);
            if current.first().is_some_and(|m| m.has_option("rw")) {
                read_only = Value::Null;
            }
        }
        set_state(&mut now, "Ready", Value::Null);
        set(&mut now, "read_only", read_only);
    } else {
        // Errors found, or still unfinished after CHECK_SECONDS: Repair is offered.
        let timed_out = get(&result, "findings") == &json!(["timeout"]);
        set_state(&mut now, "Degraded", "read_only");
        set(
            &mut now,
            "read_only",
            if timed_out {
                "check_timed_out"
            } else {
                "filesystem_errors"
            },
        );
    }
    set(&mut now, "filesystem_check", Value::Object(result));
    write_record(&root.path(MEDIA_MOUNT), &now)?;
    write_record(&runtime.join("media.json"), &now)?;
    state.extend(now);
    set(&mut state, "action", "verify");
    Ok(state)
}

// ---------------------------------------------------------------------------
// Repair, started only by the owner on the card the player marked (mounted
// read-only after a failed check, or refused). Android's vold repairs every
// card before mounting it; here the owner decides, because a repair may
// shorten or remove the damaged files.

const REPAIR_LAST: &str = "/data/system/platform/sd-repair-last.json";
const REPAIR_BACKUP: &str = "/data/system/platform/sd-repair";
const REPAIR_BACKUP_MAX: u64 = 256 << 20; // the tables of a 512 GB FAT32 card at 32 KiB clusters ≈ 128 MiB
const REPAIR_DATA_RESERVE: u64 = 64 << 20; // Y2DATA keeps this much free after the copy
const REPAIR_MINIMUM_PERCENT: i64 = 30;
const REPAIR_SECONDS: f64 = 1800.0; // never kill a repair half way unless it hangs

fn repairer(filesystem: &str) -> Option<&'static [&'static str]> {
    Some(match filesystem {
        "vfat" => &["fsck.fat", "-a", "-w"],
        "exfat" => &["fsck.exfat", "-p"],
        "ext4" => &["e2fsck", "-p"],
        _ => return None,
    })
}

/// The card the player marked: mounted read-only by us after its check, or refused and still
/// inserted. Returns (state, mount or None) or None.
fn marked_for_repair(root: &Root) -> Option<Object> {
    let old = mounts(root);
    let claim = object(root, MEDIA_MOUNT);
    if old.len() == 1
        && owned_mount(root, &old[0])
        && get_eq(&claim, "mount_id", &json!(old[0].mount_id))
        && get_in(
            &claim,
            "read_only",
            &["filesystem_errors", "check_timed_out"],
        )
    {
        return Some(claim);
    }
    if old.is_empty() && !refused_card(root).is_empty() {
        return Some(object(root, MEDIA_STATE));
    }
    None
}

fn battery_allows_repair(root: &Root) -> Result<bool, Failure> {
    let power = object(root, "/run/y2/power.json");
    if !get_eq(&power, "boot_id", &boot_id(root)) {
        return Ok(false);
    }
    let battery = match power.get("battery") {
        Some(Value::Object(battery)) => battery.clone(),
        Some(other) if truthy(other) => return Err(no_attribute(other, "get")), // AttributeError
        _ => Object::new(),
    };
    let percent = exact_int(battery.get("percent"));
    Ok(is_true(battery.get("charging"))
        || get_in(&battery, "status", &["Charging", "Full"])
        || percent.is_some_and(|p| p >= REPAIR_MINIMUM_PERCENT))
}

/// `error.strerror or str(error)`.
fn strerror(error: &io::Error) -> String {
    match error.raw_os_error() {
        // SAFETY: strerror returns a NUL-terminated message.
        Some(errno) => unsafe { CStr::from_ptr(libc::strerror(errno)) }
            .to_string_lossy()
            .into_owned(),
        None => error.to_string(),
    }
}

/// `Path.mkdir(mode=mode, parents=True, exist_ok=True)` with the OSError itself.
pub fn mkdir_io(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    if path.is_dir() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::DirBuilder::new().mode(mode).create(path) {
        Err(e) if !(e.kind() == io::ErrorKind::AlreadyExists && path.is_dir()) => Err(e),
        _ => Ok(()),
    }
}

/// `BufferedReader.read(n)`: up to `n` bytes, fewer only at the end.
fn read_up_to(file: &mut File, n: usize) -> io::Result<Vec<u8>> {
    let mut buffer = Vec::with_capacity(n);
    file.take(n as u64).read_to_end(&mut buffer)?;
    Ok(buffer)
}

/// Copy of the card's tables before the repair, for support: the newest one only. Directory
/// contents the repair changes are not in it.
fn save_tables(root: &Root, card: &Object) -> Value {
    let filesystem = card_text(card, "filesystem");
    let directory = root.path(REPAIR_BACKUP);
    let attempt = || -> io::Result<Value> {
        let mut device = File::open(root.path(card_text(card, "device")))?;
        let boot = read_up_to(&mut device, 512)?;
        let Some(size) = tables_bytes(&boot, filesystem) else {
            return Ok(json!({"saved": false, "reason": format!("no_tables_for_{filesystem}")}));
        };
        if size > REPAIR_BACKUP_MAX {
            return Ok(json!({"saved": false, "reason": "tables_too_large", "bytes": size}));
        }
        mkdir_io(&directory, 0o700)?;
        let space = statvfs(&directory)?;
        if available(&space) < size + REPAIR_DATA_RESERVE {
            return Ok(json!({"saved": false, "reason": "no_space", "bytes": size}));
        }
        for old in fs::read_dir(&directory)?.flatten() {
            if old.file_name().to_string_lossy().ends_with(".img") {
                fs::remove_file(old.path())?;
            }
        }
        let target = directory.join(format!("{}.img", prefix(card_id(card))));
        device.seek(SeekFrom::Start(0))?;
        let mut out = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&target)?;
        let mut left = size;
        while left > 0 {
            let chunk = read_up_to(&mut device, left.min(1 << 20) as usize)?;
            if chunk.is_empty() {
                return Err(io::Error::other("short_read"));
            }
            out.write_all(&chunk)?;
            left -= chunk.len() as u64;
        }
        out.sync_all()?;
        let relative = target
            .strip_prefix(root.path("/"))
            .unwrap_or(&target)
            .to_string_lossy()
            .into_owned();
        Ok(json!({"saved": true, "bytes": size, "file": relative}))
    };
    attempt().unwrap_or_else(|error| {
        let reason: String = strerror(&error).chars().take(40).collect();
        json!({"saved": false, "reason": format!("copy_failed:{reason}")})
    })
}

/// Owner command: repair the marked card, then admit it again.
pub fn repair(root: &Root) -> Result<Object, Failure> {
    let mut state = {
        let _lease = maintenance(root)?;
        let mut record = Object::new();
        let outcome = repair_locked(root, &mut record);
        if record.contains_key("card") {
            let _ = atomic_json(&root.path(REPAIR_LAST), &record, true);
        }
        outcome?
    };
    if state_is(&state, "Starting") {
        // Admission after the repair: a clean check mounts it read-write.
        state.extend(operation(root, "mount")?);
        set(&mut state, "action", "repair");
    }
    Ok(state)
}

fn repair_locked(root: &Root, record: &mut Object) -> Result<Object, Failure> {
    let runtime = runtime(root)?;
    let lock = try_lock(&runtime.join("media.lock"));
    set(record, "schema", 1);
    set(record, "time", wall_seconds());
    set(record, "boot_id", boot_id(root));
    let Some(_lock) = lock? else {
        let mut state = Object::new();
        set(&mut state, "schema", 1);
        set(&mut state, "action", "repair");
        set_state(&mut state, "Failed", "media_operation_in_progress");
        return Ok(state);
    };
    let mut state = begin(root, "repair");
    let Some(before) = marked_for_repair(root) else {
        set_state(&mut state, "Failed", "card_not_marked_for_repair");
        return Ok(state);
    };
    if !battery_allows_repair(root)? {
        set_state(&mut state, "Failed", "repair_needs_charger_or_battery");
        return Ok(state);
    }
    if !detach_for_change(root, &mut state)? {
        return Ok(state);
    }
    let available = candidates(root);
    // A non-dict identity or check crashes Python here, after the card was detached (and, for the
    // check, before the record has a card: no repair record is written).
    let identity = identity_id(&before)?;
    if available.len() != 1 || !py_eq(&json!(card_id(&available[0])), &identity) {
        set_state(&mut state, "Failed", "card_changed");
        return Ok(state);
    }
    let card = &available[0];
    let filesystem = card_text(card, "filesystem");
    let Some(repairer) = repairer(filesystem) else {
        set_state(&mut state, "Failed", "filesystem_not_repairable");
        return Ok(state);
    };
    let findings_before = match before.get("filesystem_check") {
        Some(Value::Object(check)) => get(check, "findings").clone(),
        Some(other) if truthy(other) => return Err(no_attribute(other, "get")),
        _ => Value::Null,
    };
    set(
        record,
        "card",
        prefix(identity.as_str().unwrap_or_default()),
    );
    set(record, "filesystem", filesystem);
    set(record, "findings_before", findings_before);
    set(record, "backup", save_tables(root, card));
    let started = Instant::now();
    let mut argv = repairer.to_vec();
    argv.push(card_text(card, "device"));
    let result = command::run_with(root, &argv, REPAIR_SECONDS, 1 << 20, true);
    set(record, "elapsed_ms", milliseconds(started));
    set(record, "ok", result.ok);
    set(record, "reason", json!(result.reason));
    set(record, "changed", repair_summary(result.output.as_deref()));
    let mut summary = Object::new();
    for key in ["ok", "reason", "changed", "backup", "elapsed_ms"] {
        summary.insert(key.into(), get(record, key).clone());
    }
    set(&mut state, "repair", Value::Object(summary));
    if result.reason.as_deref() == Some("command_timeout") {
        set_state(&mut state, "Failed", "repair_timed_out");
    }
    Ok(state)
}

fn apply_change(root: &Root, action: &str, confirm_cid: Option<&str>) -> Result<Object, Failure> {
    let runtime = runtime(root)?;
    let _lock = lock_now(&runtime.join("media.lock"))?;
    let mut state = begin(root, action);
    let disk = sd_disk(root);
    let cid = disk.as_ref().and_then(|d| read(&d.join("device/cid")));
    let sectors = disk.as_ref().and_then(|d| read(&d.join("size")));
    match (
        &disk,
        cid.as_deref().filter(|c| !c.is_empty()),
        sectors.as_deref(),
    ) {
        (Some(disk), Some(cid), Some(sectors)) if isdigit(sectors) => {
            let sectors: u64 = sectors.parse().unwrap_or(u64::MAX);
            if action == "format" && confirm_cid != Some(cid) {
                // The owner must name the card: a wrong or swapped card is never formatted.
                set_state(&mut state, "Failed", "cid_confirmation_mismatch");
                set(&mut state, "cid", cid);
            } else if sectors < MIN_CARD_SECTORS {
                set_state(&mut state, "Failed", "card_too_small");
            } else {
                let device = format!("/dev/{}", name(disk));
                set(&mut state, "cid", cid);
                set(&mut state, "sectors", sectors);
                set(&mut state, "device", device.clone());
                if detach_for_change(root, &mut state)? {
                    if action == "format" {
                        format_steps(root, &mut state, &device, sectors);
                    } else {
                        grow_steps(root, &mut state);
                    }
                }
            }
        }
        _ => set_state(&mut state, "Unavailable", "no_unique_card"),
    }
    if !state_is(&state, "Starting") {
        write_record(&runtime.join("media.json"), &state)?;
    }
    Ok(state)
}

fn errno(error: &io::Error) -> Value {
    json!(error.raw_os_error())
}

fn format_steps(root: &Root, state: &mut Object, device: &str, sectors: u64) {
    let Ok(card) = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_SYNC | libc::O_CLOEXEC)
        .open(root.path(device))
    else {
        set_state(state, "Failed", "card_not_writable");
        return;
    };
    // The old table, boot sector and a stale GPT backup must not survive.
    let zero = [0u8; 512];
    let wipe = || -> io::Result<()> {
        for sector in (1..PARTITION_START).chain(sectors - WIPE_TAIL_SECTORS..sectors) {
            card.write_at(&zero, sector * 512)?;
        }
        card.write_at(&mbr_for(sectors), 0)?;
        card.sync_all()
    };
    if let Err(error) = wipe() {
        set_state(state, "Failed", "card_write_failed");
        set(state, "errno", errno(&error));
        return;
    }
    // SAFETY: BLKRRPART on the card's own descriptor; no argument is read.
    if unsafe { libc::ioctl(card.as_raw_fd(), BLKRRPART as _, 0) } != 0 {
        let error = io::Error::last_os_error();
        set_state(state, "Failed", "partition_reread_failed");
        set(state, "errno", errno(&error));
        return;
    }
    drop(card);
    let mut argv = vec![
        "mkfs.ext4",
        "-F",
        "-q",
        "-L",
        "Y2SD",
        "-m",
        "0",
        "-E",
        "lazy_itable_init=1,lazy_journal_init=1",
    ];
    if sectors >= 32 * 1024 * 1024 {
        // 16 GiB and up: a 128 MiB journal is plenty
        argv.extend(["-J", "size=128"]);
    }
    let partition = format!("{device}p1");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !exists(&root.path(&partition)) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50)); // devtmpfs creates the node as the kernel re-reads the table
    }
    argv.push(&partition);
    let result = command::run(root, &argv, 180.0);
    if !result.ok {
        set_state(
            state,
            "Failed",
            format!("mkfs_failed:{}", py_str(&result.reason)),
        );
    }
}

/// `str(value)` of an optional reason (`'None'` when missing).
fn py_str(value: &Option<String>) -> String {
    value.clone().unwrap_or_else(|| "None".into())
}

fn grow_steps(root: &Root, state: &mut Object) {
    let available = candidates(root);
    if available.len() != 1 || card_text(&available[0], "filesystem") != "ext4" {
        set_state(state, "Failed", "grow_requires_one_ext4_card");
        return;
    }
    let device = card_text(&available[0], "device");
    // Only a clean (or safely auto-corrected) filesystem is resized.
    if !command::run(root, &["e2fsck", "-f", "-p", device], 300.0).ok {
        set_state(state, "Failed", "filesystem_check_failed");
        return;
    }
    let result = command::run(root, &["resize2fs", device], 300.0);
    if !result.ok {
        set_state(
            state,
            "Failed",
            format!("resize_failed:{}", py_str(&result.reason)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_partition_table_is_one_linux_partition() {
        let mbr = mbr_for(131_072);
        assert_eq!(mbr.len(), 512);
        assert_eq!(&mbr[510..], &[0x55, 0xaa]);
        assert_eq!(mbr[446 + 4], 0x83);
        assert_eq!(&mbr[454..462], &[0, 8, 0, 0, 0, 0xf8, 1, 0]);
        assert_eq!(&mbr_for(1 << 33)[458..462], &[0xff; 4]);
    }

    #[test]
    fn properties_parse_blkid_export() {
        let props = properties(Some(
            "DEVNAME=/dev/x\nUUID=A-B\nTYPE=vfat\nnoise\nTYPE=exfat",
        ));
        assert_eq!(props["TYPE"], "exfat");
        assert_eq!(props["UUID"], "A-B");
    }
}
