//! `reset execute|resume`: move a confirmed plan's targets into the quarantine, durably and in
//! order, so an interrupted reset resumes where it stopped (also after a restart).
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    atomic_json, busy, command, confirmation, data_volume, free, fstat, fsync, fsync_path,
    identity, inspect, json_dict, open_at, open_data, os, os_name, parent_fd, private_directory,
    pyjson, pyops, regular_bytes, rename_between, sha256_hex, stat_at, text, volume, Failure,
    OwnedFd, Path, Py, Root, Style, DIRECTORY, SCOPES, SETTINGS,
};
use crate::commands::reports::pyops::OrRefuse;
use crate::commands::service::update::JournalState;
use std::io::Write;
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::fs::OpenOptionsExt;

/// A reset plan field missing or of another type (the confirmation matched, so only a plan written
/// by hand gets here).
const PLAN_UNREADABLE: &str = "reset_plan_unreadable";

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
        .and_then(|()| file.sync_all())?;
    match path.parent() {
        Some(parent) => fsync_path(parent),
        None => Ok(()),
    }
}

/// A plan target's path, identity and what is there now. Before the apply the path is inspected
/// before the identity is read, in it the other way round, so an odd target is refused at the same
/// step as before; a target without both, or whose path is no str, is refused.
fn target(
    data: &OwnedFd,
    target: &Py,
    identity_first: bool,
) -> Result<(String, Py, Option<Py>), Failure> {
    let field = |key| {
        pyops::index(target, key)
            .or_refuse(PLAN_UNREADABLE)
            .cloned()
    };
    let path = field("path")?;
    let expected = if identity_first {
        Some(field("identity")?)
    } else {
        None
    };
    let Py::Str(path) = path else {
        return Err(Failure::value(PLAN_UNREADABLE));
    };
    let source = inspect(data, &path)?;
    let expected = match expected {
        Some(expected) => expected,
        None => field("identity")?,
    };
    Ok((path, expected, source))
}

/// A plan that passed every check before anything changes.
struct Admitted<'a> {
    /// The confirmation the person gave (the plan's own).
    confirmation: &'a str,
    id: String,
    scope: String,
    /// The data volume the plan was made for.
    volume: y2p_core::volume::Volume,
}

/// The checks before anything changes, in this order: the exact confirmation and plan id, the plan
/// unchanged since it was confirmed, its scope and boot (a resume may cross a restart), the erase
/// confirmation for a full reset, its expiry (not on resume), no consumer still running, the same
/// data volume, no update in progress.
fn admit<'a>(
    root: &Root,
    value: &Py,
    confirm: Option<&'a str>,
    erase: bool,
    resume: bool,
) -> Result<Admitted<'a>, Failure> {
    let required = || Failure::value("exact_reset_confirmation_required");
    // `not confirmation or value.get('confirmation') != confirmation or not re.fullmatch(...,
    // value.get('id', ''))`: an id that is no str is refused once the confirmation matched.
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
        Some(_) => return Err(Failure::value(PLAN_UNREADABLE)),
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
        let (now, expires) = pyops::index(value, "expires_monotonic_s")
            .and_then(|expires| pyops::ordered(&Py::Float(root.clock().monotonic()), expires))
            .or_refuse(PLAN_UNREADABLE)?;
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
        !pyops::index(value, "data_generation")
            .or_refuse(PLAN_UNREADABLE)?
            .py_eq(&volume.generation.clone().into())
    };
    if changed {
        return Err(Failure::value("reset_data_generation_changed"));
    }
    if !JournalState::named(Some(&volume::update_state(root)?)).terminal() {
        return Err(Failure::value("update_in_progress"));
    }
    Ok(Admitted {
        confirmation: confirmation_given,
        id,
        scope,
        volume,
    })
}

/// What became of one target.
enum Moved {
    /// Moved (or its settings reset) now: recorded as completed.
    Now,
    /// A resume found it done before the interruption: completed, nothing written.
    Before,
}

/// One plan target in quarantine order.
struct Target<'a> {
    relative: String,
    expected: Py,
    /// What is at the path now (None: nothing).
    source: Option<Py>,
    /// The quarantine entry's name (`000`, `001`, …) and what is already there.
    saved: &'a str,
    old: Option<Py>,
}

impl Target<'_> {
    fn same_source(&self) -> bool {
        self.source
            .as_ref()
            .is_some_and(|s| s.py_eq(&self.expected))
    }

    fn planned(&self, key: &str) -> Result<&Py, Failure> {
        pyops::index(&self.expected, key).or_refuse(PLAN_UNREADABLE)
    }
}

fn field(value: &Py, key: &str) -> Py {
    value.get(key).cloned().unwrap_or(Py::None)
}

