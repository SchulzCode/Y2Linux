//! Internal user media (Y2Media), the eMMC partition that the stock firmware used as its storage
//! (p8): its identity, mount, unmount and status here; `provision` and the library move in
//! `provision`, the automatic setup in `setup` (when it may start: `gate`).
//!
//! Y2Media is ext4, label Y2MEDIA, fixed UUID, at stock logical sector 3742720 for 11460608
//! sectors, on the internal controller. It is mounted at /media/internal and carries only the music
//! library (Reborn) and what the owner copies over MTP/SFTP. System state stays on Y2DATA (/data).
//!
//! Mounting only ever accepts that exact identity. Creating the filesystem replaces only the stock
//! FAT storage: either the explicit `provision` (names its serial) or `setup`, which every player runs
//! at start (STORAGE-SETUP, docs/planning/INTERNAL-STORAGE-SETUP-PLAN.md). `setup` decides from a
//! read-only look at the old storage: nothing worth keeping, or a few files (kept on Y2DATA meanwhile
//! and moved to "Old Player Files"), set up at once; a real library waits for the listener. Every step
//! is recorded on Y2DATA and resumes after a power loss.
// SPDX-License-Identifier: GPL-2.0-only
use super::records::Properties;
use super::sys::{device_instance, lock_now, mkdir, name, read, resolve, Mount};
use crate::reply::{atomic_json, Failure};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    path::{Path, PathBuf},
};
use y2p_core::{
    command,
    lenient::{self, BootId},
    mounts::mounts_at,
    Root,
};

mod gate;
mod provision;
mod setup;

pub use provision::{provision, Migration};
#[cfg(test)] // the boot order test reads the start screen's phase
pub(crate) use setup::splash;
pub use setup::{setup, shown_serial, SetupState};

const PATH: &str = "/media/internal";
const MUSIC: &str = "/media/internal/music";
const LEGACY_MUSIC: &str = "/data/music";
const LABEL: &str = "Y2MEDIA";
use y2p_core::volume::{Y2MEDIA_SIZE as SIZE, Y2MEDIA_START as START, Y2MEDIA_UUID as UUID};
const CONTROLLER: &str = "/11230000.mmc/";
const MARKER: &str = "/run/y2/internal-media.json";
// 30: noatime, no access-time writes during playback. noinit_itable: the eMMC write guard
// refuses WRITE_ZEROES/erase (storage-policy.h), so ext4's background inode-table zeroing
// only failed at every start (`mmc_erase: group start error -30`, V1 code review R6).
const OPTIONS: &str = "rw,nosuid,nodev,noexec,noatime,noinit_itable";
// Every ext4 setting named, so a later e2fsprogs cannot change the layout: the set the first
// Y2Media (owner's player, 2026-10-10) got from e2fsprogs 1.47 defaults.
const FEATURES: &str =
    "none,has_journal,ext_attr,resize_inode,dir_index,orphan_file,filetype,extent,64bit,\
flex_bg,metadata_csum_seed,sparse_super,large_file,huge_file,dir_nlink,extra_isize,metadata_csum";
const MKFS: [&str; 22] = [
    "mkfs.ext4",
    "-F",
    "-q",
    "-L",
    LABEL,
    "-U",
    UUID,
    "-m",
    "0",
    "-J",
    "size=64",
    "-b",
    "4096",
    "-I",
    "256",
    "-i",
    "16384",
    "-O",
    FEATURES,
    "-E",
    "lazy_itable_init=1,lazy_journal_init=1,nodiscard",
    "",
];

fn mkfs(device: &str) -> Vec<&str> {
    let mut argv = MKFS[..21].to_vec();
    argv.push(device);
    argv
}

/// The one internal partition with the Y2Media geometry, else None.
pub(crate) fn partition(root: &Root) -> Option<PathBuf> {
    let found: Vec<PathBuf> = root
        .glob("/sys/class/block/mmcblk*p*")
        .into_iter()
        .filter(|entry| {
            resolve(entry).to_string_lossy().contains(CONTROLLER)
                && read(&entry.join("partition")).is_some()
                && read(&entry.join("start")).as_deref() == Some(START)
                && read(&entry.join("size")).as_deref() == Some(SIZE)
        })
        .collect();
    (found.len() == 1).then(|| found[0].clone())
}

