//! `transfer.py`: verified owner uploads (private staging, identity/space checks, durable no-replace
//! publish): `y2-platform transfer begin|commit|status|discard [--id] [--name] [--bytes] [--sha256]`.
//! Output is sorted; errors are main.py's record. Every file operation below `/data` goes through
//! directory descriptors opened without following symlinks, as Python's `dir_fd` calls, and an
//! OSError names the relative name the call was given (Python's text).
// SPDX-License-Identifier: GPL-2.0-only
use super::storage::{self, Volume, INTERNAL};
use crate::reply::{atomic_json, private_directory, Failure, Outcome, Reply};
use serde_json::{json, Map, Value};
use std::{
    ffi::CString,
    io::{self, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::{
            ffi::OsStrExt,
            fs::{DirBuilderExt, OpenOptionsExt},
        },
    },
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
    thread,
    time::{Duration, Instant},
};
use y2p_core::{
    devices_support::{hex, record, Digest, Sha256},
    py::{self, eq_int, exact_int},
    Root,
};

const MAX_BYTES: i64 = 512 * 1024 * 1024;
const RESERVE: u64 = 96 * 1024 * 1024;
const CHUNK: usize = 1024 * 1024;

pub struct Request {
    pub id: Option<String>,
    pub name: Option<String>,
    pub bytes: Option<i64>,
    pub sha256: Option<String>,
}

pub fn run(root: &Root, action: &str, request: Request) -> Outcome {
    let value = match action {
        "begin" => begin(
            root,
            request.name.as_deref(),
            request.bytes,
            request.sha256.as_deref().unwrap_or_default(),
        )?,
        other => {
            // `args.id or ''`.
            let token = request.id.unwrap_or_default();
            match other {
                "commit" => workload(root, "NetworkTransfer", || commit(root, &token))?,
                "status" => inspect(root, &token)?,
                _ => discard(root, &token)?,
            }
        }
    };
    Reply::sorted(&value, 0)
}

// ---- Python's errors ------------------------------------------------------------------------------

/// An OSError of a call without a file name (read, write, fsync, flock).
fn unnamed(error: io::Error) -> Failure {
    let (text, class) = py::os_error(&error, None);
    Failure::Os { text, class }
}

/// An exception no Python code catches (an AttributeError on a tampered manifest): its traceback on
/// stderr, nothing on stdout, exit 1. Not a fallback: commands already ran, or files changed.
fn crash(what: &str) -> ! {
    let _ = writeln!(
        io::stderr(),
        "Traceback (most recent call last):\n  transfer.commit\n{what}"
    );
    std::process::exit(1)
}

// ---- descriptors ---------------------------------------------------------------------------------

fn c_name(name: &str) -> CString {
    CString::new(name).expect("validated names hold no NUL")
}

/// `os.open(name, flags, mode, dir_fd=dir)` (Python adds O_CLOEXEC).
fn openat(dir: RawFd, name: &str, flags: i32, mode: u32) -> Result<OwnedFd, Failure> {
    let c = c_name(name);
    // SAFETY: a NUL-terminated name; the result is a new descriptor this function owns.
    let fd = unsafe {
        libc::openat(
            dir,
            c.as_ptr(),
            flags | libc::O_CLOEXEC,
            mode as libc::c_uint,
        )
    };
    if fd < 0 {
        return Err(Failure::os(io::Error::last_os_error(), Path::new(name)));
    }
    // SAFETY: fd was just opened and is owned by nobody else.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// `os.open(path, O_RDONLY | O_DIRECTORY | O_NOFOLLOW)` of an absolute (root-prefixed) path.
fn open_absolute_directory(path: &Path) -> Result<OwnedFd, Failure> {
    let name = CString::new(path.as_os_str().as_bytes()).expect("paths hold no NUL");
    // SAFETY: as openat.
    let fd = unsafe {
        libc::open64(
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(Failure::os(io::Error::last_os_error(), path));
    }
    // SAFETY: as openat.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn fstat(fd: RawFd) -> Result<libc::stat64, Failure> {
    // SAFETY: a writable stat buffer for an open descriptor.
    let mut stat: libc::stat64 = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat64(fd, &mut stat) } != 0 {
        return Err(unnamed(io::Error::last_os_error()));
    }
    Ok(stat)
}

/// `f_bavail * f_frsize` of the filesystem holding `fd`.
fn available(fd: RawFd) -> Result<u64, Failure> {
    // SAFETY: a writable statvfs buffer for an open descriptor.
    let mut vfs: libc::statvfs64 = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstatvfs64(fd, &mut vfs) } != 0 {
        return Err(unnamed(io::Error::last_os_error()));
    }
    Ok(vfs.f_bavail as u64 * vfs.f_frsize as u64)
}

