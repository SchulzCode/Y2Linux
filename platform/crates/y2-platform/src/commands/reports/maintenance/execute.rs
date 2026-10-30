//! `reset execute|resume`: move a confirmed plan's targets into the quarantine, durably and in
//! order, so an interrupted reset resumes where it stopped (also after a restart).
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    atomic_json, busy, command, confirmation, data_volume, free, fstat, fsync, fsync_path,
    identity, inspect, json_dict, open_at, open_data, os, os_name, parent_fd, private_directory,
    regular_bytes, rename_between, sha256_hex, stat_at, volume, Failure, FileIdentity, OwnedFd,
    Path, Plan, ResetRecord, Root, Value, DIRECTORY, SCOPES, SETTINGS,
};
use crate::commands::reports::ValueExt;
use crate::commands::service::update::JournalState;
use serde::{Deserialize, Serialize};
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

/// A plan that passed every check before anything changes.
struct Admitted<'a> {
    /// The confirmation the person gave (the plan's own).
    confirmation: &'a str,
    plan: Plan,
    /// The data volume the plan was made for.
    volume: y2p_core::volume::Volume,
}

/// The checks before anything changes, in this order: the exact confirmation and plan id, the plan
/// unchanged since it was confirmed (the hash over the plan as read), the plan readable, its scope
/// and boot (a resume may cross a restart), the erase confirmation for a full reset, its expiry (not
/// on resume), no consumer still running, the same data volume, no update in progress.
fn admit<'a>(
    root: &Root,
    stored: &Value,
    confirm: Option<&'a str>,
    erase: bool,
    resume: bool,
) -> Result<Admitted<'a>, Failure> {
    let required = || Failure::value("exact_reset_confirmation_required");
    let confirmation_given = confirm
        .filter(|c| !c.is_empty())
        .filter(|c| stored.get("confirmation").and_then(Value::as_str) == Some(*c))
        .ok_or_else(required)?;
    let id = stored.get("id").and_then(Value::as_str).unwrap_or_default();
    let id_ok = id.len() == 32
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !id_ok {
        return Err(required());
    }
    let mut signed = stored.clone();
    if let Value::Object(map) = &mut signed {
        map.shift_remove("confirmation");
    }
    if confirmation(&signed) != confirmation_given {
        return Err(Failure::value("reset_plan_changed"));
    }
    // A plan that matches its confirmation but does not read was written by hand.
    let plan: Plan =
        serde_json::from_value(stored.clone()).map_err(|_| Failure::value(PLAN_UNREADABLE))?;
    if !SCOPES.contains(&plan.scope.as_str()) || (!resume && plan.boot_id != root.boot_id()) {
        return Err(Failure::value("stale_reset_plan"));
    }
    if plan.scope == "full-user" && !erase {
        return Err(Failure::value(
            "full_reset_requires_erase_user_music_confirmation",
        ));
    }
    if !resume && root.clock().monotonic() > plan.expires_monotonic_s {
        return Err(Failure::value("reset_plan_expired"));
    }
    if !busy(root, &plan.scope)?.is_empty() {
        return Err(Failure::value("reset_consumers_still_running"));
    }
    let volume = data_volume(root)?;
    let changed = if resume {
        plan.data_uuid
            .as_deref()
            .filter(|u| !u.is_empty())
            .is_none_or(|u| volume.uuid.as_deref() != Some(u))
    } else {
        plan.data_generation != volume.generation
    };
    if changed {
        return Err(Failure::value("reset_data_generation_changed"));
    }
    if !JournalState::named(Some(&volume::update_state(root)?)).terminal() {
        return Err(Failure::value("update_in_progress"));
    }
    Ok(Admitted {
        confirmation: confirmation_given,
        plan,
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
    relative: &'a str,
    expected: FileIdentity,
    /// What is at the path now (None: nothing).
    source: Option<FileIdentity>,
    /// The quarantine entry's name (`000`, `001`, …) and what is already there.
    saved: &'a str,
    old: Option<FileIdentity>,
}

impl Target<'_> {
    fn same_source(&self) -> bool {
        self.source == Some(self.expected)
    }
}

/// The settings result's record (settings-result.json in the quarantine).
#[derive(Serialize, Deserialize)]
struct SettingsResult {
    sha256: String,
}

