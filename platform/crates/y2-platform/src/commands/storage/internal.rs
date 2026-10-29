//! `internal_media.py`: Internal user media, the eMMC partition that the stock firmware used as its
//! storage (p8).
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
use super::media::mkdir_io;
use super::observe::setup_status;
use super::records::Properties;
use super::sys::{
    device_instance, exists, get, get_eq, is_symlink, isdigit, lock_now, mkdir,
    mounts as all_mounts, name, object, read, release_view, resolve, statvfs, to_object, Mount,
    Object,
};
use crate::reply::{atomic_json, atomic_json_mode, write_text, Failure};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawFd,
    },
    path::{Path, PathBuf},
};
use y2p_core::mounts::mounts_at;
use y2p_core::py::PyValue;
use y2p_core::{
    command,
    py::{exact_int, truthy},
    storage_media::{fat_benign_only, Sha256Stream},
    Root,
};

const PATH: &str = "/media/internal";
const MUSIC: &str = "/media/internal/music";
const LEGACY_MUSIC: &str = "/data/music";
const LABEL: &str = "Y2MEDIA";
const UUID: &str = "79324c69-6e75-4801-8000-000000000103";
const START: &str = "3742720";
const SIZE: &str = "11460608";
const CONTROLLER: &str = "/11230000.mmc/";
const MARKER: &str = "/run/y2/internal-media.json";
// 30: noatime, no access-time writes during playback. noinit_itable: the eMMC write guard
// refuses WRITE_ZEROES/erase (storage-policy.h), so ext4's background inode-table zeroing
// only failed at every start (`mmc_erase: group start error -30`, V1 code review R6).
const OPTIONS: &str = "rw,nosuid,nodev,noexec,noatime,noinit_itable";
const CHUNK: usize = 1 << 20;
const RESERVE: u64 = 64 << 20; // keep this much free after a migration
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

// --- automatic setup (STORAGE-SETUP) ---
const SETUP: &str = "/data/system/internal-media-setup.json"; // survives formatting p8
const KEPT_STAGING: &str = "/data/system/internal-media-kept"; // kept files while p8 is formatted
const OLD: &str = "/run/y2/old-storage"; // the stock FAT, read-only
const STATUS: &str = "/run/y2/internal-media-setup.json";
const KEPT: &str = "Old Player Files";
const KEEP_LIMIT: u64 = 400 * 1000 * 1000;
const DATA_RESERVE: u64 = 150 * 1000 * 1000;
// On battery: at least the percentage the plan named, read like the shown one (stock
// oam_init: the PMIC's power-on OCV, latched before the boot load pulls the voltage down).
const MIN_PERCENT: f64 = 25.0;
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
struct Marker {
    schema: u8,
    state: &'static str,
    reason: Option<String>,
    path: &'static str,
    boot_id: Option<String>,
    /// What blkid found where Y2Media should be (`null` for what it did not print).
    #[serde(skip_serializing_if = "Option::is_none")]
    filesystem: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<Value>,
    /// The stock FAT's serial a provision was not confirmed with.
    #[serde(skip_serializing_if = "Option::is_none")]
    serial: Option<Value>,
    #[serde(flatten)]
    mounted: Option<Mounted>,
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