fn probe(root: &Root, entry: &Path) -> Properties {
    let device = format!("/dev/{}", name(entry));
    let answer = command::run(
        root,
        &["blkid", "-p", "-o", "export", &device],
        command::TIMEOUT,
    );
    if !answer.ok {
        return Properties::default();
    }
    Properties::parse(answer.output.as_deref())
}

fn valid(props: &Properties) -> bool {
    props.get("TYPE") == Some("ext4")
        && props.get("LABEL") == Some(LABEL)
        && props.get("UUID") == Some(UUID)
}

/// internal-media.json: Y2Media's state this boot (`atomic_json` keeps it compact: sftp-server
/// greps it for `"state":"Ready"`), also what the commands print.
#[derive(Clone, Debug, Serialize)]
pub struct Marker {
    schema: u8,
    pub state: &'static str,
    reason: Option<String>,
    path: &'static str,
    boot_id: Option<String>,
    /// What blkid found where Y2Media should be (`null` for what it did not print).
    #[serde(skip_serializing_if = "Option::is_none")]
    filesystem: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<Option<String>>,
    /// The stock FAT's serial a provision was not confirmed with.
    #[serde(skip_serializing_if = "Option::is_none")]
    serial: Option<Option<String>>,
    #[serde(flatten)]
    mounted: Option<Mounted>,
    /// What `provision` moved from the old /data/music library (only in its answer).
    #[serde(skip_serializing_if = "Option::is_none")]
    migration: Option<Migration>,
}

/// The mount a marker names.
#[derive(Clone, Debug, Serialize)]
struct Mounted {
    mount_id: i64,
    device_id: String,
    uuid: &'static str,
    /// Only after a mount this command made (`Some(None)`: its block instance cannot be told).
    #[serde(skip_serializing_if = "Option::is_none")]
    source_instance: Option<Option<String>>,
}

impl Mounted {
    fn of(mount: &Mount) -> Mounted {
        Mounted {
            mount_id: mount.mount_id,
            device_id: mount.device_id.clone(),
            uuid: UUID,
            source_instance: None,
        }
    }
}

impl Marker {
    fn new(root: &Root, state: &'static str, reason: Option<&str>) -> Marker {
        Marker {
            schema: 1,
            state,
            reason: reason.map(str::to_owned),
            path: PATH,
            boot_id: root.boot_id(),
            filesystem: None,
            label: None,
            serial: None,
            mounted: None,
            migration: None,
        }
    }

    fn mounted(self, mounted: Mounted) -> Marker {
        Marker {
            mounted: Some(mounted),
            ..self
        }
    }

    fn ready(&self) -> bool {
        self.state == "Ready"
    }
}

/// Write the marker; it is also the answer.
fn record(root: &Root, marker: Marker) -> Result<Marker, Failure> {
    mkdir(&root.path("/run/y2"), 0o700)?;
    atomic_json(&root.path(MARKER), &marker, false)?;
    Ok(marker)
}

fn lock_runtime(root: &Root) -> Result<File, Failure> {
    let runtime = root.path("/run/y2");
    mkdir(&runtime, 0o700)?;
    lock_now(&runtime.join("internal-media.lock"))
}

/// internal-media.json as a reader checks it: the claim's state and the mount it names, with the
/// types the claim is written in: texts for state, device and boot id, an integer for the mount id. A
/// field of another type matches nothing (and is not shown).
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Claim {
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::integer"
    )]
    schema: Option<i64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    state: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    reason: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    path: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::there"
    )]
    boot_id: Option<BootId>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    filesystem: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    label: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    serial: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::integer"
    )]
    mount_id: Option<i64>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    device_id: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    uuid: Option<String>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient::text"
    )]
    source_instance: Option<String>,
}

impl Claim {
    /// The claim at internal-media.json (all absent when it is missing or no object).
    pub fn read(root: &Root) -> Claim {
        lenient::read(root.json_object(MARKER))
    }

    /// Whether Y2Media is ready: recorded Ready this boot, and the claim's mount is still the only,
    /// live mount at /media/internal (the same mount and device id).
    pub fn ready(&self, root: &Root) -> bool {
        let current = mounts_at(root, PATH);
        let [mount] = &current[..] else {
            return false;
        };
        self.state.as_deref() == Some("Ready")
            && self
                .boot_id
                .as_ref()
                .unwrap_or(&BootId::Null)
                .is(root.boot_id().as_deref())
            && self.mount_id == Some(mount.mount_id)
            && self.device_id.as_deref() == Some(mount.device_id.as_str())
    }
}

