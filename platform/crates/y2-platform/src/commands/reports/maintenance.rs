//! maintenance.py: explicit owner reset and private state export; never format Y2DATA.
//!
//! Reset moves only reviewed user paths into a private recovery directory. Purge is a separate
//! confirmed, bounded operation. SSH/entropy/update/factory state stays. The plan's `confirmation`
//! and the settings result's sha256 are hashes over Python's exact `json.dumps` bytes (pyjson): a
//! plan Python made confirms here, and one made here confirms in Python.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    json_dict,
    media::{fstat, open_at, rename_between, stat_at},
    os, os_name, os_plain,
    pyjson::{self, Py, Style},
    pyops,
    state::uuid4,
    tarball,
    volume::{self, data_volume, free, open_directory},
    workload::Lease,
    SCOPES,
};
use crate::reply::{atomic_json, private_directory, Failure, Outcome, Reply};
use sha2::{Digest, Sha256};
use std::{
    ffi::CString,
    io::{Read, Write},
    os::fd::{AsFd, AsRawFd, OwnedFd},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};
use y2p_core::json::ObjectExt;
use y2p_core::{clock as kernel, command, Root};

const SETTINGS: &str = "reborn/state/session.json";
/// The Last.fm sign-in and the plays waiting for it are settings too: a settings reset forgets them
/// (quarantined like other reset targets).
const LASTFM: [&str; 2] = ["reborn/state/lastfm.json", "reborn/state/scrobbles.jsonl"];
const TERMINAL: [&str; 4] = ["Idle", "Acknowledged", "RolledBack", "Failed"];
const DIRECTORY: i32 = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `except (OSError, ValueError, TimeoutError)`: `{"state": "Failed", "failure": str(error)}`; any
/// other exception is main.py's traceback.
fn failed(error: Failure) -> Outcome {
    match error {
        error if !error.caught() => Err(error),
        error => Reply::unsorted(
            &y2p_core::json::Object::new()
                .with("state", "Failed")
                .with("failure", error.message()),
            1,
        ),
    }
}

fn answer(result: Result<Py, Failure>) -> Outcome {
    match result {
        Ok(value) => Ok(Reply::text(pyjson::dumps(&value, Style::Spaced))),
        Err(error) => failed(error),
    }
}

/// cli.py's `reset` branch.
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

/// cli.py's `export-state`.
pub fn export_state(root: &Root, database: bool, network: bool) -> Outcome {
    answer(export(root, database, network))
}

fn open_data(root: &Root) -> Result<OwnedFd, Failure> {
    let path = root.path("/data");
    open_at(None, &path.to_string_lossy(), DIRECTORY, 0).map_err(|e| os(e, &path))
}

fn dup(fd: &OwnedFd) -> Result<OwnedFd, Failure> {
    fd.try_clone().map_err(os_plain)
}

/// `identity(meta)`.
fn identity(meta: &y2p_core::sys::Stat) -> Py {
    Py::dict()
        .with("device", Py::from(meta.dev))
        .with("inode", Py::from(meta.ino))
        .with("mode", Py::int(i64::from(meta.mode & libc::S_IFMT)))
        .with("size", Py::int(meta.size))
        .with(
            "mtime_ns",
            Py::int(y2p_core::wide(meta.mtime) * 1_000_000_000 + y2p_core::wide(meta.mtime_nsec)),
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
fn inspect(data: &OwnedFd, relative: &str) -> Result<Option<Py>, Failure> {
    let (parent, name) = match parent_fd(data, relative) {
        Err(e) if not_found(&e) => return Ok(None),
        other => other?,
    };
    match stat_at(&parent, &name) {
        Err(e) if e.raw_os_error() == Some(libc::ENOENT) => Ok(None),
        Err(e) => Err(os_name(e, &name)),
        Ok(meta) => {
            let kind = meta.mode & libc::S_IFMT;
            if !(kind == libc::S_IFREG || kind == libc::S_IFDIR)
                || meta.dev != fstat(data).map_err(os_plain)?.dev
            {
                return Err(Failure::value(format!(
                    "unsafe_maintenance_path:{relative}"
                )));
            }
            Ok(Some(identity(&meta)))
        }
    }
}

fn services(scope: &str) -> Vec<Py> {
    let mut value = vec!["S90reborn", "S42y2-readiness"];
    if matches!(scope, "network" | "bluetooth-bonds" | "full-user") {
        value.extend(["S41y2-connectivity", "S40bluetoothd"]);
    }
    value.into_iter().map(Py::from).collect()
}

/// `Path(token).name`.
fn base_name(token: &str) -> &str {
    token
        .split('/')
        .rfind(|p| !p.is_empty() && *p != ".")
        .unwrap_or("")
}

/// `busy(ctx, scope)`: actual command lines, not stale PID files. Include supervision owners.
fn busy(root: &Root, scope: &str) -> Result<Vec<Py>, Failure> {
    let mut needles = vec![
        "reborn",
        "reborn-supervise",
        "service-daemon",
        "readiness-supervise",
    ];
    if matches!(scope, "network" | "bluetooth-bonds" | "full-user") {
        needles.extend([
            "connectivity",
            "wpa_supplicant",
            "udhcpc",
            "bluealsad",
            "bluetoothd",
            "y2-bt-reconnect",
        ]);
    }
    let proc = root.path("/proc");
    // pathlib's glob('[0-9]*/cmdline') in scandir order.
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&proc).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(|c: char| c.is_ascii_digit()) || !entry.path().is_dir() {
            continue;
        }
        if entry.path().join("cmdline").symlink_metadata().is_ok() {
            entries.push(name);
        }
    }
    if entries.len() > 4096 {
        return Err(Failure::value("process_inventory_limit"));
    }
    let own = std::process::id().to_string();
    let mut found = Vec::new();
    for name in entries {
        if name == own && root.is_real() {
            continue;
        }
        let path = proc.join(&name).join("cmdline");
        let mut raw = Vec::new();
        match std::fs::File::open(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(os(e, &path)),
            Ok(file) => {
                file.take(4096)
                    .read_to_end(&mut raw)
                    .map_err(|e| os(e, &path))?;
            }
        }
        let text = String::from_utf8_lossy(&raw);
        if text
            .split('\0')
            .filter(|t| !t.is_empty())
            .any(|t| needles.contains(&base_name(t)))
        {
            // `int(entry.parent.name)`: the glob only asks for a leading digit.
            found.push(pid(&name)?);
        }
    }
    Ok(found)
}