    fn object(&self) -> Object {
        to_object(self)
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

/// True only while the recorded mount is still the live, exact Y2Media mount.
fn ready(root: &Root) -> bool {
    let claim = object(root, MARKER);
    let current = mounts_at(root, PATH);
    get(&claim, "state").as_str() == Some("Ready")
        && current.len() == 1
        && get_eq(&claim, "boot_id", &Value::from(root.boot_id()))
        && get_eq(&claim, "mount_id", &json!(current[0].mount_id))
        && get_eq(&claim, "device_id", &json!(current[0].device_id))
}

pub fn mount(root: &Root) -> Result<Object, Failure> {
    let _lock = lock_runtime(root)?;
    Ok(mount_locked(root)?.object())
}

fn mount_locked(root: &Root) -> Result<Marker, Failure> {
    let Some(entry) = partition(root) else {
        let marker = Marker::new(root, "Unavailable", Some("partition_geometry_not_found"));
        return record(root, marker);
    };
    let props = probe(root, &entry);
    if !valid(&props) {
        let marker = Marker {
            filesystem: Some(props.value("TYPE")),
            label: Some(props.value("LABEL")),
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

pub fn unmount(root: &Root) -> Result<Object, Failure> {
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
    Ok(record(root, marker)?.object())
}

/// What `internal-media status` prints.
#[derive(Serialize)]
struct Status {
    schema: u8,
    path: &'static str,
    partition: Option<String>,
    filesystem: Value,
    label: Value,
    uuid: Value,
    identity_valid: bool,
    mounted: bool,
    ready: bool,
    marker: Object,
}

pub fn status(root: &Root) -> Object {
    let entry = partition(root);
    let props = entry.as_ref().map(|e| probe(root, e)).unwrap_or_default();
    to_object(&Status {
        schema: 1,
        path: PATH,
        partition: entry.as_deref().map(name),
        filesystem: props.value("TYPE"),
        label: props.value("LABEL"),
        uuid: props.value("UUID"),
        identity_valid: valid(&props),
        mounted: !mounts_at(root, PATH).is_empty(),
        ready: ready(root),
        marker: object(root, MARKER),
    })
}

// --- one-time provisioning ------------------------------------------------------------------

/// An OSError raised by a call on two paths (`os.replace`): `[Errno n] text: 'a' -> 'b'`.
fn os_error2(error: io::Error, a: &Path, b: &Path) -> Failure {
    let (text, class) = y2p_core::py::os_error(&error, Some(&a.to_string_lossy()));
    Failure::Os {
        text: format!("{text} -> {}", y2p_core::py::repr(&b.to_string_lossy())),
        class,
    }
}

/// `os.path.join(relative, name)`.
fn join(relative: &str, name: &str) -> String {
    format!("{relative}/{name}")
}

/// `os.path.normpath(os.path.join(relative, name))` for a walk's relative path.
fn normal(relative: &str, name: &str) -> String {
    if relative == "." {
        name.to_owned()
    } else {
        join(relative, name)
    }
}

/// One `os.walk` directory: its path relative to the top, its directory and other names (scandir
/// order).
struct Walked {
    path: PathBuf,
    relative: String,
    directories: Vec<String>,
    files: Vec<String>,
}

/// `os.walk(top, followlinks=False)`, top-down: a directory that cannot be listed is left out.
fn walk(top: &Path) -> Vec<Walked> {
    fn visit(path: &Path, relative: String, out: &mut Vec<Walked>) {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        let (mut directories, mut files) = (Vec::new(), Vec::new());
        for entry in entries.flatten() {
            let entry_name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().is_dir() {
                directories.push(entry_name);
            } else {
                files.push(entry_name);
            }
        }
        let below: Vec<String> = directories.clone();
        out.push(Walked {
            path: path.to_path_buf(),
            relative: relative.clone(),
            directories,
            files,
        });
        for child in below {
            let next = path.join(&child);
            if !is_symlink(&next) {
                visit(&next, normal(&relative, &child), out);
            }
        }
    }
    let mut out = Vec::new();
    visit(top, ".".into(), &mut out);
    out
}

/// The regular files with their sizes and the directories of a walk, relative to its top.
type Tree = (Vec<(String, u64)>, Vec<String>);

/// (relative path, size) of every regular file; refuses symlinks and special files.
fn tree(top: &Path) -> Result<Tree, Failure> {
    let (mut files, mut directories) = (Vec::new(), Vec::new());
    for walked in walk(top) {
        for child in &walked.directories {
            if is_symlink(&walked.path.join(child)) {
                return Err(Failure::Value(format!(
                    "migration_refuses_symlink:{}",
                    join(&walked.relative, child)
                )));
            }
        }
        directories.push(walked.relative.clone());
        for child in &walked.files {
            let path = walked.path.join(child);
            let info = fs::symlink_metadata(&path).map_err(|e| Failure::os(e, &path))?;
            if !info.file_type().is_file() {
                return Err(Failure::Value(format!(
                    "migration_refuses_special_file:{}",
                    join(&walked.relative, child)
                )));
            }
            files.push((normal(&walked.relative, child), info.len()));
        }
    }
    Ok((files, directories))
}

fn open_nofollow(path: &Path) -> Result<File, Failure> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| Failure::os(e, path))
}

/// What a copy is verified by: the hex SHA-256 of a file's bytes (a test's may disagree, since a
/// copy that reads back wrong cannot be made on a fixture).
type Digest = fn(&Path) -> Result<String, Failure>;

fn sha256(path: &Path) -> Result<String, Failure> {
    let mut file = open_nofollow(path)?;
    // SAFETY: an advisory hint on our own descriptor; read the media, not the page cache.
    unsafe {
        libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
    }
    let mut hasher = Sha256Stream::default();
    let mut chunk = vec![0u8; CHUNK];
    loop {
        let n = file.read(&mut chunk).map_err(|e| Failure::os(e, path))?;
        if n == 0 {
            break;
        }
        hasher.update(&chunk[..n]);
    }
    Ok(hasher.hexdigest())
}

/// `os.utime(path, ns=(atime_ns, mtime_ns))`.
fn utime(path: &Path, source: &fs::Metadata) -> Result<(), Failure> {
    let name = CString::new(path.as_os_str().as_bytes()).map_err(Failure::value)?;
    // time_t and c_long are 32 bits on the ARM target, 64 on the host.
    #[allow(clippy::unnecessary_cast)]
    let times = [
        libc::timespec {
            tv_sec: source.atime() as libc::time_t,
            tv_nsec: source.atime_nsec() as libc::c_long,
        },
        libc::timespec {
            tv_sec: source.mtime() as libc::time_t,
            tv_nsec: source.mtime_nsec() as libc::c_long,
        },
    ];
    // SAFETY: a NUL-terminated path and two valid timespecs.
    if unsafe { libc::utimensat(libc::AT_FDCWD, name.as_ptr(), times.as_ptr(), 0) } != 0 {
        return Err(Failure::os(io::Error::last_os_error(), path));
    }
    Ok(())
}

/// Copy to a temporary name, fsync, verify by hash, then publish atomically.
fn copy_verified(
    source: &Path,
    target: &Path,
    size: u64,
    digest: Digest,
) -> Result<&'static str, Failure> {
    if exists(target) {
        let length = fs::metadata(target)
            .map_err(|e| Failure::os(e, target))?
            .len();
        if length == size && digest(source)? == digest(target)? {
            return Ok("present");
        }
        return Err(Failure::Value(format!(
            "migration_target_differs:{}",
            target.display()
        )));
    }
    let temporary = PathBuf::from(format!("{}.part", target.display()));
    let mut src = open_nofollow(source)?;
    let mut dst = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o644)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&temporary)
        .map_err(|e| Failure::os(e, &temporary))?;
    let mut chunk = vec![0u8; CHUNK];
    loop {
        let n = src.read(&mut chunk).map_err(|e| Failure::os(e, source))?;
        if n == 0 {
            break;
        }
        dst.write_all(&chunk[..n])
            .map_err(|e| Failure::os(e, &temporary))?;
    }
    dst.sync_all().map_err(|e| Failure::os(e, &temporary))?;
    drop((src, dst));
    if digest(source)? != digest(&temporary)? {
        fs::remove_file(&temporary).map_err(|e| Failure::os(e, &temporary))?;
        return Err(Failure::Value(format!(
            "migration_hash_mismatch:{}",
            source.display()
        )));
    }
    fs::rename(&temporary, target).map_err(|e| os_error2(e, &temporary, target))?;
    let info = fs::metadata(source).map_err(|e| Failure::os(e, source))?;
    utime(target, &info)?;
    Ok("copied")
}