/// The settings scope's session file: a copy kept in quarantine, then its settings replaced by
/// Reborn's defaults (the rest of the session kept as read); a resume whose result is already in
/// place is done before.
fn reset_settings(
    root: &Root,
    data: &OwnedFd,
    quarantine: &OwnedFd,
    location: &Path,
    target: &Target,
    resume: bool,
) -> Result<Moved, Failure> {
    let relative = target.relative;
    let old_same_inode = resume && target.old.is_some_and(|o| o.inode == target.expected.inode);
    if !target.same_source() && !old_same_inode {
        // Settings backup is a copy, handled by completed hash below.
        if resume {
            let result = location.join("settings-result.json");
            let recorded = std::fs::read(&result)
                .ok()
                .and_then(|text| serde_json::from_slice::<SettingsResult>(&text).ok());
            let current = sha256_hex(&regular_bytes(data, relative, 8 << 20)?);
            if recorded.is_some_and(|r| r.sha256 == current) {
                return Ok(Moved::Before);
            }
        }
        return Err(Failure::value("reset_settings_changed"));
    }
    let raw = regular_bytes(data, relative, 8 << 20)?;
    // Reborn's session, kept as it is apart from its settings: an object, or no session.
    let invalid = || Failure::value("invalid_saved_session");
    let mut model = match serde_json::from_slice::<Value>(&raw) {
        Ok(model @ Value::Object(_)) => model,
        _ => return Err(invalid()),
    };
    let wrapped = model.get("schema").is_some();
    // Schema 3 (Reborn Beta 8.33) stores the queue by reference; only the settings change
    // here, the rest is kept as read.
    if wrapped
        && !model
            .get("schema")
            .is_some_and(|s| s.eq(&Value::from(2)) || s.eq(&Value::from(3)))
    {
        return Err(Failure::value("unsupported_session_schema"));
    }
    let body_ok = {
        let body = if wrapped {
            model.get("model")
        } else {
            Some(&model)
        };
        body.is_some_and(|b| b.is_object() && b.get("settings").is_some())
    };
    if !body_ok {
        return Err(invalid());
    }
    let reply = command::run(root, &["/usr/bin/reborn", "--default-settings"], 2.0);
    let defaults = reply
        .output
        .filter(|_| reply.ok)
        .and_then(|output| serde_json::from_str::<Value>(&output).ok())
        .unwrap_or(Value::Null);
    // Defaults that are no object (or of another schema) are none.
    let schema_ok = defaults
        .get("session_schema")
        .is_some_and(|s| s.eq(&Value::from(2)) || s.eq(&Value::from(3)));
    let Some(settings @ Value::Object(_)) = defaults.get("settings").cloned().filter(|_| schema_ok)
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
    let result = y2p_core::record::encode(&model).map_err(Failure::value)?;
    atomic_json(
        &location.join("settings-result.json"),
        &SettingsResult {
            sha256: sha256_hex(result.as_bytes()),
        },
        true,
    )?;
    atomic_json(&root.path(&format!("/data/{relative}")), &model, true)?;
    Ok(Moved::Now)
}

/// Any other target: renamed into the quarantine (durably: both directories synced). A resume
/// whose rename was done before the interruption finds it there and nothing at the path.
fn quarantine_target(data: &OwnedFd, quarantine: &OwnedFd, target: &Target) -> Result<(), Failure> {
    let renamed_before = match (&target.old, &target.source) {
        (Some(o), None) => o.device == target.expected.device && o.inode == target.expected.inode,
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
    let (parent, name) = parent_fd(data, target.relative)?;
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
) -> Result<ResetRecord, Failure> {
    let platform = root.path("/data/system/platform");
    private_directory(&platform)?;
    let lock_path = platform.join(".maintenance.lock");
    let _lock = y2p_core::lock::try_exclusive(&lock_path)
        .map_err(|e| os(e, &lock_path))?
        .ok_or_else(Failure::busy)?;
    let stored = json_dict(root, "/data/system/platform/reset-plan.json");
    let Admitted {
        confirmation: confirmation_given,
        plan,
        volume,
    } = admit(root, &stored, confirm, erase, resume)?;
    let id = plan.id.clone();
    let data = open_data(root)?;
    if let Some(finished) = ResetRecord::read(root, "/data/system/platform/maintenance-state.json")
        .filter(|f| f.id == id && f.state == "Complete")
    {
        crate::reply::unlink_missing_ok(&platform.join("maintenance-pending"))?;
        return Ok(finished);
    }
    if !resume {
        for target in &plan.targets {
            if inspect(&data, &target.path)? != Some(target.identity) {
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
    let mut record = ResetRecord {
        schema: 1,
        id: id.clone(),
        scope: plan.scope.clone(),
        state: "Applying".into(),
        completed: Vec::new(),
        preserved: None,
        quarantine: None,
        purge_confirmation: None,
    };
    durable_flag(&platform.join("maintenance-pending"))?;
    atomic_json(&location.join("record.json"), &record, true)?;
    for (index, planned) in plan.targets.iter().enumerate() {
        let source = inspect(&data, &planned.path)?;
        let saved = format!("{index:03}");
        let old = match stat_at(&quarantine, &saved) {
            Ok(meta) => Some(identity(&meta)),
            Err(e) if e.raw_os_error() == Some(libc::ENOENT) => None,
            Err(e) => return Err(os_name(e, &saved)),
        };
        let target = Target {
            relative: &planned.path,
            expected: planned.identity,
            source,
            saved: &saved,
            old,
        };
        if plan.scope == "settings" && target.relative == SETTINGS {
            let moved = reset_settings(root, &data, &quarantine, &location, &target, resume)?;
            if let Moved::Before = moved {
                record.completed.push(planned.path.clone());
                continue;
            }
        } else {
            quarantine_target(&data, &quarantine, &target)?;
        }
        record.completed.push(planned.path.clone());
        atomic_json(&location.join("record.json"), &record, true)?;
    }
    if data_volume(root)?.generation != volume.generation {
        return Err(Failure::value("reset_data_generation_changed"));
    }
    record.state = "Complete".into();
    record.preserved = Some(plan.requirements.preserved.clone());
    record.quarantine = Some(format!("/data/reset-quarantine/{id}"));
    record.purge_confirmation = Some(confirmation_given.into());
    atomic_json(&location.join("record.json"), &record, true)?;
    atomic_json(&platform.join("maintenance-state.json"), &record, true)?;
    let pending = platform.join("maintenance-pending");
    std::fs::remove_file(&pending).map_err(|e| os(e, &pending))?;
    fsync_path(&platform)?;
    Ok(record)
}
