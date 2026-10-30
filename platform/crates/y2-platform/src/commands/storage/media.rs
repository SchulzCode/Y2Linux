//! The single explicit SD lifecycle, guarded by controller and filesystem identity
//! (`media mount|unmount|check|status|reconcile|verify|format|grow|repair|write`): here the cards the
//! controller offers, their admission and the insertion/removal work; the filesystem checks and
//! `verify` in `check`, write windows in `window`, `format`/`grow` in `change`, `repair` in `repair`.
// SPDX-License-Identifier: GPL-2.0-only
use super::observe::{media_identity, Identity, MEDIA_MOUNT};
use super::records::{Inserted, MediaRecord, Properties, ReadOnly};
use super::sys::{
    device_instance, emmc_entry, exists, is_symlink, lock_now, mkdir, name, read, release_sd,
    resolve, Mount,
};
use crate::reply::{atomic_json, unlink_missing_ok, Failure};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};
use y2p_core::{command, lenient::Shaped, mounts::mounts_at, Root};

mod change;
mod check;
mod repair;
mod window;

pub use change::change_card;
pub use check::verify;
use check::{fallback_filesystem, read_only_check, remember_admission, CHECK_SECONDS};
pub use repair::repair;
pub(crate) use repair::RepairRun;
use window::{protect, PROTECT_OFF};
pub use window::{window_expired, write_window, write_window_within};

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

/// One card the SD controller offers: a supported filesystem with a stable identity (its fields
/// start the operation record of the action that found it).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Card {
    pub device: String,
    pub filesystem: &'static str,
    pub uuid: Option<String>,
    pub cid: Option<String>,
    pub identity: Identity,
    pub source_instance: String,
}

impl Card {
    /// The identity hash (`identity.id`).
    fn id(&self) -> &str {
        &self.identity.id
    }

