//! `internal-media provision`: Y2Media over the stock FAT the owner names, and the verified move
//! of a music library onto it (also the setup's kept files).
// SPDX-License-Identifier: GPL-2.0-only
use super::super::sys::{
    exists, is_symlink, mkdir, mkdir_io, mounts as all_mounts, name, statvfs, Object,
};
use super::{
    lock_runtime, mkfs, mount_locked, partition, probe, record, valid, Marker, Mounted,
    LEGACY_MUSIC, MUSIC, PATH,
};
use crate::reply::At;
use crate::reply::Failure;
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
use y2p_core::{
    command,
    digest::Sha256,
    mounts::mounts_at,
    py::{truthy, PyValue},
    Root,
};

const CHUNK: usize = 1 << 20;
const RESERVE: u64 = 64 << 20; // keep this much free after a migration

/// A failed rename's error, naming both paths (`[Errno n] text: 'a' -> 'b'`, as the migration's
/// refusal reasons always read).
fn os_error2(error: io::Error, a: &Path, b: &Path) -> Failure {
    let (text, class) = y2p_core::py::os_error(&error, Some(&a.to_string_lossy()));
    Failure::Os {
        text: format!("{text} -> {}", y2p_core::py::repr(&b.to_string_lossy())),
        class,
    }
}

/// `name` below a walk's relative path.
fn join(relative: &str, name: &str) -> String {
    format!("{relative}/{name}")
}

/// `name` below a walk's relative path, without the leading `./` of the top.
fn normal(relative: &str, name: &str) -> String {
    if relative == "." {
        name.to_owned()
    } else {
        join(relative, name)
    }
}

/// One directory of a walk: its path relative to the top, its directory and other names
/// (directory order).
pub(super) struct Walked {
    path: PathBuf,
    relative: String,
    pub(super) directories: Vec<String>,
    pub(super) files: Vec<String>,
}

/// Every directory below `top`, top-down, symlinks not followed; one that cannot be listed is
/// left out.
pub(super) fn walk(top: &Path) -> Vec<Walked> {
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
pub(super) fn tree(top: &Path) -> Result<Tree, Failure> {
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
            let info = fs::symlink_metadata(&path).at(&path)?;
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
        .at(path)
}

/// What a copy is verified by: the hex SHA-256 of a file's bytes (a test's may disagree, since a
/// copy that reads back wrong cannot be made on a fixture).
pub(super) type Digest = fn(&Path) -> Result<String, Failure>;

pub(super) fn sha256(path: &Path) -> Result<String, Failure> {
    let mut file = open_nofollow(path)?;
    // SAFETY: an advisory hint on our own descriptor; read the media, not the page cache.
    unsafe {
        libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED);
    }
    let mut hasher = Sha256::new();
    let mut chunk = vec![0u8; CHUNK];
    loop {
        let n = file.read(&mut chunk).at(path)?;
        if n == 0 {
            break;
        }
        hasher.update(&chunk[..n]);
    }
    Ok(hasher.hexdigest())
}

/// Give `path` the access and modification times of `source` (nanoseconds kept).
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
pub(super) fn copy_verified(
    source: &Path,
    target: &Path,
    size: u64,
    digest: Digest,
) -> Result<&'static str, Failure> {
    if exists(target) {
        let length = fs::metadata(target).at(target)?.len();
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
        .at(&temporary)?;
    let mut chunk = vec![0u8; CHUNK];
    loop {
        let n = src.read(&mut chunk).at(source)?;
        if n == 0 {
            break;
        }
        dst.write_all(&chunk[..n]).at(&temporary)?;
    }
    dst.sync_all().at(&temporary)?;
    drop((src, dst));
    if digest(source)? != digest(&temporary)? {
        fs::remove_file(&temporary).at(&temporary)?;
        return Err(Failure::Value(format!(
            "migration_hash_mismatch:{}",
            source.display()
        )));
    }
    fs::rename(&temporary, target).map_err(|e| os_error2(e, &temporary, target))?;
    let info = fs::metadata(source).at(source)?;
    utime(target, &info)?;
    Ok("copied")
}

/// The directory with its parents, fine when it exists; the error names the path.
pub(super) fn makedirs(path: &Path, mode: u32) -> Result<(), Failure> {
    mkdir_io(path, mode).at(path)
}

pub(super) fn sync() {
    // SAFETY: sync(2) takes no arguments.
    unsafe { libc::sync() };
}

/// Move the old music library onto Y2Media. Sources are removed only after everything verified.
pub(super) fn migrate(
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
    let free = statvfs(&media).at(&media)?;
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
        fs::remove_file(&path).at(&path)?;
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
pub(super) fn provision_with(
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