/// True only while the recorded mount is still the live, exact Y2Media mount.
pub fn ready(root: &Root) -> bool {
    Claim::read(root).ready(root)
}

pub fn mount(root: &Root) -> Result<Marker, Failure> {
    let _lock = lock_runtime(root)?;
    mount_locked(root)
}

fn mount_locked(root: &Root) -> Result<Marker, Failure> {
    let Some(entry) = partition(root) else {
        let marker = Marker::new(root, "Unavailable", Some("partition_geometry_not_found"));
        return record(root, marker);
    };
    let props = probe(root, &entry);
    if !valid(&props) {
        let marker = Marker {
            filesystem: Some(props.text("TYPE")),
            label: Some(props.text("LABEL")),
            ..Marker::new(root, "Unavailable", Some("filesystem_identity_not_y2media"))
        };
        return record(root, marker);
    }
    let device = format!("/dev/{}", name(&entry));
    let current = mounts_at(root, PATH);
    if let Some(first) = current.first() {
        let marker = if current.len() == 1 && first.source == device {
            Marker::new(root, "Ready", None)
        } else {
            Marker::new(root, "Failed", Some("foreign_mount_at_path"))
        };
        return record(root, marker.mounted(Mounted::of(first)));
    }
    // Same policy as Y2DATA in the initramfs: only safe automatic repairs (journal replay etc).
    let check = command::run(root, &["e2fsck", "-p", &device], 180.0);
    if !check.ok {
        let marker = Marker::new(root, "Degraded", Some("filesystem_check_failed"));
        return record(root, marker);
    }
    mkdir(&root.path(PATH), 0o777)?;
    let result = command::run(
        root,
        &["mount", "-t", "ext4", "-o", OPTIONS, &device, PATH],
        20.0,
    );
    let current = mounts_at(root, PATH);
    match &current[..] {
        [mounted] if result.ok && mounted.source == device => {
            let made = Mounted {
                source_instance: Some(device_instance(root, &mounted.device_id)),
                ..Mounted::of(mounted)
            };
            record(root, Marker::new(root, "Ready", None).mounted(made))
        }
        _ => {
            let reason = result.reason.as_deref().unwrap_or("mount_identity_failed");
            record(root, Marker::new(root, "Failed", Some(reason)))
        }
    }
}

pub fn unmount(root: &Root) -> Result<Marker, Failure> {
    let _lock = lock_runtime(root)?;
    let marker = if mounts_at(root, PATH).is_empty() {
        Marker::new(root, "Unavailable", Some("not_mounted"))
    } else {
        let result = command::run(root, &["umount", PATH], 10.0);
        if !result.ok || !mounts_at(root, PATH).is_empty() {
            let reason = result.reason.as_deref().unwrap_or("unmount_busy");
            Marker::new(root, "Failed", Some(reason))
        } else {
            Marker::new(root, "Unavailable", Some("unmounted"))
        }
    };
    record(root, marker)
}

/// What `internal-media status` prints.
#[derive(Serialize)]
pub struct Status {
    schema: u8,
    path: &'static str,
    partition: Option<String>,
    filesystem: Option<String>,
    label: Option<String>,
    uuid: Option<String>,
    identity_valid: bool,
    mounted: bool,
    ready: bool,
    /// internal-media.json as it reads.
    marker: Claim,
}

pub fn status(root: &Root) -> Status {
    let entry = partition(root);
    let props = entry.as_ref().map(|e| probe(root, e)).unwrap_or_default();
    let marker = Claim::read(root);
    Status {
        schema: 1,
        path: PATH,
        partition: entry.as_deref().map(name),
        filesystem: props.text("TYPE"),
        label: props.text("LABEL"),
        uuid: props.text("UUID"),
        identity_valid: valid(&props),
        mounted: !mounts_at(root, PATH).is_empty(),
        ready: marker.ready(root),
        marker,
    }
}

#[cfg(test)]
#[path = "internal_tests.rs"]
mod internal_tests;