/// `os.makedirs(path, mode=mode, exist_ok=True)`.
fn makedirs(path: &Path, mode: u32) -> Result<(), Failure> {
    mkdir_io(path, mode).map_err(|e| Failure::os(e, path))
}

fn sync() {
    // SAFETY: sync(2) takes no arguments.
    unsafe { libc::sync() };
}

/// Move the old music library onto Y2Media. Sources are removed only after everything verified.
fn migrate(
    root: &Root,
    source: &str,
    target: &str,
    progress: &mut dyn FnMut(u64, u64),
    digest: Digest,
) -> Result<Value, Failure> {
    let (src, dst) = (root.path(source), root.path(target));
    if !src.is_dir() {
        return Ok(json!({"files": 0, "bytes": 0, "copied": 0, "removed": 0}));
    }
    let (files, directories) = tree(&src)?;
    let total: u64 = files.iter().map(|(_, size)| size).sum();
    let media = root.path(PATH);
    let free = statvfs(&media).map_err(|e| Failure::os(e, &media))?;
    if total + RESERVE > free.available() {
        return Err(Failure::value("migration_does_not_fit"));
    }
    makedirs(&dst, 0o755)?;
    for relative in &directories {
        makedirs(&dst.join(relative), 0o755)?;
    }
    let (mut copied, mut done) = (0u64, 0u64);
    for (relative, size) in &files {
        if copy_verified(&src.join(relative), &dst.join(relative), *size, digest)? == "copied" {
            copied += 1;
        }
        done += size;
        progress(done, total);
    }
    sync();
    // Everything on the target is verified: only now remove the originals.
    let mut removed = 0u64;
    for (relative, _) in &files {
        let path = src.join(relative);
        fs::remove_file(&path).map_err(|e| Failure::os(e, &path))?;
        removed += 1;
    }
    let mut longest = directories.clone();
    longest.sort_by_key(|d| std::cmp::Reverse(d.chars().count()));
    for relative in longest.iter().filter(|r| *r != ".") {
        let _ = fs::remove_dir(src.join(relative));
    }
    Ok(json!({"files": files.len(), "bytes": total, "copied": copied, "removed": removed}))
}