/// `int(name)` of a /proc entry name (whitespace around and single underscores between digits
/// allowed, any size), or its ValueError.
fn pid(name: &str) -> Result<Py, Failure> {
    let text = y2p_core::root::python_strip(name);
    let digits = text.strip_prefix('+').unwrap_or(text);
    let valid = !digits.is_empty()
        && !digits.starts_with('_')
        && !digits.ends_with('_')
        && !digits.contains("__")
        && digits.chars().all(|c| c.is_ascii_digit() || c == '_');
    if !valid {
        return Err(Failure::int_literal(name));
    }
    let digits = digits.replace('_', "");
    let trimmed = digits.trim_start_matches('0');
    Ok(Py::Int(
        if trimmed.is_empty() { "0" } else { trimmed }.to_owned(),
    ))
}

fn child_paths(root: &Root, relative: &str) -> Result<Vec<String>, Failure> {
    let directory = root.path(&format!("/data/{relative}"));
    if !directory.exists() {
        return Ok(Vec::new());
    }
    if !directory
        .symlink_metadata()
        .map_err(|e| os(e, &directory))?
        .is_dir()
    {
        return Err(Failure::value("unsafe_maintenance_directory"));
    }
    let mut names: Vec<String> = std::fs::read_dir(&directory)
        .map_err(|e| os(e, &directory))?
        .take(257)
        .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect::<Result<_, _>>()
        .map_err(|e| os(e, &directory))?;
    if names.len() > 256 {
        return Err(Failure::value("maintenance_directory_limit"));
    }
    names.sort();
    Ok(names
        .into_iter()
        .map(|n| format!("{relative}/{n}"))
        .collect())
}

fn paths(root: &Root, scope: &str) -> Result<Vec<String>, Failure> {
    let own = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    Ok(match scope {
        "settings" => own(&[SETTINGS, LASTFM[0], LASTFM[1]]),
        "network" => own(&["network"]),
        "bluetooth-bonds" => {
            let mut v = own(&["bluetooth/preferred-audio"]);
            v.extend(child_paths(root, "bluetooth/bluez")?);
            v
        }
        "library" => own(&[
            "reborn/library.db",
            "reborn/library.db-wal",
            "reborn/library.db-shm",
            "reborn/library.db-journal",
        ]),
        "caches" => own(&["reborn/cache", "cache"]),
        // Known user-owned state only. Unknown/factory/calibration/private platform directories are
        // deliberately excluded, even in this explicit full reset.
        _ => {
            let mut v = own(&[
                "reborn",
                "music",
                "network",
                "settings",
                "apps",
                "y2player",
                "cache",
                "logs",
                "bluetooth/enabled",
                "bluetooth/preferred-audio",
            ]);
            v.extend(child_paths(root, "bluetooth/bluez")?);
            v.extend(child_paths(root, "bluetooth/bluealsa")?);
            v
        }
    })
}

fn confirmation(value: &Py) -> String {
    hex(&Sha256::digest(
        pyjson::dumps(value, Style::Compact).as_bytes(),
    ))
}