/// The settings scope's session file: a copy kept in quarantine, then its settings replaced by
/// Reborn's defaults (the rest of the session kept as read); a resume whose result is already in
/// place is done before.
fn reset_settings(
    root: &Root,
    data: &OwnedFd,
    quarantine: &OwnedFd,
    location: &Path,
    id: &str,
    target: &Target,
    resume: bool,
) -> Result<Moved, Failure> {
    let relative = &target.relative;
    // `source != expected and not (resume and old and old['inode'] == expected['inode'])`.
    let old_same_inode = || -> Result<bool, Failure> {
        Ok(match &target.old {
            Some(o) if resume => field(o, "inode").py_eq(target.planned("inode")?),
            _ => false,
        })
    };
    if !target.same_source() && !old_same_inode()? {
        // Settings backup is a copy, handled by completed hash below.
        let record = super::super::json_at(
            root,
            &format!("/data/reset-quarantine/{id}/settings-result.json"),
        )
        .filter(|_| location.join("settings-result.json").exists())
        .unwrap_or(Py::dict());
        let sha = record.get("sha256").cloned();
        if resume {
            let current = sha256_hex(&regular_bytes(data, relative, 8 << 20)?);
            if sha.is_some_and(|s| s.py_eq(&Py::Str(current))) {
                return Ok(Moved::Before);
            }
        }
        return Err(Failure::value("reset_settings_changed"));
    }
    let raw = regular_bytes(data, relative, 8 << 20)?;
    let mut model = pyops::loads(&raw)?;
    // `'schema' in model`, then `model.get('schema')`: a session that is no object (a
    // number, or a list or text holding "schema") is refused.
    let invalid = || Failure::value("invalid_saved_session");
    let wrapped = pyops::contains(&model, "schema").map_err(|_| invalid())?;
    if wrapped && !model.is_dict() {
        return Err(invalid());
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
        return Err(invalid());
    }
    let reply = command::run(root, &["/usr/bin/reborn", "--default-settings"], 2.0);
    let defaults = if reply.ok {
        pyops::loads(reply.output.clone().unwrap_or_default().as_bytes())?
    } else {
        Py::dict()
    };
    // Defaults that are no object (or of another schema) are none.
    let schema_ok = defaults
        .get("session_schema")
        .is_some_and(|s| s.py_eq(&Py::int(2)) || s.py_eq(&Py::int(3)));
    let Some(settings @ Py::Dict(_)) = defaults.get("settings").cloned().filter(|_| schema_ok)
    else {
        return Err(Failure::value("application_defaults_unavailable"));
    };
    if target.old.is_none() {
        free(quarantine, raw.len() as u64 * 2)?;
        let fd = open_at(
            Some(quarantine),
            target.saved,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
        .map_err(|e| os_name(e, target.saved))?;
        let mut backup = std::fs::File::from(fd);
        backup.write_all(&raw).and_then(|()| backup.sync_all())?;
        fsync(quarantine)?;
    }
    let body = if wrapped {
        model.get_mut("model")
    } else {
        Some(&mut model)
    };
    if let Some(body) = body {
        body.set("settings", settings); // an object with settings (body_ok)
    }
    // atomic_json's exact canonical encoding, including its newline.
    let result = pyjson::dumps_strict(&model, Style::Compact).map_err(Failure::Value)? + "\n";
    atomic_json(
        &location.join("settings-result.json"),
        &Py::dict().with("sha256", Py::Str(sha256_hex(result.as_bytes()))),
        true,
    )?;
    atomic_json(&root.path(&format!("/data/{relative}")), &model, true)?;
    Ok(Moved::Now)
}

/// Any other target: renamed into the quarantine (durably: both directories synced). A resume
/// whose rename was done before the interruption finds it there and nothing at the path.
fn quarantine_target(data: &OwnedFd, quarantine: &OwnedFd, target: &Target) -> Result<(), Failure> {
    // `old and source is None and old['device'] == expected['device'] and old['inode'] ==
    // expected['inode']`.
    let renamed_before = match (&target.old, &target.source) {
        (Some(o), None) => {
            field(o, "device").py_eq(target.planned("device")?)
                && field(o, "inode").py_eq(target.planned("inode")?)
        }
        _ => false,
    };
    if renamed_before {
        // Durable rename completed before an interrupted journal update.
        return Ok(());
    }
    if target.old.is_some() || !target.same_source() {
        return Err(Failure::value(format!(
            "reset_target_changed:{}",
            target.relative
        )));
    }
    let (parent, name) = parent_fd(data, &target.relative)?;
    rename_between(&parent, &name, quarantine, target.saved).map_err(|e| os_name(e, &name))?;
    fsync(&parent)?;
    fsync(quarantine)
}

/// `reset execute|resume`: move a confirmed plan's targets (resume: those of an interrupted reset,
/// after a restart).
pub(super) fn execute(
    root: &Root,
    confirm: Option<&str>,
    erase: bool,
    resume: bool,
) -> Result<Py, Failure> {
    let platform = root.path("/data/system/platform");
    private_directory(&platform)?;
    let lock_path = platform.join(".maintenance.lock");
    let _lock = y2p_core::lock::try_exclusive(&lock_path)
        .map_err(|e| os(e, &lock_path))?
        .ok_or_else(Failure::busy)?;
    let value = json_dict(root, "/data/system/platform/reset-plan.json");
    let Admitted {
        confirmation: confirmation_given,
        id,
        scope,
        volume,
    } = admit(root, &value, confirm, erase, resume)?;
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
    let targets = || {
        pyops::index(&value, "targets")
            .and_then(pyops::iterate)
            .or_refuse(PLAN_UNREADABLE)
    };
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
    if fstat(&quarantine)?.dev != fstat(&data)?.dev {
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
        let target = Target {
            relative,
            expected,
            source,
            saved: &saved,
            old,
        };
        if scope == "settings" && target.relative == SETTINGS {
            let moved = reset_settings(root, &data, &quarantine, &location, &id, &target, resume)?;
            if let Moved::Before = moved {
                completed.push(Py::Str(target.relative));
                continue;
            }
        } else {
            quarantine_target(&data, &quarantine, &target)?;
        }
        completed.push(Py::Str(target.relative));
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
        .with(
            "preserved",
            pyops::index(&value, "preserved")
                .or_refuse(PLAN_UNREADABLE)?
                .clone(),
        )
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
