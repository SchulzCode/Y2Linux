//! The SD card lent to a computer as a USB drive (owner 2026-11-26, docs/planning/SD-DISK-MODE-PLAN.md): the
//! owner's switch, the lend record and the steps the USB service takes. While the record exists nothing on
//! the player touches the card: the computer owns its blocks.
// SPDX-License-Identifier: GPL-2.0-only
use super::{lifecycle, window::sd_state, SD};
use crate::commands::storage::records::OnPlayer;
use crate::commands::storage::sys::try_lock;
use crate::reply::{atomic_json, unlink_missing_ok, Failure};
use serde::{Deserialize, Serialize};
use y2p_core::{command, mounts::mounts_at, Root};

/// The owner's switch (Settings → Storage → SD Card as a USB Stick); missing or unreadable is off.
pub const SETTING: &str = "/data/system/platform/usb-sd-disk.json";
/// The lend record: written before the card leaves the player, removed once it is back.
const LENT: &str = "/run/y2/sd-lent.json";
const LIFECYCLE: &str = "/run/y2/media-lifecycle.json";
/// The refusal of every media operation while the computer has the card.
pub const ON_COMPUTER: &str = "sd_on_computer";

/// `usb-sd-disk.json`.
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Setting {
    pub enabled: bool,
}

/// Whether the owner switched the SD card to open as a USB drive.
pub fn enabled(root: &Root) -> bool {
    serde_json::from_value::<Setting>(serde_json::Value::Object(root.json_object(SETTING)))
        .is_ok_and(|setting| setting.enabled)
}

/// Where a lend is: the card is still mounted while the player lets go of it, then it is the drive's.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum Stage {
    Lending,
    Lent,
}

/// `/run/y2/sd-lent.json`; this build is its only writer.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Lend {
    pub stage: Stage,
    /// The partition the computer gets (`/dev/mmcblk1p1`).
    pub device: String,
    /// When the lend started (monotonic seconds).
    pub since: f64,
}

/// The computer has the card, or is about to: media operations refuse (`sd_on_computer`).
pub fn on_computer(root: &Root) -> bool {
    root.path(LENT).exists()
}

/// The lend record; none without one or when it cannot be read (the USB service then gives the card back).
pub fn current(root: &Root) -> Option<Lend> {
    serde_json::from_value(serde_json::Value::Object(root.json_object(LENT))).ok()
}

/// The partition of the platform's own SD mount when a computer may have it; else why not.
pub fn lendable(root: &Root) -> Result<String, &'static str> {
    let (claim, mount) = sd_state(root);
    let mount = mount.ok_or("no_card")?;
    // Windows and macOS read only FAT32 and exFAT: an ext4 card stays on MTP.
    if !matches!(mount.filesystem.as_str(), "vfat" | "exfat") {
        return Err("not_fat");
    }
    // The background check (F49) reads the card until it finishes.
    if claim.checking(root) {
        return Err("checking");
    }
    Ok(mount.source)
}

/// Start lending `device`: from now on the MTP view and Reborn leave the card.
pub fn begin(root: &Root, device: String) -> Result<(), Failure> {
    let lend = Lend {
        stage: Stage::Lending,
        device,
        since: root.clock().monotonic(),
    };
    atomic_json(&root.path(LENT), &lend, false)
}

/// Unmount the lent card (never lazily or by force: a file still open keeps it); true once it is no longer
/// mounted and the record says Lent. False while a media operation holds the card or a file is open.
pub fn detach(root: &Root, lend: &Lend) -> Result<bool, Failure> {
    let Some(_lock) = try_lock(&root.path("/run/y2/media.lock"))? else {
        return Ok(false);
    };
    if !mounts_at(root, SD).is_empty() {
        let (_, mount) = sd_state(root);
        if mount.is_none_or(|m| m.source != lend.device) {
            // Another card or another mount: not the one this lend named.
            return Err(Failure::value("lent_card_changed"));
        }
        command::run(root, &["umount", SD], 3.0);
        if !mounts_at(root, SD).is_empty() {
            return Ok(false);
        }
    }
    let lent = Lend {
        stage: Stage::Lent,
        ..lend.clone()
    };
    atomic_json(&root.path(LENT), &lent, false)?;
    Ok(true)
}

/// The card is the player's again. `admit`: it was unmounted, so `media reconcile` checks and mounts it
/// again (a lend that never unmounted it leaves the mount as it is).
pub fn end(root: &Root, admit: bool) -> Result<(), Failure> {
    unlink_missing_ok(&root.path(LENT))?;
    if admit {
        let mut record = lifecycle(root);
        record.retry = Some(true);
        record.attempts = Some(0);
        atomic_json(&root.path(LIFECYCLE), &record, false)?;
    }
    Ok(())
}