/// `plan(ctx, scope)`.
fn plan(root: &Root, scope: Option<&str>) -> Result<Py, Failure> {
    let scope = scope
        .filter(|s| SCOPES.contains(s))
        .ok_or_else(|| Failure::value("unknown_reset_scope"))?;
    let volume = data_volume(root)?;
    if !TERMINAL.contains(&volume::update_state(root)?.as_str()) {
        return Err(Failure::value("update_in_progress"));
    }
    if root
        .path("/data/system/platform/maintenance-pending")
        .exists()
    {
        return Err(Failure::value(
            "resume_existing_reset_before_planning_another",
        ));
    }
    let live = busy(root, scope)?;
    let preserved: Vec<Py> = [
        "SSH identity",
        "entropy",
        "update/rollback",
        "factory/calibration",
        "unknown data directories",
        "Y2Media internal music (/media/internal; delete files over MTP)",
    ]
    .into_iter()
    .map(Py::from)
    .collect();
    let requirements = |value: Py| {
        value
            .with("stop_services", Py::List(services(scope)))
            .with("restart", Py::str("owner reboot after completion"))
            .with("preserved", Py::List(preserved.clone()))
    };
    if !live.is_empty() {
        return Ok(requirements(
            Py::dict()
                .with("state", Py::str("NeedsQuiescence"))
                .with("scope", Py::str(scope))
                .with("pids", Py::List(live)),
        ));
    }
    let data = open_data(root)?;
    let mut targets = Vec::new();
    for path in paths(root, scope)? {
        if let Some(meta) = inspect(&data, &path)? {
            targets.push(
                Py::dict()
                    .with("path", Py::Str(path))
                    .with("identity", meta),
            );
        }
    }
    drop(data);
    let mut value = requirements(
        Py::dict()
            .with("schema", Py::int(1))
            .with("id", Py::Str(uuid4()?.replace('-', "")))
            .with("state", Py::str("Planned"))
            .with("scope", Py::str(scope))
            .with("boot_id", root.boot_id().into())
            .with("data_generation", volume.generation.clone().into())
            .with("data_uuid", volume.uuid.clone().into())
            .with(
                "expires_monotonic_s",
                Py::Float(kernel::monotonic() + 300.0),
            )
            .with("targets", Py::List(targets)),
    );
    let signature = confirmation(&value);
    value.set("confirmation", Py::Str(signature));
    let platform = root.path("/data/system/platform");
    private_directory(&platform)?;
    atomic_json(&platform.join("reset-plan.json"), &value, true)?;
    Ok(value)
}

fn fsync(fd: &OwnedFd) -> Result<(), Failure> {
    // SAFETY: a valid descriptor.
    if unsafe { libc::fsync(fd.as_raw_fd()) } != 0 {
        return Err(os_plain(std::io::Error::last_os_error()));
    }
    Ok(())
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

/// `durable_flag(path)`.
fn durable_flag(path: &Path) -> Result<(), Failure> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| os(e, path))?;
    file.write_all(b"Owner maintenance pending; keep mutable consumers stopped\n")
        .and_then(|()| file.sync_all())
        .map_err(os_plain)?;
    fsync_path(path.parent().expect("parent"))
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
    let meta = fstat(&fd).map_err(os_plain)?;
    if meta.mode & libc::S_IFMT != libc::S_IFREG || meta.nlink != 1 || meta.size as u64 > maximum {
        return Err(Failure::value(format!("export_file_bounds:{relative}")));
    }
    let mut value = Vec::new();
    std::fs::File::from(fd)
        .take(maximum + 1)
        .read_to_end(&mut value)
        .map_err(os_plain)?;
    if value.len() as u64 > maximum {
        return Err(Failure::value("export_file_grew"));
    }
    Ok(value)
}

fn text(value: Option<&Py>) -> Py {
    value.cloned().unwrap_or(Py::None)
}

/// A plan target's path, identity and what is there now, in Python's order: `inspect(data,
/// target['path']) != target['identity']` before the apply, `relative, expected = target['path'],
/// target['identity']` then `inspect(data, relative)` in it (a path that is no str is an
/// AttributeError at its `.split`).
fn target(
    data: &OwnedFd,
    target: &Py,
    identity_first: bool,
) -> Result<(String, Py, Option<Py>), Failure> {
    let path = pyops::index(target, "path")?.clone();
    let expected = if identity_first {
        Some(pyops::index(target, "identity")?.clone())
    } else {
        None
    };
    let Py::Str(path) = path else {
        return Err(pyops::no_attribute(&path, "split"));
    };
    let source = inspect(data, &path)?;
    let expected = match expected {
        Some(expected) => expected,
        None => pyops::index(target, "identity")?.clone(),
    };
    Ok((path, expected, source))
}

