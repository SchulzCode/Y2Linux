//! `media format` and `media grow`: the owner changes the removable card he names.
// SPDX-License-Identifier: GPL-2.0-only
use super::super::records::MediaRecord;
use super::super::sys::{exists, isdigit, lock_now, name, read, release_sd, resolve};
use super::{candidates, operation, owned_mount, runtime, CONTROLLER, SD};
use crate::reply::{atomic_json, Failure};
use std::{
    fs::{File, OpenOptions},
    io,
    os::unix::{
        fs::{FileExt, OpenOptionsExt},
        io::AsRawFd,
    },
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};
use y2p_core::{command, mounts::mounts_at, Root};

// ---------------------------------------------------------------------------
// Explicit owner operations on the removable card: format and grow.
// Never automatic: a card with a foreign or undersized filesystem is reported,
// and only a command that names the card by CID may change it.

const MIN_CARD_SECTORS: u64 = 16384; // 8 MiB; anything smaller is not a storage card
pub(super) const PARTITION_START: u64 = 2048; // 1 MiB alignment
pub(super) const WIPE_TAIL_SECTORS: u64 = 64; // a stale GPT backup header lives in the last sectors
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
pub(super) fn mbr_for(sectors: u64) -> Vec<u8> {
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
pub(super) fn detach_for_change(root: &Root, state: &mut MediaRecord) -> Result<bool, Failure> {
    let old = mounts_at(root, SD);
    if old.is_empty() {
        return Ok(true);
    }
    if old.len() > 1 || old.iter().any(|m| !owned_mount(root, m)) {
        state.set_state("Failed", Some("ambiguous_or_foreign_mount"));
        return Ok(false);
    }
    if !release_sd(root, old[0].mount_id, true)? {
        state.set_state("Failed", Some("usb_release_failed"));
        return Ok(false);
    }
    let result = command::run(root, &["umount", SD], 10.0);
    if !result.ok || !mounts_at(root, SD).is_empty() {
        state.set_state(
            "Failed",
            Some(result.reason.as_deref().unwrap_or("unmount_busy")),
        );
        return Ok(false);
    }
    Ok(true)
}

/// How format has the kernel re-read the partition table it wrote (a test's card is a plain file).
pub(super) type Reread<'a> = &'a dyn Fn(&File) -> io::Result<()>;

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
) -> Result<MediaRecord, Failure> {
    change_card_with(root, action, confirm_cid, &blkrrpart)
}

/// `change_card` with the partition table re-read by `reread`.
pub(super) fn change_card_with(
    root: &Root,
    action: &str,
    confirm_cid: Option<&str>,
    reread: Reread,
) -> Result<MediaRecord, Failure> {
    if action != "format" && action != "grow" {
        return Err(Failure::value("unknown_media_change"));
    }
    let mut state = apply_change(root, action, confirm_cid, reread)?;
    if state.state_is("Starting") {
        // Mount through the normal admission path, after the change released the media lock.
        state.absorb(operation(root, "mount")?);
        state.action = Some(action.into());
    }
    Ok(state)
}

fn apply_change(
    root: &Root,
    action: &str,
    confirm_cid: Option<&str>,
    reread: Reread,
) -> Result<MediaRecord, Failure> {
    let runtime = runtime(root)?;
    let _lock = lock_now(&runtime.join("media.lock"))?;
    let mut state = MediaRecord::begin(root, action);
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
                state.set_state("Failed", Some("cid_confirmation_mismatch"));
                state.cid = Some(cid.into());
            } else if sectors < MIN_CARD_SECTORS {
                state.set_state("Failed", Some("card_too_small"));
            } else {
                let device = format!("/dev/{}", name(disk));
                state.cid = Some(cid.into());
                state.sectors = Some(sectors);
                state.device = Some(device.clone());
                if detach_for_change(root, &mut state)? {
                    if action == "format" {
                        format_steps(root, &mut state, &device, sectors, reread);
                    } else {
                        grow_steps(root, &mut state);
                    }
                }
            }
        }
        _ => state.set_state("Unavailable", Some("no_unique_card")),
    }
    if !state.state_is("Starting") {
        atomic_json(&runtime.join("media.json"), &state, false)?;
    }
    Ok(state)
}

pub(super) fn format_steps(
    root: &Root,
    state: &mut MediaRecord,
    device: &str,
    sectors: u64,
    reread: Reread,
) {
    let Ok(card) = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_SYNC | libc::O_CLOEXEC)
        .open(root.path(device))
    else {
        state.set_state("Failed", Some("card_not_writable"));
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
        state.set_state("Failed", Some("card_write_failed"));
        state.errno = error.raw_os_error();
        return;
    }
    if let Err(error) = reread(&card) {
        state.set_state("Failed", Some("partition_reread_failed"));
        state.errno = error.raw_os_error();
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
        state.set_state("Failed", Some(&format!("mkfs_failed:{reason}")));
    }
}

fn grow_steps(root: &Root, state: &mut MediaRecord) {
    let available = candidates(root);
    let [card] = &available[..] else {
        state.set_state("Failed", Some("grow_requires_one_ext4_card"));
        return;
    };
    if card.filesystem != "ext4" {
        state.set_state("Failed", Some("grow_requires_one_ext4_card"));
        return;
    }
    let device = card.device.as_str();
    // Only a clean (or safely auto-corrected) filesystem is resized.
    if !command::run(root, &["e2fsck", "-f", "-p", device], 300.0).ok {
        state.set_state("Failed", Some("filesystem_check_failed"));
        return;
    }
    let result = command::run(root, &["resize2fs", device], 300.0);
    if !result.ok {
        let reason = result.reason.as_deref().unwrap_or("None");
        state.set_state("Failed", Some(&format!("resize_failed:{reason}")));
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
}