/// Create Y2Media over the stock FAT partition. Names the filesystem it replaces.
pub fn provision(
    root: &Root,
    confirm_uuid: &Value,
    migrate_music: bool,
) -> Result<Object, Failure> {
    provision_with(root, confirm_uuid, migrate_music, sha256)
}

/// `provision` with the migration's copies verified by `digest`.
fn provision_with(
    root: &Root,
    confirm_uuid: &Value,
    migrate_music: bool,
    digest: Digest,
) -> Result<Object, Failure> {
    if let Some(refused) = replace_stock(root, confirm_uuid)? {
        return Ok(refused.object());
    }
    let state = {
        let _lock = lock_runtime(root)?;
        mount_locked(root)?
    };
    if !state.ready() {
        return Ok(state.object());
    }
    mkdir(&root.path(MUSIC), 0o755)?;
    let mut result = state.object();
    if migrate_music {
        match migrate(root, LEGACY_MUSIC, MUSIC, &mut |_, _| {}, digest) {
            Ok(migration) => {
                result.insert("migration".into(), migration);
            }
            // `except (OSError, ValueError)`: anything else is uncaught.
            Err(error) if !error.caught() => return Err(error),
            Err(error) => {
                let reason = format!("migration_failed:{}", error.message());
                let mounted = state.mounted.map(|m| Mounted {
                    source_instance: None,
                    ..m
                });
                let marker = Marker {
                    mounted,
                    ..Marker::new(root, "Degraded", Some(&reason))
                };
                return Ok(record(root, marker)?.object());
            }
        }
    }
    Ok(result)
}