/// `execute(ctx, confirmation, erase_music, resume)`.
fn execute(root: &Root, confirm: Option<&str>, erase: bool, resume: bool) -> Result<Py, Failure> {
    let platform = root.path("/data/system/platform");
    private_directory(&platform)?;
    let lock_path = platform.join(".maintenance.lock");
    let _lock = y2p_core::lock::try_exclusive(&lock_path)
        .map_err(|e| os(e, &lock_path))?
        .ok_or_else(|| os_plain(std::io::Error::from_raw_os_error(libc::EAGAIN)))?;
    let value = json_dict(root, "/data/system/platform/reset-plan.json");
    let required = || Failure::value("exact_reset_confirmation_required");
    // `not confirmation or value.get('confirmation') != confirmation or not re.fullmatch(...,
    // value.get('id', ''))`: an id that is no str is a TypeError once the confirmation matched.
    let confirmation_given = confirm
        .filter(|c| !c.is_empty())
        .filter(|c| {
            value
                .get("confirmation")
                .is_some_and(|v| v.py_eq(&Py::str(*c)))
        })
        .ok_or_else(required)?;
    let id = match value.get("id") {
        None => String::new(),
        Some(Py::Str(id)) => id.clone(),
        Some(other) => {
            return Err(Failure::type_error(format!(
                "expected string or bytes-like object, got '{}'",
                pyops::type_name(other)
            )))
        }
    };
    let id_ok = id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !id_ok {
        return Err(required());
    }
    let mut signed = value.clone();
    signed.remove("confirmation");
    if confirmation(&signed) != confirmation_given {
        return Err(Failure::value("reset_plan_changed"));
    }
    let scope = value
        .get("scope")
        .and_then(Py::as_str)
        .unwrap_or("")
        .to_owned();
    if !SCOPES.contains(&scope.as_str())
        || (!resume && !text(value.get("boot_id")).py_eq(&root.boot_id().into()))
    {
        return Err(Failure::value("stale_reset_plan"));
    }
    if scope == "full-user" && !erase {
        return Err(Failure::value(
            "full_reset_requires_erase_user_music_confirmation",
        ));
    }
    if !resume {
        // `time.monotonic() > value['expires_monotonic_s']`.
        let expires = pyops::index(&value, "expires_monotonic_s")?;
        let (now, expires) = pyops::ordered(&Py::Float(kernel::monotonic()), ">", expires)?;
        if now > expires {
            return Err(Failure::value("reset_plan_expired"));
        }
    }
    if !busy(root, &scope)?.is_empty() {
        return Err(Failure::value("reset_consumers_still_running"));
    }
    let volume = data_volume(root)?;
    let changed = if resume {
        let uuid = value.get("data_uuid").filter(|u| u.truthy());
        uuid.is_none_or(|u| !u.py_eq(&volume.uuid.clone().into()))
    } else {
        !pyops::index(&value, "data_generation")?.py_eq(&volume.generation.clone().into())
    };
    if changed {
        return Err(Failure::value("reset_data_generation_changed"));
    }
    if !TERMINAL.contains(&volume::update_state(root)?.as_str()) {
        return Err(Failure::value("update_in_progress"));
    }
    let data = open_data(root)?;
    let finished = json_dict(root, "/data/system/platform/maintenance-state.json");
    if finished
        .get("id")
        .is_some_and(|f| f.py_eq(&Py::Str(id.clone())))
        && finished
            .get("state")
            .is_some_and(|s| s.py_eq(&Py::str("Complete")))
    {
        crate::reply::unlink_missing_ok(&platform.join("maintenance-pending"))?;
        return Ok(finished);
    }
    // `value['targets']`, iterated (a resume reads it first in the loop below, after the pending flag
    // and the record are written).
    let targets = || pyops::iterate(pyops::index(&value, "targets")?);
    if !resume {
        for item in targets()? {
            let (_, expected, now) = target(&data, &item, false)?;
            if !now.unwrap_or(Py::None).py_eq(&expected) {
                return Err(Failure::value("reset_target_changed_before_apply"));
            }
        }
    }
    let quarantine_root = root.path("/data/reset-quarantine");
    private_directory(&quarantine_root)?;
    let location = quarantine_root.join(&id);
    match std::fs::DirBuilder::new().mode(0o700).create(&location) {
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && resume && location.is_dir() => {}
        Err(e) => return Err(os(e, &location)),
        Ok(()) => {}
    }
    let quarantine =
        open_at(None, &location.to_string_lossy(), DIRECTORY, 0).map_err(|e| os(e, &location))?;
    if fstat(&quarantine).map_err(os_plain)?.dev != fstat(&data).map_err(os_plain)?.dev {
        return Err(Failure::value("reset_quarantine_submount"));
    }
    let mut completed: Vec<Py> = Vec::new();
    let progress = |completed: &[Py], state: &str| {
        Py::dict()
            .with("schema", Py::int(1))
            .with("id", Py::Str(id.clone()))
            .with("scope", Py::Str(scope.clone()))
            .with("state", Py::str(state))
            .with("completed", Py::List(completed.to_vec()))
    };
    durable_flag(&platform.join("maintenance-pending"))?;
    atomic_json(
        &location.join("record.json"),
        &progress(&completed, "Applying"),
        true,
    )?;
    for (index, item) in targets()?.iter().enumerate() {
        let (relative, expected, source) = target(&data, item, true)?;
        let saved = format!("{index:03}");
        let old = match stat_at(&quarantine, &saved) {
            Ok(meta) => Some(identity(&meta)),
            Err(e) if e.raw_os_error() == Some(libc::ENOENT) => None,
            Err(e) => return Err(os_name(e, &saved)),
        };
        let field = |v: &Py, k: &str| v.get(k).cloned().unwrap_or(Py::None);
        let same_source = source.as_ref().is_some_and(|s| s.py_eq(&expected));
        if scope == "settings" && relative == SETTINGS {
            // `source != expected and not (resume and old and old['inode'] == expected['inode'])`.
            let old_same_inode = || -> Result<bool, Failure> {
                Ok(match &old {
                    Some(o) if resume => field(o, "inode").py_eq(pyops::index(&expected, "inode")?),
                    _ => false,
                })
            };
            if !same_source && !old_same_inode()? {
                // Settings backup is a copy, handled by completed hash below.
                let record = super::json_at(
                    root,
                    &format!("/data/reset-quarantine/{id}/settings-result.json"),
                )
                .filter(|_| location.join("settings-result.json").exists())
                .unwrap_or(Py::dict());
                let sha = record.get("sha256").cloned();
                if resume {
                    let current = hex(&Sha256::digest(regular_bytes(&data, &relative, 8 << 20)?));
                    if sha.is_some_and(|s| s.py_eq(&Py::Str(current))) {
                        completed.push(Py::Str(relative.clone()));
                        continue;
                    }
                }
                return Err(Failure::value("reset_settings_changed"));
            }
            let raw = regular_bytes(&data, &relative, 8 << 20)?;
            let mut model = pyops::loads(&raw)?;
            // `'schema' in model`, then `model.get('schema')`.
            let wrapped = pyops::contains(&model, "schema")?;
            if wrapped {
                pyops::get(&model, "schema")?;
            }
            // Schema 3 (Reborn Beta 8.33) stores the queue by reference; only the settings change
            // here, the rest is kept as read.
            if wrapped
                && !model
                    .get("schema")
                    .is_some_and(|s| s.py_eq(&Py::int(2)) || s.py_eq(&Py::int(3)))
            {
                return Err(Failure::value("unsupported_session_schema"));
            }
            let body_ok = {
                let body = if wrapped {
                    model.get("model")
                } else {
                    Some(&model)
                };
                body.is_some_and(|b| b.is_dict() && b.get("settings").is_some())
            };
            if !body_ok {
                return Err(Failure::value("invalid_saved_session"));
            }
            let reply = command::run(root, &["/usr/bin/reborn", "--default-settings"], 2.0);
            let defaults = if reply.ok {
                pyops::loads(reply.output.clone().unwrap_or_default().as_bytes())?
            } else {
                Py::dict()
            };
            let schema_ok = pyops::get(&defaults, "session_schema")?
                .is_some_and(|s| s.py_eq(&Py::int(2)) || s.py_eq(&Py::int(3)));
            let Some(settings @ Py::Dict(_)) =
                defaults.get("settings").cloned().filter(|_| schema_ok)
            else {
                return Err(Failure::value("application_defaults_unavailable"));
            };
            if old.is_none() {
                free(&quarantine, raw.len() as u64 * 2)?;
                let fd = open_at(
                    Some(&quarantine),
                    &saved,
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    0o600,
                )
                .map_err(|e| os_name(e, &saved))?;
                let mut backup = std::fs::File::from(fd);
                backup
                    .write_all(&raw)
                    .and_then(|()| backup.sync_all())
                    .map_err(os_plain)?;
                fsync(&quarantine)?;
            }
            let body = if wrapped {
                model.get_mut("model").expect("checked")
            } else {
                &mut model
            };
            body.set("settings", settings);
            // atomic_json's exact canonical encoding, including its newline.
            let result =
                pyjson::dumps_strict(&model, Style::Compact).map_err(Failure::Value)? + "\n";
            atomic_json(
                &location.join("settings-result.json"),
                &Py::dict().with("sha256", Py::Str(hex(&Sha256::digest(result.as_bytes())))),
                true,
            )?;
            atomic_json(&root.path(&format!("/data/{relative}")), &model, true)?;
        } else {
            // `old and source is None and old['device'] == expected['device'] and old['inode'] ==
            // expected['inode']`.
            let renamed_before = match (&old, &source) {
                (Some(o), None) => {
                    field(o, "device").py_eq(pyops::index(&expected, "device")?)
                        && field(o, "inode").py_eq(pyops::index(&expected, "inode")?)
                }
                _ => false,
            };
            if renamed_before {
                // Durable rename completed before an interrupted journal update.
            } else if old.is_some() || !same_source {
                return Err(Failure::value(format!("reset_target_changed:{relative}")));
            } else {
                let (parent, name) = parent_fd(&data, &relative)?;
                #[cfg(test)]
                seams::rename()?;
                rename_between(&parent, &name, &quarantine, &saved)
                    .map_err(|e| os_name(e, &name))?;
                fsync(&parent)?;
                fsync(&quarantine)?;
            }
        }
        completed.push(Py::Str(relative));
        atomic_json(
            &location.join("record.json"),
            &progress(&completed, "Applying"),
            true,
        )?;
    }
    if data_volume(root)?.generation != volume.generation {
        return Err(Failure::value("reset_data_generation_changed"));
    }
    let record = progress(&completed, "Complete")
        .with("preserved", pyops::index(&value, "preserved")?.clone())
        .with(
            "quarantine",
            Py::Str(format!("/data/reset-quarantine/{id}")),
        )
        .with("purge_confirmation", Py::str(confirmation_given));
    atomic_json(&location.join("record.json"), &record, true)?;
    atomic_json(&platform.join("maintenance-state.json"), &record, true)?;
    let pending = platform.join("maintenance-pending");
    std::fs::remove_file(&pending).map_err(|e| os(e, &pending))?;
    fsync_path(&platform)?;
    Ok(record)
}