fn check(result: libc::c_int) -> Result<(), Failure> {
    if result != 0 {
        return Err(unnamed(io::Error::last_os_error()));
    }
    Ok(())
}

fn fsync(fd: RawFd) -> Result<(), Failure> {
    // SAFETY: plain syscall on an open descriptor.
    check(unsafe { libc::fsync(fd) })
}

/// `fcntl.flock(fd, operation | LOCK_NB)`: busy is `[Errno 11] Resource temporarily unavailable`.
fn flock(fd: RawFd, operation: libc::c_int) -> Result<(), Failure> {
    // SAFETY: plain syscall on an open descriptor.
    check(unsafe { libc::flock(fd, operation | libc::LOCK_NB) })
}

/// `os.unlink(name, dir_fd=dir)` (`os.rmdir` with AT_REMOVEDIR); `missing_ok` as
/// `except FileNotFoundError: pass`.
fn unlinkat(dir: RawFd, name: &str, flags: libc::c_int, missing_ok: bool) -> Result<(), Failure> {
    let c = c_name(name);
    // SAFETY: a NUL-terminated name relative to an open directory.
    if unsafe { libc::unlinkat(dir, c.as_ptr(), flags) } != 0 {
        let error = io::Error::last_os_error();
        if missing_ok && error.raw_os_error() == Some(libc::ENOENT) {
            return Ok(());
        }
        return Err(Failure::os(error, Path::new(name)));
    }
    Ok(())
}

/// `os.link(source, name, src_dir_fd=…, dst_dir_fd=…, follow_symlinks=False)`: atomic, and fails
/// on an existing name (`[Errno 17] File exists: 'sealed.part' -> 'name'`).
fn linkat(from_dir: RawFd, from: &str, to_dir: RawFd, to: &str) -> Result<(), Failure> {
    // SAFETY: NUL-terminated names relative to open directories.
    let result = unsafe {
        libc::linkat(
            from_dir,
            c_name(from).as_ptr(),
            to_dir,
            c_name(to).as_ptr(),
            0,
        )
    };
    if result != 0 {
        let error = io::Error::last_os_error();
        let (text, class) = py::os_error(&error, None);
        return Err(Failure::Os {
            text: format!("{text}: {} -> {}", py::repr(from), py::repr(to)),
            class,
        });
    }
    Ok(())
}

fn read_chunk(fd: RawFd, limit: usize) -> Result<Vec<u8>, Failure> {
    let mut buffer = vec![0u8; limit];
    // SAFETY: the buffer is writable for `limit` bytes.
    let count = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), limit) };
    if count < 0 {
        return Err(unnamed(io::Error::last_os_error()));
    }
    buffer.truncate(count as usize);
    Ok(buffer)
}

fn write_some(fd: RawFd, data: &[u8]) -> Result<usize, Failure> {
    // SAFETY: the slice is readable for its length.
    let count = unsafe { libc::write(fd, data.as_ptr().cast(), data.len()) };
    if count < 0 {
        return Err(unnamed(io::Error::last_os_error()));
    }
    Ok(count as usize)
}

/// `open_directory(parent, component, create)`: one component, never through a symlink, never onto
/// another filesystem.
fn open_directory(parent: RawFd, component: &str, create: bool) -> Result<OwnedFd, Failure> {
    if create {
        // SAFETY: a NUL-terminated name relative to an open directory.
        let c = c_name(component);
        if unsafe { libc::mkdirat(parent, c.as_ptr(), 0o755) } == 0 {
            fsync(parent)?;
        } else {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::EEXIST) {
                return Err(Failure::os(error, Path::new(component)));
            }
        }
    }
    let fd = openat(
        parent,
        component,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        0o777,
    )?;
    if fstat(fd.as_raw_fd())?.st_dev != fstat(parent)?.st_dev {
        return Err(Failure::value("transfer_submount_refused"));
    }
    Ok(fd)
}

