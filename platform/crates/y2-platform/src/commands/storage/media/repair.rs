//! `media repair`: the owner repairs the card the player marked, after a copy of its tables.
// SPDX-License-Identifier: GPL-2.0-only
use super::super::records::{MediaRecord, ReadOnly};
use super::super::sys::{maintenance, mkdir_io, statvfs, try_lock, wall_seconds};
use super::change::detach_for_change;
use super::{candidates, milliseconds, operation, owned_mount, prefix, runtime, Card, SD};
use crate::commands::power::published::{Battery, PowerState};
use crate::reply::{atomic_json, Failure};
use serde::Serialize;
use serde_json::Value;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    os::unix::fs::OpenOptionsExt,
    time::Instant,
};
use y2p_core::py::strerror;
use y2p_core::{
    command,
    mounts::mounts_at,
    storage_media::{repair_summary, tables_bytes},
    Root,
};

// ---------------------------------------------------------------------------
// Repair, started only by the owner on the card the player marked (mounted
// read-only after a failed check, or refused). Android's vold repairs every
// card before mounting it; here the owner decides, because a repair may
// shorten or remove the damaged files.

pub(super) const REPAIR_LAST: &str = "/data/system/platform/sd-repair-last.json";
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
pub(super) fn marked_for_repair(root: &Root) -> Option<MediaRecord> {
    let old = mounts_at(root, SD);
    let claim = MediaRecord::claim(root);
    if old.len() == 1
        && owned_mount(root, &old[0])
        && claim.mount_id == Some(old[0].mount_id)
        && ReadOnly::damaged(claim.read_only())
    {
        return Some(claim);
    }
    if old.is_empty() && MediaRecord::refused(root).is_some() {
        return Some(MediaRecord::last(root));
    }
    None
}

fn battery_allows_repair(root: &Root) -> Result<bool, Failure> {
    let power = PowerState::read(root);
    if !power.is_this_boot(root) {
        return Ok(false);
    }
    let battery = match &power.battery {
        Battery::Odd => return Err(Failure::value("power_battery_invalid")),
        battery => battery.reading(),
    };
    Ok(battery.charging_or_full() || battery.percent.is_some_and(|p| p >= REPAIR_MINIMUM_PERCENT))
}

/// Up to `n` bytes, fewer only at the end of the file.
fn read_up_to(file: &mut File, n: usize) -> io::Result<Vec<u8>> {
    let mut buffer = Vec::with_capacity(n);
    file.take(n as u64).read_to_end(&mut buffer)?;
    Ok(buffer)
}

/// The copy of a card's tables taken before its repair (`backup` of the repair record).
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Backup {
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
pub(crate) struct RepairRun {
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
    findings_before: Option<Vec<String>>,
    #[serde(flatten)]
    run: RepairRun,
}

/// Owner command: repair the marked card, then admit it again.
pub fn repair(root: &Root) -> Result<MediaRecord, Failure> {
    let mut state = {
        let _lease = maintenance(root)?;
        let (state, record) = repair_locked(root)?;
        if let Some(record) = record {
            let _ = atomic_json(&root.path(REPAIR_LAST), &record, true);
        }
        state
    };
    if state.state_is("Starting") {
        // Admission after the repair: a clean check mounts it read-write.
        state.absorb(operation(root, "mount")?);
        state.action = Some("repair".into());
    }
    Ok(state)
}

/// The repair under the media lock: its state, and its record once the repairer ran.
fn repair_locked(root: &Root) -> Result<(MediaRecord, Option<RepairRecord>), Failure> {
    let runtime = runtime(root)?;
    let lock = try_lock(&runtime.join("media.lock"));
    let (time, boot_id) = (wall_seconds(), root.boot_id());
    let mut state = MediaRecord::begin(root, "repair");
    let Some(_lock) = lock? else {
        state.set_state("Failed", Some("media_operation_in_progress"));
        return Ok((state, None));
    };
    let Some(before) = marked_for_repair(root) else {
        state.set_state("Failed", Some("card_not_marked_for_repair"));
        return Ok((state, None));
    };
    if !battery_allows_repair(root)? {
        state.set_state("Failed", Some("repair_needs_charger_or_battery"));
        return Ok((state, None));
    }
    if !detach_for_change(root, &mut state)? {
        return Ok((state, None));
    }
    let available = candidates(root);
    // An identity or check that cannot be read stops here, after the card was detached and
    // before a repair record exists.
    let identity = before.identity_id()?.map(str::to_owned);
    let card = match &available[..] {
        [card] if identity.as_deref() == Some(card.id()) => card,
        _ => {
            state.set_state("Failed", Some("card_changed"));
            return Ok((state, None));
        }
    };
    let Some(repairer) = repairer(card.filesystem) else {
        state.set_state("Failed", Some("filesystem_not_repairable"));
        return Ok((state, None));
    };
    let findings_before = before.findings()?;
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
    state.repair = Some(run.clone());
    if result.reason.as_deref() == Some("command_timeout") {
        state.set_state("Failed", Some("repair_timed_out"));
    }
    let record = RepairRecord {
        schema: 1,
        time,
        boot_id,
        card: prefix(identity.as_deref().unwrap_or_default()),
        filesystem: card.filesystem,
        findings_before,
        run,
    };
    Ok((state, Some(record)))
}