/// The names in a directory, in readdir order (`os.scandir(fd)`).
fn entries(fd: &OwnedFd) -> Result<Vec<String>, Failure> {
    let copy = dup(fd)?;
    let raw = copy.as_raw_fd();
    std::mem::forget(copy);
    // SAFETY: fdopendir takes ownership of the duplicated descriptor; closedir releases it.
    let dir = unsafe { libc::fdopendir(raw) };
    if dir.is_null() {
        return Err(os_plain(std::io::Error::last_os_error()));
    }
    // SAFETY: rewind to the start as scandir's fresh stream.
    unsafe { libc::rewinddir(dir) };
    let mut names = Vec::new();
    loop {
        // SAFETY: a valid DIR stream.
        let entry = unsafe { libc::readdir(dir) };
        if entry.is_null() {
            break;
        }
        // SAFETY: d_name is NUL-terminated.
        let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        if name != "." && name != ".." {
            names.push(name);
        }
    }
    // SAFETY: the stream is closed once.
    unsafe { libc::closedir(dir) };
    Ok(names)
}

fn three_digits(name: &str) -> bool {
    name.len() == 3 && name.bytes().all(|b| b.is_ascii_digit())
}

/// `purge(ctx, token, confirmation)` (Maintenance workload).
fn purge(root: &Root, token: &str, confirm: Option<&str>) -> Result<Py, Failure> {
    let id_ok = token.len() == 32
        && token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    let _lease = Lease::take(root, "Maintenance")?;
    if !id_ok {
        return Err(Failure::value("invalid_reset_id"));
    }
    data_volume(root)?;
    let data = open_data(root)?;
    let parent = open_directory(&data, "reset-quarantine")?;
    let top = open_directory(&parent, token)?;
    drop((parent, data));
    // `json.loads(...)` (its JSONDecodeError is a Failed answer), then `record.get(...)` (an
    // AttributeError for a record that is no dict).
    let record = pyops::loads(&regular_bytes(&top, "record.json", 16384)?)?;
    if !pyops::get(&record, "id")?.is_some_and(|v| v.py_eq(&Py::str(token)))
        || !record
            .get("state")
            .is_some_and(|v| v.py_eq(&Py::str("Complete")))
        || !text(record.get("purge_confirmation")).py_eq(&confirm.map(Py::str).unwrap_or(Py::None))
    {
        return Err(Failure::value("completed_reset_confirmation_required"));
    }
    if !y2p_core::lock::try_fd(top.as_fd(), y2p_core::lock::Kind::Exclusive).map_err(os_plain)? {
        return Err(os_plain(y2p_core::lock::would_block()));
    }
    let device = fstat(&top).map_err(os_plain)?.dev;
    let deadline = kernel::monotonic() + 8.0;
    let mut removed = 0usize;
    fn walk(
        fd: &OwnedFd,
        depth: usize,
        device: u64,
        deadline: f64,
        removed: &mut usize,
    ) -> Result<bool, Failure> {
        if depth > 16 {
            return Err(Failure::value("purge_depth_limit"));
        }
        for name in entries(fd)? {
            if depth == 0 && !three_digits(&name) {
                continue;
            }
            if *removed >= 256 || kernel::monotonic() >= deadline {
                return Ok(false);
            }
            let meta = stat_at(fd, &name).map_err(|e| os_name(e, &name))?;
            if meta.dev != device {
                return Err(Failure::value("purge_submount_refused"));
            }
            let c = CString::new(name.as_str()).expect("no NUL");
            if meta.mode & libc::S_IFMT == libc::S_IFDIR {
                let child = open_directory(fd, &name)?;
                let complete = walk(&child, depth + 1, device, deadline, removed)?;
                drop(child);
                if !complete {
                    return Ok(false);
                }
                // SAFETY: a valid fd and name.
                if unsafe { libc::unlinkat(fd.as_raw_fd(), c.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
                    return Err(os_name(std::io::Error::last_os_error(), &name));
                }
            } else {
                // Symlinks inside user content are unlinked, never followed.
                // SAFETY: a valid fd and name.
                if unsafe { libc::unlinkat(fd.as_raw_fd(), c.as_ptr(), 0) } != 0 {
                    return Err(os_name(std::io::Error::last_os_error(), &name));
                }
            }
            *removed += 1;
        }
        fsync(fd)?;
        Ok(true)
    }
    let complete = walk(&top, 0, device, deadline, &mut removed)?;
    Ok(Py::dict()
        .with(
            "state",
            Py::str(if complete {
                "Complete"
            } else {
                "MoreRemaining"
            }),
        )
        .with("removed", Py::from(removed))
        .with("id", Py::str(token)))
}

/// `urllib.parse.quote_from_bytes(path)` with `/` kept (`Path.as_uri()`).
fn quote(path: &str) -> String {
    path.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"_.-~/".contains(&b) {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// `tarfile.add(path, arcname)` metadata: the file's own (user and group names by lookup, mtime a
/// float as os.stat gives it).
fn tar_info(path: &Path, name: &str) -> Result<tarball::Info, Failure> {
    let meta = path.symlink_metadata().map_err(|e| os(e, path))?;
    let lookup = |uid: bool, id: u32| -> String {
        // SAFETY: getpwuid/getgrgid return static records or NULL.
        unsafe {
            let name = if uid {
                let p = libc::getpwuid(id);
                if p.is_null() {
                    std::ptr::null()
                } else {
                    (*p).pw_name
                }
            } else {
                let g = libc::getgrgid(id);
                if g.is_null() {
                    std::ptr::null()
                } else {
                    (*g).gr_name
                }
            };
            if name.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(name)
                    .to_string_lossy()
                    .into_owned()
            }
        }
    };
    Ok(tarball::Info {
        name: name.to_owned(),
        mode: meta.mode(),
        uid: u64::from(meta.uid()),
        gid: u64::from(meta.gid()),
        size: meta.len(),
        mtime: tarball::Mtime::Float(meta.mtime() as f64 + meta.mtime_nsec() as f64 * 1e-9),
        uname: lookup(true, meta.uid()),
        gname: lookup(false, meta.gid()),
    })
}

fn file_digest(path: &Path) -> Result<String, Failure> {
    let data = std::fs::read(path).map_err(|e| os(e, path))?;
    Ok(hex(&Sha256::digest(&data)))
}

/// `export(ctx, include_database, include_network)` (Maintenance workload).
fn export(root: &Root, include_database: bool, include_network: bool) -> Result<Py, Failure> {
    let _lease = Lease::take(root, "Maintenance")?;
    let volume = data_volume(root)?;
    let directory = root.path("/data/exports");
    private_directory(&directory)?;
    let token = uuid4()?.replace('-', "");
    let scratch = directory.join(format!(".export-{token}"));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&scratch)
        .map_err(|e| os(e, &scratch))?;
    let data = match open_data(root) {
        Ok(data) => data,
        Err(error) => {
            let _ = std::fs::remove_dir(&scratch);
            return Err(error);
        }
    };
    let result = export_into(
        root,
        &data,
        &directory,
        &scratch,
        &token,
        &volume,
        include_database,
        include_network,
    );
    drop(data);
    for entry in std::fs::read_dir(&scratch).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.symlink_metadata().is_ok_and(|m| m.is_file()) {
            let _ = std::fs::remove_file(&path);
        }
    }
    std::fs::remove_dir(&scratch)
        .map_err(|e| os(e, &scratch))
        .and(result)
}

