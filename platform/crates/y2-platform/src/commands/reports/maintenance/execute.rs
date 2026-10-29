//! `reset execute|resume`: move a confirmed plan's targets into the quarantine, durably and in
//! order, so an interrupted reset resumes where it stopped (also after a restart).
// SPDX-License-Identifier: GPL-2.0-only
use super::*;

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
        .and_then(|()| file.sync_all())
        .map_err(os_plain)?;
    fsync_path(path.parent().expect("parent"))
}

/// A plan target's path, identity and what is there now, in Python's order: `inspect(data,
/// target['path']) != target['identity']` before the apply, `relative, expected = target['path'],
/// target['identity']` then `inspect(data, relative)` in it. A target without both, or whose path is
/// no str, is refused.
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

/// `execute(ctx, confirmation, erase_music, resume)`.
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
        .ok_or_else(|| os_plain(std::io::Error::from_raw_os_error(libc::EAGAIN)))?;
    let value = json_dict(root, "/data/system/platform/reset-plan.json");
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
        let (now, expires) = pyops::index(&value, "expires_monotonic_s")
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
        !pyops::index(&value, "data_generation")
            .or_refuse(PLAN_UNREADABLE)?
            .py_eq(&volume.generation.clone().into())
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
        let planned = |k: &str| pyops::index(&expected, k).or_refuse(PLAN_UNREADABLE);
        let same_source = source.as_ref().is_some_and(|s| s.py_eq(&expected));
        if scope == "settings" && relative == SETTINGS {
            // `source != expected and not (resume and old and old['inode'] == expected['inode'])`.
            let old_same_inode = || -> Result<bool, Failure> {
                Ok(match &old {
                    Some(o) if resume => field(o, "inode").py_eq(planned("inode")?),
                    _ => false,
                })
            };
            if !same_source && !old_same_inode()? {
                // Settings backup is a copy, handled by completed hash below.
                let record = super::super::json_at(
                    root,
                    &format!("/data/reset-quarantine/{id}/settings-result.json"),
                )
                .filter(|_| location.join("settings-result.json").exists())
                .unwrap_or(Py::dict());
                let sha = record.get("sha256").cloned();
                if resume {
                    let current = sha256_hex(&regular_bytes(&data, &relative, 8 << 20)?);
                    if sha.is_some_and(|s| s.py_eq(&Py::Str(current))) {
                        completed.push(Py::Str(relative.clone()));
                        continue;
                    }
                }
                return Err(Failure::value("reset_settings_changed"));
            }
            let raw = regular_bytes(&data, &relative, 8 << 20)?;
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
                model.get_mut("model")
            } else {
                Some(&mut model)
            };
            if let Some(body) = body {
                body.set("settings", settings); // an object with settings (body_ok)
            }
            // atomic_json's exact canonical encoding, including its newline.
            let result =
                pyjson::dumps_strict(&model, Style::Compact).map_err(Failure::Value)? + "\n";
            atomic_json(
                &location.join("settings-result.json"),
                &Py::dict().with("sha256", Py::Str(sha256_hex(result.as_bytes()))),
                true,
            )?;
            atomic_json(&root.path(&format!("/data/{relative}")), &model, true)?;
        } else {
            // `old and source is None and old['device'] == expected['device'] and old['inode'] ==
            // expected['inode']`.
            let renamed_before = match (&old, &source) {
                (Some(o), None) => {
                    field(o, "device").py_eq(planned("device")?)
                        && field(o, "inode").py_eq(planned("inode")?)
                }
                _ => false,
            };
            if renamed_before {
                // Durable rename completed before an interrupted journal update.
            } else if old.is_some() || !same_source {
                return Err(Failure::value(format!("reset_target_changed:{relative}")));
            } else {
                let (parent, name) = parent_fd(&data, &relative)?;
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