// ---- checks ---------------------------------------------------------------------------------------

fn hex_of(text: &str, length: usize) -> bool {
    text.len() == length && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// `filename(name)`: a relative path of 1-8 components, none empty or hidden, each at most 120 bytes
/// without control characters or backslashes, at most 512 bytes in all.
fn filename(name: Option<&str>) -> Result<Vec<String>, Failure> {
    let invalid = || Failure::value("invalid_media_name");
    let name = name.ok_or_else(invalid)?;
    // `len(name.encode())`: a name that was not UTF-8 (surrogate escapes) cannot be encoded.
    if let Some(text) = y2p_core::os_text::encode_error(name) {
        return Err(Failure::raised("UnicodeEncodeError", text));
    }
    if name.len() > 512 {
        return Err(invalid());
    }
    let parts: Vec<&str> = name.split('/').collect();
    let bad = |p: &&str| {
        p.is_empty()
            || p.starts_with('.')
            || p.len() > 120
            || p.chars().any(|c| (c as u32) < 32 || c == '\\')
    };
    if !(1..=8).contains(&parts.len()) || parts.iter().any(bad) {
        return Err(invalid());
    }
    Ok(parts.into_iter().map(str::to_owned).collect())
}

fn stage_path(root: &Root, token: &str) -> Result<PathBuf, Failure> {
    if !hex_of(token, 32) {
        return Err(Failure::value("invalid_transfer_id"));
    }
    Ok(root.path("/data/transfer-staging").join(token))
}

fn data_volume(root: &Root) -> Result<Volume, Failure> {
    let value = storage::volume(root, "/data");
    if value.state != "Ready" || value.generation.is_none() {
        return Err(Failure::value("verified_data_unavailable"));
    }
    Ok(value)
}

/// The verified Y2Media volume; uploads are published there once it is mounted and recorded Ready.
fn media_volume(root: &Root) -> Result<Volume, Failure> {
    let value = storage::volume(root, INTERNAL);
    if value.state != "Ready" || value.generation.is_none() {
        return Err(Failure::value("verified_internal_media_unavailable"));
    }
    Ok(value)
}

/// `common.json_read(path, default)`: any parsed document, the default when unreadable or not JSON.
fn json_read(path: &Path) -> Option<Value> {
    serde_json::from_str(
        &y2p_core::root::read(path, y2p_core::root::READ_LIMIT).unwrap_or_default(),
    )
    .ok()
}

// ---- the actions ----------------------------------------------------------------------------------

fn begin(
    root: &Root,
    name: Option<&str>,
    size: Option<i64>,
    sha256: &str,
) -> Result<Value, Failure> {
    filename(name)?;
    let name = name.unwrap_or_default();
    let size = size
        .filter(|s| (1..=MAX_BYTES).contains(s) && hex_of(sha256, 64))
        .ok_or_else(|| Failure::value("invalid_transfer_size_or_hash"))?;
    let volume = data_volume(root)?;
    if !storage::admitted(&volume, size as u64 * 2) {
        return Err(Failure::value(
            "insufficient_space_for_upload_seal_and_reserve",
        ));
    }
    let staging = root.path("/data/transfer-staging");
    private_directory(&staging)?;
    let entries = std::fs::read_dir(&staging).map_err(|e| Failure::os(e, &staging))?;
    let mut count = 0;
    for entry in entries.take(9) {
        entry.map_err(|e| Failure::os(e, &staging))?;
        count += 1;
    }
    if count >= 8 {
        return Err(Failure::value(
            "staging_limit_remove_completed_or_aborted_transfers",
        ));
    }
    let token = py::uuid4()
        .map_err(|e| Failure::os(e, Path::new("/dev/urandom")))?
        .replace('-', "");
    let directory = staging.join(&token);
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .map_err(|e| Failure::os(e, &directory))?;
    let mut value = Map::new();
    value.insert("schema".into(), json!(1));
    value.insert("id".into(), json!(token));
    value.insert("name".into(), json!(name));
    value.insert("bytes".into(), json!(size));
    value.insert("sha256".into(), json!(sha256));
    value.insert("data_generation".into(), json!(volume.generation));
    value.insert("state".into(), json!("Staging"));
    value.insert(
        "record".into(),
        serde_json::to_value(record(
            root,
            "usb-transfer",
            json!({"name": name, "bytes": size}),
        ))
        .map_err(Failure::value)?,
    );
    atomic_json(&directory.join("manifest.json"), &value, true)?;
    value.insert(
        "upload_path".into(),
        json!(format!("/data/transfer-staging/{token}/payload.part")),
    );
    Ok(Value::Object(value))
}

fn commit(root: &Root, token: &str) -> Result<Value, Failure> {
    stage_path(root, token)?; // validate before any path access
    let volume = data_volume(root)?;
    let data = open_absolute_directory(&root.path("/data"))?;
    let stage_root = open_directory(data.as_raw_fd(), "transfer-staging", false)?;
    let stage = open_directory(stage_root.as_raw_fd(), token, false)?;
    let lock = openat(
        stage.as_raw_fd(),
        "lock",
        libc::O_CREAT | libc::O_RDWR | libc::O_NOFOLLOW,
        0o600,
    )?;
    flock(lock.as_raw_fd(), libc::LOCK_EX)?;
    // The manifest through the stage's own descriptor ("used even under a test root").
    let anchored = PathBuf::from(format!("/proc/self/fd/{}", stage.as_raw_fd()));
    let mut value = match json_read(&anchored.join("manifest.json")).unwrap_or(json!({})) {
        Value::Object(map) => map,
        // `value.get` of a list or number: Python's AttributeError. No fallback: the volume's blkid
        // requests already ran (a second run would ask again).
        _ => crash("AttributeError: the manifest is not a dict"),
    };
    let state = value
        .get("state")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if !eq_int(value.get("schema"), 1)
        || value.get("id").and_then(Value::as_str) != Some(token)
        || !matches!(state.as_deref(), Some("Staging" | "Committed"))
    {
        return Err(Failure::value("invalid_transfer_state"));
    }
    if state.as_deref() == Some("Committed") {
        return Ok(Value::Object(value));
    }
    let parts = filename(value.get("name").and_then(Value::as_str))?;
    let recorded_generation = value.get("data_generation").cloned().unwrap_or(Value::Null);
    if recorded_generation != json!(volume.generation) {
        return Err(Failure::value("data_generation_changed"));
    }
    let size = exact_int(value.get("bytes")).filter(|s| (1..=MAX_BYTES).contains(s));
    let expected = value
        .get("sha256")
        .and_then(Value::as_str)
        .filter(|s| hex_of(s, 64))
        .map(str::to_owned);
    let (Some(size), Some(expected)) = (size, expected) else {
        return Err(Failure::value("invalid_transfer_manifest"));
    };
    if !storage::admitted(&volume, size as u64) {
        return Err(Failure::value("insufficient_space_for_seal_and_reserve"));
    }
    let source = openat(
        stage.as_raw_fd(),
        "payload.part",
        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        0o777,
    )?;
    let original = fstat(source.as_raw_fd())?;
    if original.st_mode & libc::S_IFMT != libc::S_IFREG
        || original.st_nlink != 1
        || original.st_size != size
    {
        return Err(Failure::value("incomplete_or_invalid_upload"));
    }
    // Copy to an O_EXCL inode: an older SFTP FD cannot mutate the published bytes after
    // verification. Stale seal files are safe explicit retry artifacts.
    let sealed = openat(
        stage.as_raw_fd(),
        "sealed.part",
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW,
        0o600,
    )?;
    let mut digest = Sha256::new();
    let mut remaining = size as usize;
    let deadline = Instant::now() + Duration::from_secs(600);
    while remaining > 0 {
        if Instant::now() >= deadline {
            return Err(Failure::value("transfer_seal_deadline"));
        }
        let chunk = read_chunk(source.as_raw_fd(), CHUNK.min(remaining))?;
        if chunk.is_empty() {
            return Err(Failure::value("upload_disappeared_or_truncated"));
        }
        if (available(sealed.as_raw_fd())? as i128) - (chunk.len() as i128) < RESERVE as i128 {
            return Err(Failure::value("transfer_storage_reserve"));
        }
        let mut view = &chunk[..];
        while !view.is_empty() {
            let count = write_some(sealed.as_raw_fd(), view)?;
            if count == 0 {
                return Err(Failure::os_message("short_transfer_write"));
            }
            view = &view[count..];
        }
        digest.update(&chunk);
        remaining -= chunk.len();
    }
    if hex(&digest.finalize()) != expected || fstat(source.as_raw_fd())?.st_size != size {
        return Err(Failure::value("upload_hash_or_size_mismatch"));
    }
    // SAFETY: plain syscall on an open descriptor.
    check(unsafe { libc::fchmod(sealed.as_raw_fd(), 0o644) })?;
    fsync(sealed.as_raw_fd())?;
    if json!(data_volume(root)?.generation) != recorded_generation {
        return Err(Failure::value("data_generation_changed"));
    }
    // Music lives on Y2Media once it is mounted and recorded Ready, else on Y2DATA.
    let mut media = None;
    let mut destination = None;
    if storage::internal_ready(root) {
        destination = Some(media_volume(root)?);
        media = Some(open_absolute_directory(&root.path(INTERNAL))?);
    }
    let base = media.as_ref().unwrap_or(&data).as_raw_fd();
    let mut target = open_directory(base, "music", true)?;
    for part in &parts[..parts.len() - 1] {
        target = open_directory(target.as_raw_fd(), part, true)?;
    }
    let leaf = &parts[parts.len() - 1];
    if let Some(destination) = destination {
        if media_volume(root)?.generation != destination.generation {
            return Err(Failure::value("media_generation_changed"));
        }
        if fstat(stage.as_raw_fd())?.st_dev == fstat(target.as_raw_fd())?.st_dev {
            linkat(stage.as_raw_fd(), "sealed.part", target.as_raw_fd(), leaf)?;
        } else {
            publish_copy(
                stage.as_raw_fd(),
                leaf,
                target.as_raw_fd(),
                size as u64,
                &expected,
            )?;
        }
    } else {
        // linkat is atomic and fails on an existing name; never overwrite music.
        linkat(stage.as_raw_fd(), "sealed.part", target.as_raw_fd(), leaf)?;
    }
    fsync(target.as_raw_fd())?;
    value.insert("state".into(), json!("Committed"));
    match value.get_mut("record") {
        Some(Value::Object(record)) => {
            record.insert("result".into(), json!("OK"));
            record.insert("failure".into(), Value::Null);
        }
        _ => crash("AttributeError: the manifest's record is not a dict"),
    }
    atomic_json(&anchored.join("manifest.json"), &value, true)?;
    unlinkat(stage.as_raw_fd(), "sealed.part", 0, false)?;
    unlinkat(stage.as_raw_fd(), "payload.part", 0, false)?;
    fsync(stage.as_raw_fd())?;
    Ok(Value::Object(value))
}

/// Cross-filesystem publish: copy the sealed bytes into the target directory, verify, link without
/// replacing. A hard link cannot leave Y2DATA, so the already hash-verified sealed file is copied to
/// an exclusive temporary name beside its destination and then linked: the final name appears
/// atomically and never replaces an existing file.
fn publish_copy(
    stage: RawFd,
    name: &str,
    target: RawFd,
    size: u64,
    expected: &str,
) -> Result<(), Failure> {
    let temporary = format!(".{name}.y2part");
    let source = openat(
        stage,
        "sealed.part",
        libc::O_RDONLY | libc::O_NOFOLLOW,
        0o777,
    )?;
    let out = openat(
        target,
        &temporary,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW,
        0o644,
    )?;
    let result = (|| {
        let mut digest = Sha256::new();
        let mut remaining = size as usize;
        if (available(out.as_raw_fd())? as i128) - (size as i128) < RESERVE as i128 {
            return Err(Failure::value("transfer_storage_reserve"));
        }
        while remaining > 0 {
            let chunk = read_chunk(source.as_raw_fd(), CHUNK.min(remaining))?;
            if chunk.is_empty() {
                return Err(Failure::value("sealed_upload_truncated"));
            }
            let mut view = &chunk[..];
            while !view.is_empty() {
                let count = write_some(out.as_raw_fd(), view)?;
                view = &view[count..];
            }
            digest.update(&chunk);
            remaining -= chunk.len();
        }
        if hex(&digest.finalize()) != expected || fstat(out.as_raw_fd())?.st_size as u64 != size {
            return Err(Failure::value("published_copy_hash_or_size_mismatch"));
        }
        fsync(out.as_raw_fd())?;
        // Atomic and fails on an existing name: never overwrite music.
        linkat(target, &temporary, target, name)
    })();
    drop((source, out));
    // An unlink error other than a missing file replaces the original exception, as in Python's
    // `finally`.
    unlinkat(target, &temporary, 0, true)?;
    result
}

fn inspect(root: &Root, token: &str) -> Result<Value, Failure> {
    let path = stage_path(root, token)?;
    let symlink = |p: &Path| {
        p.symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
    };
    if symlink(&path) || path.parent().is_some_and(symlink) {
        return Err(Failure::value("staging_symlink_refused"));
    }
    Ok(json_read(&path.join("manifest.json")).unwrap_or_else(|| json!({"state": "Unavailable"})))
}

/// Explicit owner abort removes only a known stage; never follows a symlink.
fn discard(root: &Root, token: &str) -> Result<Value, Failure> {
    data_volume(root)?;
    stage_path(root, token)?;
    let staging = open_absolute_directory(&root.path("/data/transfer-staging"))?;
    let fd = open_directory(staging.as_raw_fd(), token, false)?;
    let lock = openat(
        fd.as_raw_fd(),
        "lock",
        libc::O_CREAT | libc::O_RDWR | libc::O_NOFOLLOW,
        0o600,
    )?;
    flock(lock.as_raw_fd(), libc::LOCK_EX)?;
    for name in ["manifest.json", "payload.part", "sealed.part", "lock"] {
        unlinkat(fd.as_raw_fd(), name, 0, true)?;
    }
    unlinkat(staging.as_raw_fd(), token, libc::AT_REMOVEDIR, false)?;
    fsync(staging.as_raw_fd())?;
    Ok(json!({"state": "Discarded", "id": token}))
}

// ---- workload.workload ----------------------------------------------------------------------------

/// `@workload(class_name)`: a shared activity lease for the whole call (sleep takes it exclusively;
/// unlike optional frequency hints this is a mandatory operation exclusion, and flock dies with the
/// process), plus the optional `/dev/y2-workload` QoS hint renewed every second on its own thread,
/// even while a blocking transfer runs for minutes. A missing or failing hint never fails the work.
fn workload<T>(
    root: &Root,
    class: &str,
    work: impl FnOnce() -> Result<T, Failure>,
) -> Result<T, Failure> {
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    let path = runtime.join("activity.lock");
    let _lease = y2p_core::lock::try_shared(&path)
        .map_err(|e| Failure::os(e, &path))?
        .ok_or_else(|| Failure::value("sleep_operation_in_progress"))?;
    let payload = format!("{class} 3000\n").into_bytes();
    let mut renewal = None;
    let mut hint = None;
    if let Ok(file) = std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(root.path("/dev/y2-workload"))
    {
        let file = Arc::new(file);
        if matches!((&*file).write(&payload), Ok(n) if n == payload.len()) {
            let (stop, stopped) = mpsc::channel::<()>();
            let (writer, payload) = (Arc::clone(&file), payload.clone());
            let worker = thread::spawn(move || loop {
                if !matches!(stopped.try_recv(), Err(mpsc::TryRecvError::Empty)) {
                    return;
                }
                if !matches!((&*writer).write(&payload), Ok(n) if n == payload.len()) {
                    return;
                }
                if !matches!(
                    stopped.recv_timeout(Duration::from_secs(1)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    return;
                }
            });
            renewal = Some((stop, worker));
        }
        hint = Some(file);
    }
    let result = work();
    if let Some((stop, worker)) = renewal {
        drop(stop);
        let _ = worker.join();
    }
    drop(hint); // close/process death releases only this lease
    result
}

#[cfg(test)]
#[path = "transfer_tests.rs"]
mod transfer_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_ids_follow_transfer_py() {
        assert_eq!(filename(Some("A/b.flac")).unwrap(), ["A", "b.flac"]);
        for bad in [
            "",
            "a//b",
            ".hidden",
            "a/.b",
            "a\\b",
            "a\u{1}b",
            "1/2/3/4/5/6/7/8/9",
        ] {
            assert!(filename(Some(bad)).is_err(), "{bad:?}");
        }
        assert!(filename(None).is_err());
        assert!(filename(Some(&"x".repeat(121))).is_err());
        assert!(filename(Some(&"x".repeat(120))).is_ok());
        assert!(hex_of(&"a".repeat(32), 32) && !hex_of(&"A".repeat(32), 32));
    }
}