#[allow(clippy::too_many_arguments)]
fn export_into(
    root: &Root,
    data: &OwnedFd,
    directory: &Path,
    scratch: &Path,
    token: &str,
    volume: &volume::Volume,
    include_database: bool,
    include_network: bool,
) -> Result<Py, Failure> {
    let mut files: Vec<(String, String)> = Vec::new();
    let mut absent: Vec<Py> = Vec::new();
    let mut sources = vec![SETTINGS, "bluetooth/preferred-audio"];
    if include_network {
        sources.extend(["network/wpa_supplicant.conf", "network/enabled"]);
    }
    for relative in sources {
        let raw = match regular_bytes(data, relative, 8 << 20) {
            Err(e) if not_found(&e) => {
                absent.push(Py::str(relative));
                continue;
            }
            other => other?,
        };
        free(data, raw.len() as u64)?;
        let name = relative.replace('/', "--");
        let path = scratch.join(&name);
        std::fs::write(&path, &raw).map_err(|e| os(e, &path))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| os(e, &path))?;
        files.push((name, relative.to_owned()));
    }
    if include_database {
        let relative = "reborn/library.db";
        match inspect(data, relative)? {
            None => absent.push(Py::str(relative)),
            Some(before) => {
                use y2p_core::reports_sqlite::{Database, Error, Value};
                let database = root.path(&format!("/data/{relative}"));
                let target = scratch.join("library.db");
                let deadline = kernel::monotonic() + 30.0;
                let sqlite = |error: Error| match error {
                    Error::Callback(text) => Failure::Value(text),
                    other => Failure::value(format!("sqlite3.{}", other.class())),
                };
                {
                    let source = Database::open(
                        &format!("file://{}?mode=ro", quote(&database.to_string_lossy())),
                        500,
                    )
                    .map_err(sqlite)?;
                    let dest = Database::open(&target.to_string_lossy(), 5000).map_err(sqlite)?;
                    source
                        .backup(&dest, 128, 50, |_, _, total| {
                            if kernel::monotonic() > deadline || i64::from(total) * 4096 > 128 << 20
                            {
                                return Err("database_export_limit".into());
                            }
                            free(data, 1 << 20).map_err(|e| e.message())
                        })
                        .map_err(sqlite)?;
                    let check = dest.query("PRAGMA quick_check").map_err(sqlite)?;
                    if check.first().map(Vec::as_slice) != Some(&[Value::Text("ok".into())][..]) {
                        return Err(Failure::value("database_backup_integrity"));
                    }
                }
                let after = inspect(data, relative)?;
                let key = |v: &Py| (v.get("device").cloned(), v.get("inode").cloned());
                if after.as_ref().is_none_or(|a| key(a) != key(&before)) {
                    return Err(Failure::value("database_source_replaced"));
                }
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600))
                    .map_err(|e| os(e, &target))?;
                files.push(("library.db".into(), relative.into()));
            }
        }
    }
    let record =
        y2p_core::devices_support::record(root, "owner-state-export", serde_json::json!({}));
    let record = Py::from_serde(&serde_json::to_value(&record).map_err(Failure::value)?);
    let mut listed = Vec::new();
    for (name, relative) in &files {
        let path = scratch.join(name);
        listed.push(
            Py::dict()
                .with("name", Py::str(name.clone()))
                .with("original", Py::str(relative.clone()))
                .with("sha256", Py::Str(file_digest(&path)?))
                .with(
                    "bytes",
                    Py::from(std::fs::metadata(&path).map_err(|e| os(e, &path))?.len()),
                ),
        );
    }
    let manifest = Py::dict()
        .with("schema", Py::int(1))
        .with("record", record)
        .with("contains_network_credentials", Py::Bool(include_network))
        .with("excludes_bonds_and_private_keys", Py::Bool(true))
        .with(
            "consistency",
            Py::str("SQLite online backup; each session file atomic; no cross-file transaction"),
        )
        .with("files", Py::List(listed))
        .with("absent", Py::List(absent));
    atomic_json(&scratch.join("manifest.json"), &manifest, true)?;
    let output = directory.join(format!(".state-{token}.partial"));
    let mut stream = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&output)
        .map_err(|e| os(e, &output))?;
    let mut tar = tarball::Writer::new();
    let names: Vec<String> = std::iter::once("manifest.json".to_owned())
        .chain(files.iter().map(|(n, _)| n.clone()))
        .collect();
    for name in &names {
        let path = scratch.join(name);
        free(
            data,
            std::fs::metadata(&path).map_err(|e| os(e, &path))?.len(),
        )?;
        let body = std::fs::read(&path).map_err(|e| os(e, &path))?;
        tar.add(&tar_info(&path, name)?, &body);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as u32);
    // tarfile 'w:gz': GzipFile at level 9 with the current time in its header.
    stream
        .write_all(&tarball::gzip(&tar.finish(), 9, now))
        .and_then(|()| stream.sync_all())
        .map_err(os_plain)?;
    drop(stream);
    if data_volume(root)?.generation != volume.generation {
        return Err(Failure::value("export_data_generation_changed"));
    }
    let complete = directory.join(format!("state-{token}.tar.gz"));
    std::fs::rename(&output, &complete).map_err(|e| os(e, &output))?;
    fsync_path(directory)?;
    let digest = file_digest(&complete)?;
    let bytes = std::fs::metadata(&complete)
        .map_err(|e| os(e, &complete))?
        .len();
    Ok(Py::dict()
        .with("state", Py::str("Complete"))
        .with(
            "path",
            Py::Str(format!("/data/exports/state-{token}.tar.gz")),
        )
        .with("sha256", Py::Str(digest))
        .with("bytes", Py::from(bytes))
        .with("contains_network_credentials", Py::Bool(include_network)))
}

/// What tests/test_platform_maintenance.py patched (`os.rename` failing once), per thread and only in
/// unit tests.
#[cfg(test)]
mod seams {
    use crate::reply::Failure;
    use std::cell::Cell;

    thread_local! {
        /// The reset target rename (counted from 1) that fails like an interrupted reset.
        pub static FAIL_RENAME: Cell<Option<u32>> = const { Cell::new(None) };
        static RENAMES: Cell<u32> = const { Cell::new(0) };
    }

    pub fn rename() -> Result<(), Failure> {
        let count = RENAMES.with(|r| {
            r.set(r.get() + 1);
            r.get()
        });
        if FAIL_RENAME.with(Cell::get) == Some(count) {
            return Err(Failure::os_message("injected interruption"));
        }
        Ok(())
    }

    pub fn reset() {
        FAIL_RENAME.with(|f| f.set(None));
        RENAMES.with(|r| r.set(0));
    }
}

#[cfg(test)]
#[path = "maintenance_tests.rs"]
mod maintenance_tests;