    /// The card's fields into the operation record.
    fn into_record(self, record: &mut MediaRecord) {
        record.device = Some(self.device);
        record.filesystem = Some(self.filesystem.into());
        record.uuid = self.uuid;
        record.cid = self.cid;
        record.identity = Some(Shaped::Is(self.identity));
        record.source_instance = Some(self.source_instance);
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
        if emmc_entry(&entry) || !resolve(&entry).to_string_lossy().contains(CONTROLLER) {
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
            cid: identity.cid.clone(),
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
    let claim = MediaRecord::claim(root);
    // Removed cards no longer have a sysfs controller path. The retained claim
    // permits only the exact mount we created, never an arbitrary mountpoint.
    claim.holds(root, mounted.mount_id)
        && claim.device_id.as_deref() == Some(mounted.device_id.as_str())
        && claim.device.as_deref() == Some(mounted.source.as_str())
}

/// SD presence by stable kernel identity, never by kernfs inode/ctime.
///
/// Page-cache drops and ordinary memory pressure reclaim kernfs inodes, so a
/// recreated inode is not a new card. dev_t and the disk sequence number stay
/// fixed for one gendisk; every re-enumeration allocates a new diskseq, which
/// also distinguishes a reinserted identical card (same CID).
pub(crate) fn inventory(root: &Root) -> Vec<Inserted> {
    let mut result = Vec::new();
    for entry in root.glob("/sys/class/block/mmcblk*") {
        if emmc_entry(&entry) {
            continue;
        }
        let device = resolve(&entry);
        if device.to_string_lossy().contains(CONTROLLER) {
            let disk = if read(&entry.join("partition")).is_some() {
                device.parent().map(Path::to_path_buf).unwrap_or(device)
            } else {
                device
            };
            result.push((
                name(&entry),
                read(&entry.join("dev")),
                read(&disk.join("diskseq")),
                read(&disk.join("device/cid")),
            ));
        }
    }
    result
}

/// media-lifecycle.json: what reconcile last did.
pub(crate) fn lifecycle(root: &Root) -> MediaRecord {
    y2p_core::lenient::read(root.json_object("/run/y2/media-lifecycle.json"))
}

/// Whether reconcile returns its last record as it is: the same cards, and no retry due.
pub(crate) fn settled(previous: &MediaRecord, current: &[Inserted]) -> bool {
    previous.inventory.as_deref() == Some(current) && !previous.retry.unwrap_or(false)
}

/// Insertion/removal work; no automatic retry storm or remount after eject.
pub fn reconcile(root: &Root) -> Result<MediaRecord, Failure> {
    let runtime = runtime(root)?;
    let current = inventory(root);
    let previous = lifecycle(root);
    let same = previous.inventory.as_ref() == Some(&current);
    if settled(&previous, &current) {
        protect(root)?;
        return Ok(previous);
    }
    let attempts = if same {
        previous.attempts.unwrap_or(0)
    } else {
        0
    };
    let empty = current.is_empty();
    let mut result = MediaRecord {
        schema: Some(1),
        inventory: Some(current),
        attempts: Some(attempts + 1),
        retry: Some(false),
        boot_id: Some(root.boot_id().into()),
        ..MediaRecord::default()
    };
    let lifecycle = runtime.join("media-lifecycle.json");
    if !mounts_at(root, SD).is_empty() {
        result.absorb(operation(root, "unmount")?);
        if !mounts_at(root, SD).is_empty() {
            result.retry = Some(attempts < 4);
            result.reason = Some("source_change_unmount_failed".into());
            atomic_json(&lifecycle, &result, false)?;
            return Ok(result);
        }
    }
    if !empty {
        result.absorb(operation(root, "mount")?);
    } else {
        result.set_state("Unavailable", Some("no_card"));
    }
    atomic_json(&lifecycle, &result, false)?;
    Ok(result)
}

/// `media status`'s answer: the card as the status reports it.
pub fn status(root: &Root) -> Result<crate::commands::observation::snapshot::Volume, Failure> {
    crate::commands::observation::snapshot::volume_at(root, SD)
}

pub fn operation(root: &Root, action: &str) -> Result<MediaRecord, Failure> {
    let runtime = runtime(root)?;
    let _lock = lock_now(&runtime.join("media.lock"))?;
    let record = runtime.join("media.json");
    let mut state = MediaRecord::begin(root, action);
    let old = mounts_at(root, SD);
    if !old.is_empty() && old.iter().any(|m| !owned_mount(root, m)) {
        state.set_state("Failed", Some("mounted_source_not_sd"));
        atomic_json(&record, &state, false)?;
        return Ok(state);
    }
    if action == "unmount" {
        if old.len() > 1 {
            state.set_state("Failed", Some("ambiguous_mount_stack"));
        } else if let Some(mounted) = old.first() {
            if !release_sd(root, mounted.mount_id, false)? {
                state.set_state("Failed", Some("usb_transfer_connected_or_busy"));
                atomic_json(&record, &state, false)?;
                return Ok(state);
            }
            let result = command::run(root, &["umount", SD], 3.0);
            if !result.ok || !mounts_at(root, SD).is_empty() {
                let reason = result.reason.as_deref().unwrap_or("unmount_busy");
                state.set_state("Failed", Some(reason));
            } else {
                state.set_state("Unavailable", Some("ejected"));
            }
        } else {
            state.set_state("Unavailable", Some("not_mounted"));
        }
    } else if !old.is_empty() {
        // Never check/repair an active filesystem or stack a new card on it.
        state.set_state(
            "Failed",
            Some(if action == "check" {
                "already_mounted"
            } else {
                "mount_requires_unmounted_path"
            }),
        );
    } else {
        let available = candidates(root);
        if let [card] = &available[..] {
            card.clone().into_record(&mut state);
            if action == "check" {
                let check = read_only_check(root, card, true, Some(CHECK_SECONDS));
                if check.ok {
                    state.set_state("Ready", Some("read_only_check_passed"));
                } else {
                    state.set_state("Degraded", check.reason.as_deref());
                }
                state.filesystem_check = Some(Shaped::Is(check));
            } else if admit(root, &mut state, card, &available, &record)? {
                return Ok(state);
            }
        } else {
            state.set_state("Unavailable", Some("no_unique_supported_filesystem"));
        }
    }
    atomic_json(&record, &state, false)?;
    Ok(state)
}

/// The mount branch of `operation` after the unique card was found; true when it returns early
/// (its record already written).
fn admit(
    root: &Root,
    state: &mut MediaRecord,
    card: &Card,
    available: &[Card],
    record: &Path,
) -> Result<bool, Failure> {
    let filesystem = card.filesystem;
    let device = card.device.as_str();
    // Read-only admission never authorizes checker repairs.
    // Large/unhealthy media may time out;
    // the owner must see that failure rather than a RW mount.
    let check = read_only_check(root, card, false, None);
    remember_admission(root, card, &check);
    let (timed_out, passed) = (check.timed_out(), check.ok);
    state.filesystem_check = Some(Shaped::Is(check));
    let mut read_only = ReadOnly::ReadWrite;
    if timed_out && fallback_filesystem(filesystem) {
        // Not finished is not damaged (F49): it plays read-only
        // now and `verify` finishes the check in the background.
        read_only = ReadOnly::Checking;
    } else if !passed {
        state.set_state("Degraded", Some("filesystem_check_failed"));
        // FAT/exFAT with a finding mounts read-only: reading never changes a card,
        // writing into a damaged table can overwrite another file.
        // The music plays; Repair (or a computer) makes it writable.
        // ext4 stays out: even a read-only mount replays its journal.
        if !fallback_filesystem(filesystem) {
            atomic_json(record, state, false)?;
            return Ok(true);
        }
        read_only = ReadOnly::FilesystemErrors;
    }
    if candidates(root) != available {
        state.set_state("Failed", Some("source_changed_during_check"));
        atomic_json(record, state, false)?;
        return Ok(true);
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
            state.set_state(
                if damaged { "Degraded" } else { "Ready" },
                damaged.then_some("read_only"),
            );
            state.mount_id = Some(mounted.mount_id);
            state.device_id = Some(mounted.device_id.clone());
            state.set_read_only(read_only);
            state.source_instance = device_instance(root, &mounted.device_id);
            if candidates(root) != available {
                state.set_state("Failed", Some("source_changed_during_mount"));
            } else {
                atomic_json(&root.path(MEDIA_MOUNT), state, false)?;
                // The kernel recycles mount ids, so a release marker for a card that was
                // ejected or reformatted must not hide the next mount that reuses its id.
                unlink_missing_ok(&root.path("/run/y2/usb-release-sd.json"))?;
            }
        }
        _ => state.set_state(
            "Failed",
            Some(result.reason.as_deref().unwrap_or("mount_identity_failed")),
        ),
    }
    Ok(false)
}

#[cfg(test)]
#[path = "media_tests.rs"]
mod media_tests;
