//! `media.py`: the single explicit SD lifecycle, guarded by controller and filesystem identity
//! (`media mount|unmount|check|status|reconcile|verify|format|grow|repair|write`).
// SPDX-License-Identifier: GPL-2.0-only
use super::observe::{media_identity, refused_card, volumes, MEDIA_MOUNT, MEDIA_STATE};
use super::records::{Claim, Properties, ReadOnly};
use super::sys::{
    device_instance, exists, get, get_eq, is_symlink, isdigit, lock_now, lock_within, maintenance,
    mkdir, name, no_attribute, object, read, release_sd, resolve, statvfs, to_object, try_lock,
    wall_seconds, Mount, Object,
};
use crate::reply::{atomic_json, unlink_missing_ok, Failure};
use serde::Serialize;
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
use y2p_core::mounts::mounts_at;
use y2p_core::py::PyValue;
use y2p_core::{
    command,
    py::{exact_int, is_true, number, truthy},
    storage_media::{
        fat_benign_only, fat_findings, fat_other_shapes, repair_summary, tables_bytes,
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

/// What a filesystem check found (`filesystem_check` of the records). The checker's output is kept
/// only from a run that passed: a failed one can name files.
#[derive(Clone, Debug, Serialize)]
struct Check {
    ok: bool,
    reason: Option<String>,
    output: Option<String>,
    elapsed_ms: u64,
    /// The classes of what it reported: `timeout`, the FAT line classes, or `other`.
    #[serde(skip_serializing_if = "Option::is_none")]
    findings: Option<Vec<String>>,
    /// What FAT's unclassified lines said, without names.
    #[serde(skip_serializing_if = "Option::is_none")]
    other: Option<Value>,
    /// Why a FAT check that failed passed anyway: `unclean_unmount` (#44) or `benign`.
    #[serde(skip_serializing_if = "Option::is_none")]
    finding: Option<&'static str>,
    /// Run by `verify` after the card was admitted read-only.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    background: bool,
}

impl Check {
    /// The checker ran out of time; that is not damage (F49).
    fn timed_out(&self) -> bool {
        self.findings.as_ref().is_some_and(|f| f == &["timeout"])
    }

    fn value(&self) -> Value {
        Value::Object(to_object(self))
    }
}

/// The admission check, never a repair. Benign findings pass with `finding`, and every run names
/// its classes in `findings`.
fn read_only_check(root: &Root, card: &Card, force: bool, timeout: Option<f64>) -> Check {
    let filesystem = card.filesystem;
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
    argv.push(&card.device);
    let started = Instant::now();
    let result = command::run_with(root, &argv, timeout, command::LIMIT, filesystem == "vfat");
    let mut check = Check {
        ok: result.ok,
        reason: result.reason.clone(),
        output: if result.ok {
            result.output.clone()
        } else {
            None
        },
        elapsed_ms: milliseconds(started),
        findings: None,
        other: None,
        finding: None,
        background: false,
    };
    let output = result.output.as_deref();
    if result.reason.as_deref() == Some("command_timeout") {
        check.findings = Some(vec!["timeout".into()]);
    } else if !result.ok
        && result.reason.as_deref() == Some("command_failed")
        && filesystem == "vfat"
    {
        let findings = fat_findings(output);
        if findings.iter().any(|f| f == "other") {
            check.other = Some(fat_other_shapes(output, 5));
        }
        let only_dirty = findings == ["dirty"];
        check.findings = Some(findings);
        if fat_benign_only(output) {
            // 'unclean_unmount' is the #44 name for the dirty bit alone.
            check.ok = true;
            check.finding = Some(if only_dirty {
                "unclean_unmount"
            } else {
                "benign"
            });
        }
    } else if !result.ok {
        check.findings = Some(vec!["other".into()]);
    }
    check
}

/// One entry of sd-admissions.json.
#[derive(Serialize)]
struct Admission<'a> {
    time: i64,
    card: String,
    filesystem: &'a str,
    ok: bool,
    reason: &'a Option<String>,
    finding: Option<&'static str>,
    findings: &'a [String],
    elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    other: Option<&'a Value>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    background: bool,
}

fn remember_admission(root: &Root, card: &Card, check: &Check) {
    let old = object(root, ADMISSIONS);
    let mut entries: Vec<Value> = match old.get("admissions") {
        None => Vec::new(),
        Some(Value::Array(items)) => items.iter().filter(|e| e.is_object()).cloned().collect(),
        // A text or an object holds no entries; a number or a flag is no list at all: the record
        // is left as it is rather than replaced.
        Some(Value::String(_)) | Some(Value::Object(_)) => Vec::new(),
        Some(_) => return,
    };
    let entry = Admission {
        time: wall_seconds(),
        card: prefix(card.id()),
        filesystem: card.filesystem,
        ok: check.ok,
        reason: &check.reason,
        finding: check.finding,
        findings: check.findings.as_deref().unwrap_or_default(),
        elapsed_ms: check.elapsed_ms,
        other: check.other.as_ref().filter(|o| truthy(o)),
        background: check.background,
    };
    entries.push(Value::Object(to_object(&entry)));
    let path = root.path(ADMISSIONS);
    let Some(directory) = path.parent() else {
        return;
    };
    if mkdir(directory, 0o700).is_err() {
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
        return Ok(volumes(root).swap_remove(2));
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
    let mut found = root
        .glob("/sys/class/block/mmcblk*")
        .into_iter()
        .filter(|entry| {
            resolve(entry).to_string_lossy().contains(CONTROLLER)
                && read(&entry.join("partition")).is_none()
        });
    let disk = found.next()?;
    found.next().is_none().then_some(disk)
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
    let old = mounts_at(root, SD);
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
    if !result.ok || !mounts_at(root, SD).is_empty() {
        set_state(
            state,
            "Failed",
            result.reason.unwrap_or_else(|| "unmount_busy".into()),
        );
        return Ok(false);
    }
    Ok(true)
}

/// How format has the kernel re-read the partition table it wrote (a test's card is a plain file).
pub(crate) type Reread<'a> = &'a dyn Fn(&File) -> io::Result<()>;

/// BLKRRPART on the card's own descriptor.
fn blkrrpart(card: &File) -> io::Result<()> {
    // SAFETY: BLKRRPART on an open descriptor; no argument is read.
    if unsafe { libc::ioctl(card.as_raw_fd(), BLKRRPART as _, 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// action: 'format' (new table + ext4 over the whole card) or 'grow' (ext4 to the partition).
pub fn change_card(
    root: &Root,
    action: &str,
    confirm_cid: Option<&str>,
) -> Result<Object, Failure> {
    change_card_with(root, action, confirm_cid, &blkrrpart)
}

/// `change_card` with the partition table re-read by `reread`.
pub(crate) fn change_card_with(
    root: &Root,
    action: &str,
    confirm_cid: Option<&str>,
    reread: Reread,
) -> Result<Object, Failure> {
    if action != "format" && action != "grow" {
        return Err(Failure::value("unknown_media_change"));
    }
    let mut state = apply_change(root, action, confirm_cid, reread)?;
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

/// sd-write.json: the open write windows of one mount, holder → monotonic end time.
#[derive(Serialize)]
struct Windows<'a> {
    schema: u8,
    boot_id: Option<String>,
    mount_id: i64,
    holders: &'a Object,
}

/// (claim, mount) of our current SD mount, else (claim, None).
fn sd_state(root: &Root) -> (Claim, Option<Mount>) {
    let claim = Claim::load(root);
    let mut old = mounts_at(root, SD);
    if old.len() == 1 && owned_mount(root, &old[0]) && claim.holds(root, old[0].mount_id) {
        return (claim, old.pop());
    }
    (claim, None)
}

/// The live windows of `mount` (a holder whose end time has passed is left out).
fn writers(root: &Root, mount: &Mount) -> Result<Object, Failure> {
    let value = object(root, SD_WRITE);
    if !get_eq(&value, "boot_id", &Value::from(root.boot_id()))
        || !get_eq(&value, "mount_id", &json!(mount.mount_id))
    {
        return Ok(Object::new());
    }
    let holders = match value.get("holders") {
        Some(Value::Object(holders)) => holders.clone(),
        Some(other) if truthy(other) => return Err(no_attribute(other, "items")),
        _ => Object::new(),
    };
    let now = root.clock().monotonic();
    Ok(holders
        .into_iter()
        .filter(|(k, v)| {
            WRITE_HOLDERS.contains(&k.as_str()) && number(Some(v)).is_some_and(|v| v > now)
        })
        .collect())
}

/// Remount to match the holders; records them. Returns writable now.
fn apply_window(root: &Root, mount: &Mount, holders: &Object) -> Result<bool, Failure> {
    let windows = Windows {
        schema: 1,
        boot_id: root.boot_id(),
        mount_id: mount.mount_id,
        holders,
    };
    atomic_json(&root.path(SD_WRITE), &windows, false)?;
    let mut writable = mount.has_option("rw");
    if !holders.is_empty() != writable {
        let mode = if holders.is_empty() { "ro" } else { "rw" };
        command::run(root, &["mount", "-o", &format!("remount,{mode}"), SD], 10.0);
        let now = mounts_at(root, SD);
        writable = now.first().is_some_and(|m| m.has_option("rw"));
    }
    Ok(writable)
}

/// What `write` prints (with `holders` once a window changed).
#[derive(Serialize)]
struct WindowAnswer {
    state: &'static str,
    reason: Option<&'static str>,
    writable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    holders: Option<Vec<String>>,
}

/// An answer before any window changed.
fn answer(state: &'static str, reason: Option<&'static str>, writable: bool) -> Object {
    to_object(&WindowAnswer {
        state,
        reason,
        writable,
        holders: None,
    })
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
    match claim.read_only() {
        // Writable once its background check passes (USB asks again every 10 s).
        Some(ReadOnly::Checking) => return Ok(answer("Failed", Some("card_being_checked"), false)),
        Some(ReadOnly::ReadWrite) => return Ok(answer("Ready", None, mount.has_option("rw"))),
        Some(ReadOnly::Protected) => {}
        // Errors found or a check unfinished: Repair first, never a window.
        _ => return Ok(answer("Degraded", Some("card_has_errors"), false)),
    }
    let mut holders = writers(root, &mount)?;
    if release {
        holders.shift_remove(holder);
    } else {
        let hold = seconds.clamp(1, WRITE_HOLD_MAX);
        holders.insert(holder.into(), json!(root.clock().monotonic() + hold as f64));
    }
    let writable = apply_window(root, &mount, &holders)?;
    let ok = writable == !holders.is_empty() || release;
    let mut names: Vec<String> = holders.keys().cloned().collect();
    names.sort();
    Ok(to_object(&WindowAnswer {
        state: if ok { "Ready" } else { "Failed" },
        reason: (!ok).then_some("remount_failed"),
        writable,
        holders: Some(names),
    }))
}

/// Whether `protect` has work: a protected card still mounted read-write with no live window. The
/// service asks before it starts `media reconcile` (file reads only, no subprocess).
pub fn window_expired(root: &Root) -> bool {
    match sd_state(root) {
        (claim, Some(m))
            if claim.read_only() == Some(ReadOnly::Protected) && m.has_option("rw") =>
        {
            // A holders record that is not an object fails reconcile too: never a restart per tick.
            writers(root, &m).is_ok_and(|holders| holders.is_empty())
        }
        _ => false,
    }
}

/// Reconcile tick: a protected card with no live window goes read-only.
fn protect(root: &Root) -> Result<(), Failure> {
    match sd_state(root) {
        (claim, Some(m))
            if claim.read_only() == Some(ReadOnly::Protected) && m.has_option("rw") => {}
        _ => return Ok(()),
    }
    // A writer or an operation holds the card; the next tick tries again.
    if let Some(_lock) = try_lock(&root.path("/run/y2/media.lock"))? {
        if let (claim, Some(mount)) = sd_state(root) {
            if claim.read_only() == Some(ReadOnly::Protected) {
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
fn slept_ns(root: &Root) -> i128 {
    let clock = root.clock();
    i128::from(clock.boottime_ns()) - i128::from(clock.monotonic_ns())
}

/// `(record.get('identity') or {}).get('id')`; a truthy identity that is not an object cannot be
/// read.
fn identity_id(identity: &Value) -> Result<Value, Failure> {
    match identity {
        Value::Object(identity) => Ok(get(identity, "id").clone()),
        other if truthy(other) => Err(no_attribute(other, "get")),
        _ => Ok(Value::Null),
    }
}

/// The one candidate is the card a record names by identity id and block instance.
fn same_card(cards: &[Card], id: &Value, instance: &Value) -> bool {
    match cards {
        [card] => json!(card.id()).py_eq(id) && json!(card.source_instance).py_eq(instance),
        _ => false,
    }
}

pub fn verify(root: &Root) -> Result<Object, Failure> {
    let mut state = Object::new();
    set(&mut state, "schema", 1);
    set(&mut state, "action", "verify");
    set_state(&mut state, "Starting", Value::Null);
    let (claim, mount) = sd_state(root);
    if mount.is_none() || claim.read_only() != Some(ReadOnly::Checking) {
        set_state(&mut state, "Unavailable", "nothing_to_check");
        return Ok(state);
    }
    let id = identity_id(claim.get("identity"))?;
    let instance = claim.get("source_instance");
    let available = candidates(root);
    if !same_card(&available, &id, instance) {
        set_state(&mut state, "Failed", "card_changed");
        return Ok(state);
    }
    let card = &available[0];
    let slept = slept_ns(root);
    let mut check = read_only_check(root, card, false, Some(CHECK_SECONDS));
    check.background = true;
    if !check.ok && slept_ns(root) - slept > 1_000_000_000 {
        // A check read across a system sleep is no verdict; the service runs it again.
        set_state(&mut state, "Failed", "slept_during_check");
        return Ok(state);
    }
    remember_admission(root, card, &check);
    let runtime = root.path("/run/y2");
    let Some(_lock) = lock_within(&runtime.join("media.lock"), 30.0)? else {
        set_state(&mut state, "Failed", "media_operation_in_progress");
        return Ok(state);
    };
    let (now, mount) = sd_state(root);
    if mount.is_none()
        || !now.get("mount_id").py_eq(claim.get("mount_id"))
        || now.read_only() != Some(ReadOnly::Checking)
        || !same_card(&candidates(root), &id, instance)
    {
        set_state(&mut state, "Failed", "card_changed");
        return Ok(state);
    }
    let mut now = now.into_object();
    if check.ok {
        let mut read_only = ReadOnly::Protected;
        if exists(&root.path(PROTECT_OFF)) {
            command::run(root, &["mount", "-o", "remount,rw", SD], 10.0);
            let current = mounts_at(root, SD);
            if current.first().is_some_and(|m| m.has_option("rw")) {
                read_only = ReadOnly::ReadWrite;
            }
        }
        set_state(&mut now, "Ready", Value::Null);
        set(&mut now, "read_only", read_only.value());
    } else {
        // Errors found, or still unfinished after CHECK_SECONDS: Repair is offered.
        let read_only = if check.timed_out() {
            ReadOnly::CheckTimedOut
        } else {
            ReadOnly::FilesystemErrors
        };
        set_state(&mut now, "Degraded", "read_only");
        set(&mut now, "read_only", read_only.value());
    }
    set(&mut now, "filesystem_check", check.value());
    atomic_json(&root.path(MEDIA_MOUNT), &now, false)?;
    atomic_json(&runtime.join("media.json"), &now, false)?;
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

/// The card the player marked: mounted read-only by us after its check (the claim), or refused
/// and still inserted (the last operation record).
fn marked_for_repair(root: &Root) -> Option<Object> {
    let old = mounts_at(root, SD);
    let claim = Claim::load(root);
    if old.len() == 1
        && owned_mount(root, &old[0])
        && claim.get("mount_id").py_eq(&json!(old[0].mount_id))
        && ReadOnly::damaged(claim.read_only())
    {
        return Some(claim.into_object());
    }
    if old.is_empty() && !refused_card(root).is_empty() {
        return Some(object(root, MEDIA_STATE));
    }
    None
}

fn battery_allows_repair(root: &Root) -> Result<bool, Failure> {
    let power = object(root, "/run/y2/power.json");
    if !get_eq(&power, "boot_id", &Value::from(root.boot_id())) {
        return Ok(false);
    }
    let battery = match power.get("battery") {
        Some(Value::Object(battery)) => battery.clone(),
        Some(other) if truthy(other) => return Err(no_attribute(other, "get")),
        _ => Object::new(),
    };
    let percent = exact_int(battery.get("percent"));
    Ok(is_true(battery.get("charging"))
        || get(&battery, "status")
            .as_str()
            .is_some_and(|s| ["Charging", "Full"].contains(&s))
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

/// The copy of a card's tables taken before its repair (`backup` of the repair record).
#[derive(Clone, Debug, Serialize)]
struct Backup {
    saved: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes: Option<u64>,
    /// Below the root, relative.
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
}

impl Backup {
    fn skipped(reason: impl Into<String>, bytes: Option<u64>) -> Backup {
        Backup {
            saved: false,
            reason: Some(reason.into()),
            bytes,
            file: None,
        }
    }
}

/// Copy of the card's tables before the repair, for support: the newest one only. Directory
/// contents the repair changes are not in it.
fn save_tables(root: &Root, card: &Card) -> Backup {
    let filesystem = card.filesystem;
    let directory = root.path(REPAIR_BACKUP);
    let attempt = || -> io::Result<Backup> {
        let mut device = File::open(root.path(&card.device))?;
        let boot = read_up_to(&mut device, 512)?;
        let Some(size) = tables_bytes(&boot, filesystem) else {
            return Ok(Backup::skipped(format!("no_tables_for_{filesystem}"), None));
        };
        if size > REPAIR_BACKUP_MAX {
            return Ok(Backup::skipped("tables_too_large", Some(size)));
        }
        mkdir_io(&directory, 0o700)?;
        let space = statvfs(&directory)?;
        if space.available() < size + REPAIR_DATA_RESERVE {
            return Ok(Backup::skipped("no_space", Some(size)));
        }
        for old in fs::read_dir(&directory)?.flatten() {
            if old.file_name().to_string_lossy().ends_with(".img") {
                fs::remove_file(old.path())?;
            }
        }
        let target = directory.join(format!("{}.img", prefix(card.id())));
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
        Ok(Backup {
            saved: true,
            reason: None,
            bytes: Some(size),
            file: Some(relative),
        })
    };
    attempt().unwrap_or_else(|error| {
        let reason: String = strerror(&error).chars().take(40).collect();
        Backup::skipped(format!("copy_failed:{reason}"), None)
    })
}

/// One repair run (`repair` of the printed record, and part of sd-repair-last.json).
#[derive(Clone, Debug, Serialize)]
struct RepairRun {
    ok: bool,
    reason: Option<String>,
    changed: Value,
    backup: Backup,
    elapsed_ms: u64,
}

/// sd-repair-last.json: the newest repair, for support; card prefix and counts, never names.
#[derive(Serialize)]
struct RepairRecord {
    schema: u8,
    time: i64,
    boot_id: Option<String>,
    card: String,
    filesystem: &'static str,
    findings_before: Value,
    #[serde(flatten)]
    run: RepairRun,
}

/// Owner command: repair the marked card, then admit it again.
pub fn repair(root: &Root) -> Result<Object, Failure> {
    let mut state = {
        let _lease = maintenance(root)?;
        let (state, record) = repair_locked(root)?;
        if let Some(record) = record {
            let _ = atomic_json(&root.path(REPAIR_LAST), &record, true);
        }
        state
    };
    if state_is(&state, "Starting") {
        // Admission after the repair: a clean check mounts it read-write.
        state.extend(operation(root, "mount")?);
        set(&mut state, "action", "repair");
    }
    Ok(state)
}

/// The repair under the media lock: its state, and its record once the repairer ran.
fn repair_locked(root: &Root) -> Result<(Object, Option<RepairRecord>), Failure> {
    let runtime = runtime(root)?;
    let lock = try_lock(&runtime.join("media.lock"));
    let (time, boot_id) = (wall_seconds(), root.boot_id());
    let Some(_lock) = lock? else {
        let mut state = Object::new();
        set(&mut state, "schema", 1);
        set(&mut state, "action", "repair");
        set_state(&mut state, "Failed", "media_operation_in_progress");
        return Ok((state, None));
    };
    let mut state = begin(root, "repair");
    let Some(before) = marked_for_repair(root) else {
        set_state(&mut state, "Failed", "card_not_marked_for_repair");
        return Ok((state, None));
    };
    if !battery_allows_repair(root)? {
        set_state(&mut state, "Failed", "repair_needs_charger_or_battery");
        return Ok((state, None));
    }
    if !detach_for_change(root, &mut state)? {
        return Ok((state, None));
    }
    let available = candidates(root);
    // An identity or check that cannot be read stops here, after the card was detached and
    // before a repair record exists.
    let identity = identity_id(get(&before, "identity"))?;
    let card = match &available[..] {
        [card] if json!(card.id()).py_eq(&identity) => card,
        _ => {
            set_state(&mut state, "Failed", "card_changed");
            return Ok((state, None));
        }
    };
    let Some(repairer) = repairer(card.filesystem) else {
        set_state(&mut state, "Failed", "filesystem_not_repairable");
        return Ok((state, None));
    };
    let findings_before = match before.get("filesystem_check") {
        Some(Value::Object(check)) => get(check, "findings").clone(),
        Some(other) if truthy(other) => return Err(no_attribute(other, "get")),
        _ => Value::Null,
    };
    let backup = save_tables(root, card);
    let started = Instant::now();
    let mut argv = repairer.to_vec();
    argv.push(&card.device);
    let result = command::run_with(root, &argv, REPAIR_SECONDS, 1 << 20, true);
    let run = RepairRun {
        ok: result.ok,
        reason: result.reason.clone(),
        changed: repair_summary(result.output.as_deref()),
        backup,
        elapsed_ms: milliseconds(started),
    };
    set(&mut state, "repair", Value::Object(to_object(&run)));
    if result.reason.as_deref() == Some("command_timeout") {
        set_state(&mut state, "Failed", "repair_timed_out");
    }
    let record = RepairRecord {
        schema: 1,
        time,
        boot_id,
        card: prefix(identity.as_str().unwrap_or_default()),
        filesystem: card.filesystem,
        findings_before,
        run,
    };
    Ok((state, Some(record)))
}

fn apply_change(
    root: &Root,
    action: &str,
    confirm_cid: Option<&str>,
    reread: Reread,
) -> Result<Object, Failure> {
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
                        format_steps(root, &mut state, &device, sectors, reread);
                    } else {
                        grow_steps(root, &mut state);
                    }
                }
            }
        }
        _ => set_state(&mut state, "Unavailable", "no_unique_card"),
    }
    if !state_is(&state, "Starting") {
        atomic_json(&runtime.join("media.json"), &state, false)?;
    }
    Ok(state)
}

fn format_steps(root: &Root, state: &mut Object, device: &str, sectors: u64, reread: Reread) {
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
        set(state, "errno", error.raw_os_error());
        return;
    }
    if let Err(error) = reread(&card) {
        set_state(state, "Failed", "partition_reread_failed");
        set(state, "errno", error.raw_os_error());
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
        let reason = result.reason.as_deref().unwrap_or("None");
        set_state(state, "Failed", format!("mkfs_failed:{reason}"));
    }
}

fn grow_steps(root: &Root, state: &mut Object) {
    let available = candidates(root);
    let [card] = &available[..] else {
        set_state(state, "Failed", "grow_requires_one_ext4_card");
        return;
    };
    if card.filesystem != "ext4" {
        set_state(state, "Failed", "grow_requires_one_ext4_card");
        return;
    }
    let device = card.device.as_str();
    // Only a clean (or safely auto-corrected) filesystem is resized.
    if !command::run(root, &["e2fsck", "-f", "-p", device], 300.0).ok {
        set_state(state, "Failed", "filesystem_check_failed");
        return;
    }
    let result = command::run(root, &["resize2fs", device], 300.0);
    if !result.ok {
        let reason = result.reason.as_deref().unwrap_or("None");
        set_state(state, "Failed", format!("resize_failed:{reason}"));
    }
}

#[cfg(test)]
#[path = "media_tests.rs"]
mod media_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Fixture;

    const NOW: f64 = 1000.0;

    /// A protected card mounted by this boot (mount 51), `rw` or `ro`, with these holders'
    /// deadlines, at monotonic time NOW.
    fn protected(options: &str, holders: Value) -> Fixture {
        let clock = std::sync::Arc::new(y2p_core::clock::Manual::at(
            1_000_000_000_000,
            1_000_000_000_000,
        ));
        let fixture = Fixture::new().with_clock(clock);
        fixture
            .put("/proc/sys/kernel/random/boot_id", "boot-a\n")
            .put(
                "/proc/self/mountinfo",
                &format!(
                    "51 22 179:97 / /media/sd {options},nosuid,nodev,noexec,noatime - vfat \
                     /dev/mmcblk1p1 {options}\n"
                ),
            )
            .put_json(
                MEDIA_MOUNT,
                &json!({"boot_id": "boot-a", "mount_id": 51, "device_id": "179:97",
                        "device": "/dev/mmcblk1p1", "read_only": "protected"}),
            )
            .put_json(
                SD_WRITE,
                &json!({"schema": 1, "boot_id": "boot-a", "mount_id": 51, "holders": holders}),
            );
        fixture
    }

    #[test]
    fn an_expired_write_window_asks_for_a_reconcile() {
        let now = NOW;
        assert!(window_expired(
            &protected("rw", json!({"reborn": now - 1.0})).root
        ));
        assert!(window_expired(&protected("rw", json!({})).root));
        // A live window, or a card already read-only: nothing to do.
        assert!(!window_expired(
            &protected("rw", json!({"usb": now + 60.0})).root
        ));
        assert!(!window_expired(
            &protected("ro", json!({"reborn": now - 1.0})).root
        ));
        // A record the reconcile fails on never restarts it every tick.
        assert!(!window_expired(&protected("rw", json!(["reborn"])).root));
    }

    #[test]
    fn the_partition_table_is_one_linux_partition() {
        let mbr = mbr_for(131_072);
        assert_eq!(mbr.len(), 512);
        assert_eq!(&mbr[510..], &[0x55, 0xaa]);
        assert_eq!(mbr[446 + 4], 0x83);
        assert_eq!(&mbr[454..462], &[0, 8, 0, 0, 0, 0xf8, 1, 0]);
        assert_eq!(&mbr_for(1 << 33)[458..462], &[0xff; 4]);
    }
}