/// The first half of `provision`: Y2Media made over the confirmed stock FAT (nothing when it is
/// Y2Media already); `Some` refusal record otherwise.
fn replace_stock(root: &Root, confirm_uuid: &Value) -> Result<Option<Marker>, Failure> {
    let _lock = lock_runtime(root)?;
    let Some(entry) = partition(root) else {
        let marker = Marker::new(root, "Unavailable", Some("partition_geometry_not_found"));
        return Ok(Some(record(root, marker)?));
    };
    let props = probe(root, &entry);
    if valid(&props) {
        return Ok(None);
    }
    let device = format!("/dev/{}", name(&entry));
    let in_use =
        !mounts_at(root, PATH).is_empty() || all_mounts(root).iter().any(|m| m.source == device);
    if in_use {
        // Never replace a mounted filesystem. A finished Y2Media may be re-run to resume
        // an interrupted migration.
        let marker = Marker::new(root, "Failed", Some("partition_is_mounted"));
        return Ok(Some(record(root, marker)?));
    }
    if props.get("TYPE") != Some("vfat")
        || !truthy(confirm_uuid)
        || !props.value("UUID").py_eq(confirm_uuid)
    {
        // Only the stock FAT filesystem, named by its serial, may be replaced.
        let reason = "confirmation_does_not_match_stock_filesystem";
        let marker = Marker {
            filesystem: Some(props.value("TYPE")),
            serial: Some(props.value("UUID")),
            ..Marker::new(root, "Failed", Some(reason))
        };
        return Ok(Some(record(root, marker)?));
    }
    let made = command::run(root, &mkfs(&device), 300.0);
    if !made.ok {
        let reason = format!("mkfs_failed:{}", made.reason.as_deref().unwrap_or("None"));
        return Ok(Some(record(
            root,
            Marker::new(root, "Failed", Some(&reason)),
        )?));
    }
    Ok(None)
}

// --- automatic setup (STORAGE-SETUP) ----------------------------------------------------------

