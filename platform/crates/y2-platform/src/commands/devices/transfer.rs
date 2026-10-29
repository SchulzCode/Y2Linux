//! `transfer.py`: verified owner uploads (private staging, identity/space checks, durable no-replace
//! publish): `y2-platform transfer begin|commit|status|discard [--id] [--name] [--bytes] [--sha256]`.
//! Output is sorted; errors are the error record. Every file operation below `/data` goes through
//! directory descriptors opened without following symlinks (`fd`).
// SPDX-License-Identifier: GPL-2.0-only
use super::storage::{self, data_volume, Volume, INTERNAL};
use crate::reply::{atomic_json, private_directory, Failure, Outcome, Reply};
use fd::{
    available, check, flock, fstat, fsync, linkat, open_absolute_directory, open_directory, openat,
    read_chunk, unlinkat, write_some,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    os::{
        fd::{AsFd, AsRawFd, RawFd},
        unix::fs::DirBuilderExt,
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use workload::workload;
use y2p_core::{
    devices_support::{hex, record, Digest, Sha256},
    json::{Object, ObjectExt},
    py::{self, eq_int, exact_int},
    Root,
};

const MAX_BYTES: i64 = 512 * 1024 * 1024;
const RESERVE: u64 = 96 * 1024 * 1024;
const CHUNK: usize = 1024 * 1024;

mod fd;
mod workload;

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

/// The verified Y2Media volume; uploads are published there once it is mounted and recorded Ready.
fn media_volume(root: &Root) -> Result<Volume, Failure> {
    y2p_core::volume::verified(root, INTERNAL)
        .ok_or_else(|| Failure::value("verified_internal_media_unavailable"))
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
    let mut manifest = NewManifest {
        schema: 1,
        id: &token,
        name,
        bytes: size,
        sha256,
        data_generation: volume.generation,
        state: "Staging",
        record: record(root, "usb-transfer", json!({"name": name, "bytes": size})),
        upload_path: None,
    };
    atomic_json(&directory.join("manifest.json"), &manifest, true)?;
    manifest.upload_path = Some(format!("/data/transfer-staging/{token}/payload.part"));
    serde_json::to_value(manifest).map_err(Failure::value)
}

/// manifest.json as `begin` writes it (sorted); it prints it with the path to upload to.
#[derive(Serialize)]
struct NewManifest<'a> {
    schema: u8,
    id: &'a str,
    name: &'a str,
    bytes: i64,
    sha256: &'a str,
    /// The /data volume the upload was admitted on; commit refuses another one.
    data_generation: Option<String>,
    state: &'static str,
    /// The evidence record (`ctx.record`) commit completes with its result.
    record: Object,
    #[serde(skip_serializing_if = "Option::is_none")]
    upload_path: Option<String>,
}

/// What commit takes from a Staging manifest, each field checked (in this order: the name, the
/// /data generation, size, hash and evidence record).
struct Staged {
    /// The name's path components below `music`.
    parts: Vec<String>,
    size: i64,
    sha256: String,
}

fn staged(manifest: &Object, volume: &Volume) -> Result<Staged, Failure> {
    let parts = filename(manifest.get("name").and_then(Value::as_str))?;
    if manifest.value("data_generation") != json!(volume.generation) {
        return Err(Failure::value("data_generation_changed"));
    }
    let size = exact_int(manifest.get("bytes")).filter(|s| (1..=MAX_BYTES).contains(s));
    let sha256 = manifest
        .get("sha256")
        .and_then(Value::as_str)
        .filter(|s| hex_of(s, 64));
    // The evidence record is completed once the upload is published: one that is not an object
    // refuses before anything is copied.
    let record = manifest.get("record").is_some_and(Value::is_object);
    let (Some(size), Some(sha256), true) = (size, sha256, record) else {
        return Err(Failure::value("invalid_transfer_manifest"));
    };
    Ok(Staged {
        parts,
        size,
        sha256: sha256.to_owned(),
    })
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
    flock(lock.as_fd())?;
    // The manifest through the stage's own descriptor ("used even under a test root").
    let anchored = PathBuf::from(format!("/proc/self/fd/{}", stage.as_raw_fd()));
    // A missing or unreadable manifest has no state; JSON that is not an object is no manifest.
    let Value::Object(mut value) =
        json_read(&anchored.join("manifest.json")).unwrap_or_else(|| json!({}))
    else {
        return Err(Failure::value("invalid_transfer_manifest"));
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
    let Staged {
        parts,
        size,
        sha256: expected,
    } = staged(&value, &volume)?;
    if !storage::admitted(&volume, size as u64) {
        return Err(Failure::value("insufficient_space_for_seal_and_reserve"));
    }
    seal(stage.as_raw_fd(), size, &expected)?;
    if data_volume(root)?.generation != volume.generation {
        return Err(Failure::value("data_generation_changed"));
    }
    publish(
        root,
        data.as_raw_fd(),
        stage.as_raw_fd(),
        &parts,
        size,
        &expected,
    )?;
    value.insert("state".into(), json!("Committed"));
    // `staged` refused a manifest whose record is not an object.
    if let Some(record) = value.get_mut("record").and_then(Value::as_object_mut) {
        record.insert("result".into(), json!("OK"));
        record.insert("failure".into(), Value::Null);
    }
    atomic_json(&anchored.join("manifest.json"), &value, true)?;
    unlinkat(stage.as_raw_fd(), "sealed.part", 0, false)?;
    unlinkat(stage.as_raw_fd(), "payload.part", 0, false)?;
    fsync(stage.as_raw_fd())?;
    Ok(Value::Object(value))
}

/// Copy the upload to an exclusive `sealed.part` while hashing it, within the deadline and the
/// storage reserve; the copy must have the manifest's size and hash.
fn seal(stage: RawFd, size: i64, expected: &str) -> Result<(), Failure> {
    let source = openat(
        stage,
        "payload.part",
        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        0o777,
    )?;
    let original = fstat(source.as_raw_fd())?;
    if original.mode & libc::S_IFMT != libc::S_IFREG || original.nlink != 1 || original.size != size
    {
        return Err(Failure::value("incomplete_or_invalid_upload"));
    }
    // Copy to an O_EXCL inode: an older SFTP FD cannot mutate the published bytes after
    // verification. Stale seal files are safe explicit retry artifacts.
    let sealed = openat(
        stage,
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
    if hex(&digest.finalize()) != expected || fstat(source.as_raw_fd())?.size != size {
        return Err(Failure::value("upload_hash_or_size_mismatch"));
    }
    // SAFETY: plain syscall on an open descriptor.
    check(unsafe { libc::fchmod(sealed.as_raw_fd(), 0o644) })?;
    fsync(sealed.as_raw_fd())?;
    Ok(())
}

/// Publish the sealed upload as `music/<parts>` on its music volume without replacing anything.
fn publish(
    root: &Root,
    data: RawFd,
    stage: RawFd,
    parts: &[String],
    size: i64,
    expected: &str,
) -> Result<(), Failure> {
    // Music lives on Y2Media once it is mounted and recorded Ready, else on Y2DATA.
    let mut media = None;
    let mut destination = None;
    if storage::internal_ready(root) {
        destination = Some(media_volume(root)?);
        media = Some(open_absolute_directory(&root.path(INTERNAL))?);
    }
    let base = media.as_ref().map_or(data, AsRawFd::as_raw_fd);
    let mut target = open_directory(base, "music", true)?;
    for part in &parts[..parts.len() - 1] {
        target = open_directory(target.as_raw_fd(), part, true)?;
    }
    let leaf = &parts[parts.len() - 1];
    if let Some(destination) = destination {
        if media_volume(root)?.generation != destination.generation {
            return Err(Failure::value("media_generation_changed"));
        }
        if fstat(stage)?.dev == fstat(target.as_raw_fd())?.dev {
            linkat(stage, "sealed.part", target.as_raw_fd(), leaf)?;
        } else {
            publish_copy(stage, leaf, target.as_raw_fd(), size as u64, expected)?;
        }
    } else {
        // linkat is atomic and fails on an existing name; never overwrite music.
        linkat(stage, "sealed.part", target.as_raw_fd(), leaf)?;
    }
    fsync(target.as_raw_fd())?;
    Ok(())
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
        if hex(&digest.finalize()) != expected || fstat(out.as_raw_fd())?.size as u64 != size {
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
    flock(lock.as_fd())?;
    for name in ["manifest.json", "payload.part", "sealed.part", "lock"] {
        unlinkat(fd.as_raw_fd(), name, 0, true)?;
    }
    unlinkat(staging.as_raw_fd(), token, libc::AT_REMOVEDIR, false)?;
    fsync(staging.as_raw_fd())?;
    Ok(json!({"state": "Discarded", "id": token}))
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
