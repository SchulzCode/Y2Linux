//! `reset plan`: what a reset of a scope would move, after the consumers stopped; the plan is
//! signed by the hash of its exact bytes and expires after five minutes.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;

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
pub(super) fn busy(root: &Root, scope: &str) -> Result<Vec<Py>, Failure> {
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

pub(super) fn confirmation(value: &Py) -> String {
    sha256_hex(pyjson::dumps(value, Style::Compact).as_bytes())
}

/// `plan(ctx, scope)`.
pub(super) fn plan(root: &Root, scope: Option<&str>) -> Result<Py, Failure> {
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
                Py::Float(root.clock().monotonic() + 300.0),
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