/// A stock leftover (cache, empty marker, computer clutter), never kept.
fn leftover(relative: &str) -> bool {
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
fn decide(kept: &[(String, u64)], data_free: u64) -> u8 {
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

/// `update.status(ctx).get('state')`.
fn update_state(root: &Root) -> Value {
    let path = root.path("/data/updates/state.json");
    let value = root.json("/data/updates/state.json");
    let value = match value {
        None if !exists(&path) && !is_symlink(&path) => {
            let ready = root.read("/data/.y2data-schema").as_deref() == Some("1")
                && root.path("/data/updates").is_dir();
            return json!(if ready { "Idle" } else { "Unavailable" });
        }
        other => other,
    };
    // The native core writes journal schema 2 since OTA-35 (BOOTIMG + root).
    match value {
        Some(Value::Object(map))
            if get(&map, "schema").py_eq(&json!(1)) || get(&map, "schema").py_eq(&json!(2)) =>
        {
            get(&map, "state").clone()
        }
        _ => json!("Failed"),
    }
}

/// `battery.ascending_table`: [[uv, value], ...]: 2..128 rows, strictly ascending voltage, bounded
/// values.
fn ascending_table(points: Option<&Value>, lower: i64, upper: i64, increasing: bool) -> bool {
    let Some(Value::Array(points)) = points else {
        return false;
    };
    if !(2..=128).contains(&points.len()) {
        return false;
    }
    let mut previous = (0i64, lower - 1);
    for point in points {
        let Some([uv, value]) = point
            .as_array()
            .map(Vec::as_slice)
            .and_then(|p| <&[Value; 2]>::try_from(p).ok())
        else {
            return false;
        };
        let (Some(uv), Some(value)) = (exact_int(Some(uv)), exact_int(Some(value))) else {
            return false;
        };
        if !(2_500_000..=4_300_000).contains(&uv)
            || !(lower..=upper).contains(&value)
            || uv <= previous.0
            || (increasing && value <= previous.1)
        {
            return false;
        }
        previous = (uv, value);
    }
    true
}

/// `battery.load_profile(ctx)['voltage_soc']`, or None for no valid profile.
fn load_profile(root: &Root) -> Option<Vec<(i64, i64)>> {
    let value = match root.json("/data/system/platform/battery-profile.json") {
        None | Some(Value::Null) => root.json("/etc/y2linux/battery-profile.json"),
        some => some,
    };
    let Some(Value::Object(value)) = value else {
        return None;
    };
    if !get(&value, "schema").py_eq(&json!(1)) {
        return None;
    }
    let points = value.get("voltage_soc");
    if !ascending_table(points, 0, 100, true) {
        return None;
    }
    let points: Vec<(i64, i64)> = points?
        .as_array()?
        .iter()
        .filter_map(|p| Some((p[0].as_i64()?, p[1].as_i64()?)))
        .collect();
    if points.first()?.1 != 0 || points.last()?.1 != 100 {
        return None;
    }
    // Stock r_profile (POWER-FINAL-16): internal resistance by voltage, used
    // with the charger-path current exactly where stock oam_run uses it.
    let resistance = value.get("voltage_resistance_milliohm");
    if resistance.is_some_and(|r| !r.is_null()) && !ascending_table(resistance, 20, 1000, false) {
        return None;
    }
    let capacity = value.get("design_capacity_uah");
    if capacity.is_some_and(|c| !c.is_null())
        && !exact_int(capacity).is_some_and(|c| (100_000..=5_000_000).contains(&c))
    {
        return None;
    }
    for (key, lower, upper) in [
        ("filter_seconds", 1, 300),
        ("rest_seconds", 10, 3600),
        ("rest_band_uv", 1000, 100_000),
        ("charging_offset_uv", 0, 200_000),
        ("seconds_per_percent", 10, 600),
    ] {
        if !exact_int(value.get(key)).is_some_and(|v| (lower..=upper).contains(&v)) {
            return None;
        }
    }
    if !value.get("provisional").is_some_and(Value::is_boolean)
        || !value.get("source").is_some_and(Value::is_string)
    {
        return None;
    }
    Some(points)
}

/// `battery.interpolate`.
fn interpolate(points: &[(i64, i64)], uv: i64) -> f64 {
    if uv <= points[0].0 {
        return points[0].1 as f64;
    }
    for pair in points.windows(2) {
        let ((low, a), (high, b)) = (pair[0], pair[1]);
        if uv <= high {
            return a as f64 + ((b - a) * (uv - low)) as f64 / (high - low) as f64;
        }
    }
    points[points.len() - 1].1 as f64
}

/// Why the setup must not start on its own now (it runs at a later start).
fn automatic_blocked(root: &Root) -> Option<&'static str> {
    let update = update_state(root);
    if ["Queued", "PendingHealth", "RollbackPending"]
        .iter()
        .any(|s| update.as_str() == Some(s))
    {
        return Some("update_in_progress"); // 180 s health window: never a long step here
    }
    let battery = root.path("/sys/class/power_supply/BAT0");
    if matches!(
        read(&battery.join("status")).as_deref(),
        Some("Charging" | "Full")
    ) || root
        .read("/sys/class/power_supply/y2-usb-input/online")
        .as_deref()
        == Some("1")
    {
        return None;
    }
    let profile = load_profile(root);
    let mut voltage = read(&battery.join("voltage_ocv"));
    if !voltage.as_deref().is_some_and(isdigit) {
        voltage = read(&battery.join("voltage_now")); // loaded: lower, so never too early
    }
    let (Some(profile), Some(voltage)) = (profile, voltage.filter(|v| isdigit(v))) else {
        return Some("battery_unknown");
    };
    let uv = voltage.parse::<i64>().unwrap_or(i64::MAX);
    if interpolate(&profile, uv) >= MIN_PERCENT {
        None
    } else {
        Some("battery_low")
    }
}

/// The stock FAT read-only at OLD after the read-only check (#44/F12 rules); None or a reason.
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

fn save(root: &Root, value: &Object) -> Result<(), Failure> {
    let path = root.path(SETUP);
    let folder = path.parent().expect("parent").to_path_buf();
    mkdir(&folder, 0o700)?;
    atomic_json(&path, value, true)?;
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open(&folder)
        .map_err(|e| Failure::os(e, &folder))?;
    directory.sync_all().map_err(|e| Failure::os(e, &folder))
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

    /// `saved[key]`: a missing key is Python's KeyError (a traceback, after the records written so
    /// far: the 'Running' note).
    fn key(&self, name: &str) -> Result<&Value, Failure> {
        self.0.get(name).ok_or_else(|| Failure::key_error(name))
    }

    fn phase(&self) -> Result<Option<&str>, Failure> {
        Ok(self.key("phase")?.as_str())
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
    match setup_locked(root, automatic, confirm_serial) {
        // `except (OSError, ValueError)`: a KeyError (or any other exception) is uncaught.
        Err(error) if !error.caught() => Err(error),
        Err(error) => {
            let reason = format!(
                "setup_error:{}",
                error.message().chars().take(100).collect::<String>()
            );
            note(root, "Failed", Some(&reason))
        }
        ok => ok,
    }
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
            if automatic && get(&saved.0, "phase").as_str() == Some("staging") {
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

    if saved.phase()? == Some("staging") {
        // `TYPE != 'vfat' or UUID != saved['serial']`: the serial is looked up only for a FAT.
        if props.get("TYPE") != Some("vfat") || !props.value("UUID").py_eq(saved.key("serial")?) {
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
            makedirs(target.parent().expect("parent"), 0o700)?;
            copy_verified(&source.join(relative), &target, *size, sha256)?;
            done += size;
            splash(root, tens(10, 50, done, total));
        }
        sync();
        saved.advance(root, "formatting")?;
    }

    if saved.phase()? == Some("formatting") {
        // Old Storage leaves the computer first (its view ended with 'Running' above).
        if !release_view(root, OLD) || !unmount_old(root) {
            return note(root, "Failed", Some("old_storage_busy"));
        }
        let props = probe(root, &entry);
        if !valid(&props) {
            // `TYPE == 'vfat' and UUID != saved['serial']`: the serial only for a FAT.
            if props.get("TYPE") == Some("vfat") && !props.value("UUID").py_eq(saved.key("serial")?)
            {
                return note(root, "Failed", Some("old_storage_changed"));
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

    if saved.phase()? == Some("migrating") {
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
        fs::remove_file(&path).map_err(|e| Failure::os(e, &path))?;
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
            let free = statvfs(&data).map_err(|e| Failure::os(e, &data))?;
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

/// `os.walk(top, topdown=False)` removing every directory below `top` that is empty by then.
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

/// `setup --confirm-shown`: the serial this boot reported as waiting, else None.
pub fn shown_serial(root: &Root) -> Value {
    let shown = setup_status(root);
    if get(&shown, "state").as_str() == Some("Waiting") {
        get(&shown, "serial").clone()
    } else {
        Value::Null
    }
}

#[cfg(test)]
#[path = "internal_tests.rs"]
mod internal_tests;

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

    #[test]
    fn profiles_interpolate_as_python() {
        let points = [(3_400_000, 0), (3_800_000, 50), (4_200_000, 100)];
        assert_eq!(interpolate(&points, 3_000_000), 0.0);
        assert_eq!(interpolate(&points, 3_600_000), 25.0);
        assert_eq!(interpolate(&points, 4_300_000), 100.0);
        assert_eq!(interpolate(&points, 3_700_001), 37.500125);
    }
}
