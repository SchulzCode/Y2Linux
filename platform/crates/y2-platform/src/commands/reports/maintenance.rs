//! The owner's reset and private state export (maintenance.py); Y2DATA is never formatted.
//!
//! Reset moves only reviewed user paths into a private recovery directory (`plan`, then `execute`).
//! Purge is a separate confirmed, bounded operation (`purge`). SSH/entropy/update/factory state
//! stays. The plan's `confirmation` and the settings result's sha256 are hashes over Python's exact
//! `json.dumps` bytes (pyjson): a plan an earlier release made still confirms. This file holds what
//! they share: the commands' answers and the descriptor-relative walk of /data that never follows a
//! link or crosses a mount.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    json_dict,
    media::{fstat, open_at, rename_between, stat_at},
    os, os_name,
    pyops::{self},
    sha256_hex,
    state::uuid4,
    tarball,
    volume::{self, data_volume, free, open_directory},
    SCOPES,
};
use crate::commands::reports::{object, Value, ValueExt};
use crate::commands::workload::Lease;
use crate::reply::{atomic_json, private_directory, Failure, Outcome, Reply};
use std::{ffi::CString, io::Read, os::fd::OwnedFd, path::Path};
use y2p_core::json::ObjectExt;
use y2p_core::{command, Root};

const SETTINGS: &str = "reborn/state/session.json";
/// The Last.fm sign-in and the plays waiting for it are settings too: a settings reset forgets them
/// (quarantined like other reset targets).
const LASTFM: [&str; 2] = ["reborn/state/lastfm.json", "reborn/state/scrobbles.jsonl"];
const DIRECTORY: i32 = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;

mod execute;
mod export;
mod plan;
mod purge;
use execute::execute;
use export::export;
use plan::{busy, confirmation, plan};
use purge::purge;

/// The answer: the result, or `{"state": "Failed", "failure": str(error)}` (exit 1).
fn answer(result: Result<Value, Failure>) -> Outcome {
    match result {
        Ok(value) => Ok(Reply::text(
            serde_json::to_string(&value).unwrap_or_default(),
        )),
        Err(error) => Reply::unsorted(
            &y2p_core::json::Object::new()
                .with("state", "Failed")
                .with("failure", error.message()),
            1,
        ),
    }
}

/// `reset plan|execute|resume|purge`.
pub fn reset(
    root: &Root,
    action: &str,
    scope: Option<&str>,
    confirm: Option<&str>,
    id: Option<&str>,
    erase: bool,
) -> Outcome {
    answer(match action {
        "plan" => plan(root, scope),
        "purge" => purge(root, id.unwrap_or(""), confirm),
        _ => execute(root, confirm, erase, action == "resume"),
    })
}

/// `export-state`.
pub fn export_state(root: &Root, database: bool, network: bool) -> Outcome {
    answer(export(root, database, network))
}

fn open_data(root: &Root) -> Result<OwnedFd, Failure> {
    let path = root.path("/data");
    open_at(None, &path.to_string_lossy(), DIRECTORY, 0).map_err(|e| os(e, &path))
}

fn dup(fd: &OwnedFd) -> Result<OwnedFd, Failure> {
    fd.try_clone().map_err(Failure::from)
}

/// `identity(meta)`.
fn identity(meta: &y2p_core::sys::Stat) -> Value {
    object()
        .with("device", Value::from(meta.dev))
        .with("inode", Value::from(meta.ino))
        .with("mode", Value::from(i64::from(meta.mode & libc::S_IFMT)))
        .with("size", Value::from(meta.size))
        .with(
            "mtime_ns",
            Value::from(
                y2p_core::wide(meta.mtime) * 1_000_000_000 + y2p_core::wide(meta.mtime_nsec),
            ),
        )
}

fn not_found(error: &Failure) -> bool {
    matches!(
        error,
        Failure::Os {
            class: "FileNotFoundError",
            ..
        }
    )
}

/// `parent_fd(data, relative)`.
fn parent_fd(data: &OwnedFd, relative: &str) -> Result<(OwnedFd, String), Failure> {
    let parts: Vec<&str> = relative.split('/').collect();
    if parts
        .iter()
        .any(|p| p.is_empty() || *p == "." || *p == "..")
    {
        return Err(Failure::value("invalid_maintenance_path"));
    }
    let mut fd = dup(data)?;
    for part in &parts[..parts.len() - 1] {
        fd = open_directory(&fd, part)?;
    }
    Ok((fd, parts[parts.len() - 1].to_owned()))
}

/// `inspect(data, relative)`.
fn inspect(data: &OwnedFd, relative: &str) -> Result<Option<Value>, Failure> {
    let (parent, name) = match parent_fd(data, relative) {
        Err(e) if not_found(&e) => return Ok(None),
        other => other?,
    };
    match stat_at(&parent, &name) {
        Err(e) if e.raw_os_error() == Some(libc::ENOENT) => Ok(None),
        Err(e) => Err(os_name(e, &name)),
        Ok(meta) => {
            let kind = meta.mode & libc::S_IFMT;
            if !(kind == libc::S_IFREG || kind == libc::S_IFDIR) || meta.dev != fstat(data)?.dev {
                return Err(Failure::value(format!(
                    "unsafe_maintenance_path:{relative}"
                )));
            }
            Ok(Some(identity(&meta)))
        }
    }
}

fn fsync(fd: &OwnedFd) -> Result<(), Failure> {
    Ok(y2p_core::sys::fsync(std::os::fd::AsFd::as_fd(fd))?)
}

fn fsync_path(path: &Path) -> Result<(), Failure> {
    let fd = open_at(
        None,
        &path.to_string_lossy(),
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        0,
    )
    .map_err(|e| os(e, path))?;
    fsync(&fd)
}

/// `regular_bytes(data, relative, maximum)`.
fn regular_bytes(data: &OwnedFd, relative: &str, maximum: u64) -> Result<Vec<u8>, Failure> {
    let (parent, name) = parent_fd(data, relative)?;
    let fd = open_at(
        Some(&parent),
        &name,
        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        0,
    )
    .map_err(|e| os_name(e, &name))?;
    drop(parent);
    let meta = fstat(&fd)?;
    if meta.mode & libc::S_IFMT != libc::S_IFREG || meta.nlink != 1 || meta.size as u64 > maximum {
        return Err(Failure::value(format!("export_file_bounds:{relative}")));
    }
    let mut value = Vec::new();
    std::fs::File::from(fd)
        .take(maximum + 1)
        .read_to_end(&mut value)?;
    if value.len() as u64 > maximum {
        return Err(Failure::value("export_file_grew"));
    }
    Ok(value)
}

fn text(value: Option<&Value>) -> Value {
    value.cloned().unwrap_or(Value::Null)
}

#[cfg(test)]
#[path = "maintenance_tests.rs"]
mod maintenance_tests;
