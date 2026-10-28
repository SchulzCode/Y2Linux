//! diagnostics.py: the tester export with an allowlisted projection, separate from private backup.
//!
//! No raw log, database, credential file, calibration or user filename is exported. Log excerpts are
//! represented by fixed event classes and numeric errno only: free text cannot be reliably scrubbed by
//! a password regular expression.
//!
//! The bundle carries `observe.snapshot(ctx)` and `health.check(ctx)`, which belong to the
//! observation group: until those are in Rust, `export-diagnostics` hands itself to Python before it
//! reads or writes anything (`platform_state`).
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    fields::{FIELDS, KERNEL_FIELDS, KERNEL_STATUS_KEYS, PUBLIC_WORDS},
    json_dict, os, os_plain,
    pyjson::{self, Py, Style},
    pyre::{self, lazy, S},
    state::uuid4,
    tarball,
    volume::{admitted, data_volume},
};
use crate::reply::{private_directory, Failure, Outcome, Reply};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::{io::Write, os::unix::fs::OpenOptionsExt, sync::OnceLock};
use y2p_core::{command, Root};

const MAX_BYTES: usize = 2 * 1024 * 1024;
const IDENTITY_KEYS: [&str; 5] = [
    "build_git_commit",
    "reborn_source_commit",
    "y2linux_commit",
    "reborn_commit",
    "sha256",
];
const VERSION_KEYS: [&str; 7] = [
    "kernel_version",
    "kernel",
    "rootfs_version",
    "rootfs_release",
    "release_version",
    "reborn_version",
    "build_id",
];
const EVENTS: [&str; 14] = [
    "timeout",
    "crc",
    "underrun",
    "xrun",
    "overflow",
    "abort",
    "resume",
    "suspend",
    "reconnect",
    "disconnect",
    "failed",
    "error",
    "panic",
    "warning",
];
const COMPONENTS: [&str; 21] = [
    "msdc",
    "mmc",
    "musb",
    "usb",
    "spm",
    "cirq",
    "gpt",
    "afe",
    "alsa",
    "cs431",
    "bluetooth",
    "bluealsa",
    "wifi",
    "wlan",
    "reborn",
    "battery",
    "charger",
    "ext4",
    "drm",
    "lima",
    "watchdog",
];

fn sorted_contains(list: &[&str], value: &str) -> bool {
    list.binary_search(&value).is_ok()
}

fn private_key(key: &str) -> bool {
    static CELL: OnceLock<Regex> = OnceLock::new();
    lazy(
        &CELL,
        r"(?i)(password|passphrase|secret|credential|ssid|bssid|address|mac(?:\n?\z)|cid(?:\n?\z)|serial|calibration|nvram|bond|link.?key|identity|filename|track|title|artist|album|path|cmdline)",
    )
    .is_match(key)
}

fn full(pattern: &'static OnceLock<Regex>, re: &str, value: &str) -> bool {
    lazy(pattern, re).is_match(value)
}

/// `project(value, key, depth)`: keep counters and known public enums; discard unknown strings and
/// keys.
pub fn project(value: &Py, key: &str, depth: usize) -> Py {
    if depth > 16 {
        return Py::str("[depth-limit]");
    }
    // A public bonding fact is a boolean, never a bond record/key/string.
    if key == "bonded" && matches!(value, Py::Bool(_)) {
        return value.clone();
    }
    if private_key(key) {
        return Py::str("[redacted]");
    }
    match value {
        Py::None | Py::Bool(_) | Py::Int(_) => value.clone(),
        Py::Float(f) => {
            if f.is_finite() {
                value.clone()
            } else {
                Py::None
            }
        }
        Py::Dict(items) => {
            let mut out: Vec<(String, Py)> = Vec::new();
            for (k, v) in items.iter().take(512) {
                if sorted_contains(&FIELDS, k) {
                    out.push((k.clone(), project(v, k, depth + 1)));
                }
            }
            Py::Dict(out)
        }
        Py::List(items) => Py::List(
            items
                .iter()
                .take(256)
                .map(|v| project(v, key, depth + 1))
                .collect(),
        ),
        Py::Str(text) => {
            if sorted_contains(&KERNEL_STATUS_KEYS, key)
                || matches!(key, "11200000.usb" | "11230000.mmc" | "11240000.mmc")
            {
                static PAIRS: OnceLock<Regex> = OnceLock::new();
                let pairs = lazy(&PAIRS, &format!(r"([\p{{L}}\p{{N}}_]+)=([^{S}]+)"));
                let mut out: Vec<(String, Py)> = Vec::new();
                for caps in pairs.captures_iter(text) {
                    let field = &caps[1];
                    if sorted_contains(&KERNEL_FIELDS, field) {
                        let projected = project(&Py::str(&caps[2]), field, depth + 1);
                        match out.iter_mut().find(|(k, _)| k == field) {
                            Some(entry) => entry.1 = projected,
                            None => out.push((field.to_owned(), projected)),
                        }
                    }
                }
                return Py::Dict(out);
            }
            static HASH: OnceLock<Regex> = OnceLock::new();
            static BOOT: OnceLock<Regex> = OnceLock::new();
            static VERSION: OnceLock<Regex> = OnceLock::new();
            static NUMBER: OnceLock<Regex> = OnceLock::new();
            if IDENTITY_KEYS.contains(&key)
                && full(&HASH, r"\A(?:[0-9a-f]{40}|[0-9a-f]{64})\z", text)
            {
                return value.clone();
            }
            if matches!(key, "boot_id" | "previous_boot_id")
                && full(
                    &BOOT,
                    r"\A[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}\z",
                    text,
                )
            {
                return value.clone();
            }
            if VERSION_KEYS.contains(&key)
                && full(&VERSION, r"\A[a-zA-Z0-9][a-zA-Z0-9.+_-]{0,99}\z", text)
            {
                return value.clone();
            }
            if text.chars().count() < 32
                && full(
                    &NUMBER,
                    r"\A-?(?:[0-9]+(?:\.[0-9]+)?|0x[0-9a-fA-F]+)\z",
                    text,
                )
            {
                return value.clone();
            }
            if PUBLIC_WORDS.binary_search(&text.as_str()).is_ok() {
                return value.clone();
            }
            Py::str("[redacted]")
        }
    }
}

/// `events(raw)`: fixed-vocabulary excerpts; no source text is ever copied into the bundle.
pub fn events(raw: Option<&str>) -> Py {
    static ERRNO: OnceLock<Regex> = OnceLock::new();
    let errno = lazy(
        &ERRNO,
        &format!(r"(?:errno|error|ret)[{S}]*[=:][{S}]*(-?\d{{1,4}})(?:[^\p{{L}}\p{{N}}_]|\z)"),
    );
    let lines = pyre::splitlines(raw.unwrap_or(""));
    let start = lines.len().saturating_sub(256);
    let mut result = Vec::new();
    for (number, line) in lines[start..].iter().enumerate() {
        let lower = line.to_lowercase();
        let tags: Vec<Py> = EVENTS
            .iter()
            .filter(|tag| {
                lower
                    .match_indices(*tag)
                    .any(|(at, _)| pyre::word_start(&lower, at))
            })
            .map(|t| Py::str(*t))
            .collect();
        let components: Vec<Py> = COMPONENTS
            .iter()
            .filter(|c| lower.contains(*c))
            .map(|c| Py::str(*c))
            .collect();
        if !tags.is_empty() && !components.is_empty() {
            let numbers: Vec<Py> = pyre::finditer(errno, &lower, 1, pyre::word_start)
                .iter()
                .take(4)
                .filter_map(|c| c[1].parse::<i64>().ok().map(Py::int))
                .collect();
            result.push(
                Py::dict()
                    .with("tail_line", Py::from(number))
                    .with("components", Py::List(components))
                    .with("events", Py::List(tags))
                    .with("errno", Py::List(numbers)),
            );
        }
    }
    Py::List(result)
}

/// `observe.snapshot(ctx)` and `health.check(ctx)`, once the observation group provides them.
fn platform_state(_root: &Root) -> Option<(Py, Py)> {
    None
}

/// `log_tail(ctx, relative)`: walk reviewed data paths with no symlinks; never read arbitrary files.
fn log_tail(root: &Root, relative: &str) -> Result<Py, Failure> {
    use super::media::{fstat, open_at};
    use std::io::{Read, Seek, SeekFrom};
    let directory = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    let data = root.path("/data");
    let mut fd = open_at(None, &data.to_string_lossy(), directory, 0).map_err(|e| os(e, &data))?;
    let parts: Vec<&str> = relative.split('/').collect();
    let missing = |e: std::io::Error, name: &str| -> Result<Option<()>, Failure> {
        if e.raw_os_error() == Some(libc::ENOENT) {
            Ok(None)
        } else {
            Err(super::os_name(e, name))
        }
    };
    for part in &parts[..parts.len() - 1] {
        fd = match open_at(Some(&fd), part, directory, 0) {
            Ok(child) => child,
            Err(e) => return missing(e, part).map(|_| Py::List(Vec::new())),
        };
    }
    let name = parts[parts.len() - 1];
    let file = match open_at(
        Some(&fd),
        name,
        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        0,
    ) {
        Ok(file) => file,
        Err(e) => return missing(e, name).map(|_| Py::List(Vec::new())),
    };
    let meta = fstat(&file).map_err(os_plain)?;
    if meta.st_mode & libc::S_IFMT != libc::S_IFREG || meta.st_nlink != 1 {
        return Err(Failure::value("diagnostic_log_type"));
    }
    let mut stream = std::fs::File::from(file);
    stream
        .seek(SeekFrom::Start((meta.st_size as u64).saturating_sub(65536)))
        .map_err(os_plain)?;
    let mut tail = Vec::new();
    stream
        .take(65536)
        .read_to_end(&mut tail)
        .map_err(os_plain)?;
    Ok(events(Some(&String::from_utf8_lossy(&tail))))
}

fn collect(root: &Root, snapshot: Py, health: Py) -> Py {
    let mut logs = Py::dict();
    for (name, path) in [
        ("system", "logs/system.log"),
        ("reborn", "reborn/logs/reborn.log"),
    ] {
        let value =
            log_tail(root, path).unwrap_or_else(|_| Py::dict().with("state", Py::str("Failed")));
        logs.set(name, value);
    }
    let kernel = command::run_with(root, &["dmesg"], 2.0, 1 << 20, false);
    logs.set(
        "kernel",
        if kernel.ok {
            events(kernel.output.as_deref())
        } else {
            Py::dict().with("state", Py::str("Unavailable"))
        },
    );
    Py::dict()
        .with("schema", Py::str("org.y2linux.tester-diagnostics/v1"))
        .with("redacted", Py::Bool(true))
        .with(
            "privacy",
            Py::str("Counters and known enums only; logs are event classes, never raw text."),
        )
        .with(
            "versions",
            project(&json_dict(root, "/etc/y2linux/versions.json"), "", 0),
        )
        .with(
            "boot_id",
            project(&super::boot_id(root).into(), "boot_id", 0),
        )
        .with("platform", project(&snapshot, "", 0))
        .with("health", project(&health, "", 0))
        .with(
            "previous_boot",
            project(
                &json_dict(root, "/data/system/platform/previous-boot-evidence.json"),
                "",
                0,
            ),
        )
        .with(
            "suspend",
            project(
                &json_dict(root, "/data/system/platform/suspend-last.json"),
                "",
                0,
            ),
        )
        .with("logs", logs)
}

fn export_inner(root: &Root, snapshot: Py, health: Py) -> Result<Py, Failure> {
    let volume = data_volume(root)?;
    if !admitted(&volume, MAX_BYTES as u64) {
        return Err(Failure::value("diagnostic_space_reserved"));
    }
    let directory = root.path("/data/exports");
    private_directory(&directory)?;
    let lock_path = directory.join(".diagnostics.lock");
    let _lock = y2p_core::lock::try_exclusive(&lock_path)
        .map_err(|e| os(e, &lock_path))?
        .ok_or_else(|| Failure::value("busy"))?;
    let existing = std::fs::read_dir(&directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with("y2-diagnostics-") && n.ends_with(".tar.gz")
        })
        .count();
    if existing >= 8 {
        return Err(Failure::value("diagnostic_export_limit_retrieve_existing"));
    }
    let raw = pyjson::dumps_strict(&collect(root, snapshot, health), Style::Spaced)
        .map_err(Failure::Value)?
        + "\n";
    if raw.len() > MAX_BYTES {
        return Err(Failure::value("diagnostic_size_limit"));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let name = format!("y2-diagnostics-{now}-{}.tar.gz", uuid4()?.replace('-', ""));
    let partial = directory.join(format!(".{name}.partial"));
    let result = (|| {
        let mut stream = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&partial)
            .map_err(|e| os(e, &partial))?;
        let mut tar = tarball::Writer::new();
        tar.add(
            &tarball::Info::file("diagnostics.json", raw.len(), 0, 0o600),
            raw.as_bytes(),
        );
        stream
            .write_all(&tarball::gzip(&tar.finish(), 9, now as u32))
            .and_then(|()| stream.sync_all())
            .map_err(os_plain)?;
        if data_volume(root)?.generation != volume.generation {
            return Err(Failure::value("diagnostic_data_generation_changed"));
        }
        let complete = directory.join(&name);
        std::fs::rename(&partial, &complete).map_err(|e| os(e, &partial))?;
        std::fs::File::open(&directory)
            .and_then(|d| d.sync_all())
            .map_err(os_plain)?;
        let bytes = std::fs::read(&complete).map_err(|e| os(e, &complete))?;
        let digest: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Ok(Py::dict()
            .with("state", Py::str("Complete"))
            .with("path", Py::Str(format!("/data/exports/{name}")))
            .with("sha256", Py::Str(digest))
            .with("bytes", Py::from(bytes.len()))
            .with("redacted", Py::Bool(true)))
    })();
    let _ = std::fs::remove_file(&partial);
    result
}

/// cli.py's `export-diagnostics`: any OSError or ValueError is `diagnostic_export_failed`.
pub fn export(root: &Root) -> Outcome {
    let Some((snapshot, health)) = platform_state(root) else {
        return Err(Failure::Fallback);
    };
    match export_inner(root, snapshot, health) {
        Ok(value) => Ok(Reply::text(pyjson::dumps(&value, Style::Spaced))),
        Err(Failure::Fallback) => Err(Failure::Fallback),
        Err(_) => Reply::unsorted(
            &y2p_core::json::Ordered::new()
                .with("state", "Failed")
                .with("failure", "diagnostic_export_failed"),
            1,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same values through host Python's diagnostics.project and events.
    #[test]
    fn projection_and_events_match_python() {
        let value = pyjson::loads(
            r#"{"state": "Ready", "ssid": "Home", "boot_id": "0123abcd-0123-4567-89ab-0123456789ab",
                "release_version": "1.0.0-beta.8.44", "battery": {"percent": 80, "temperature_c": 31.5,
                "status": "Charging", "mac": "aa", "other": "x"}, "spm": "valid=1 sequence=7 bogus=3 power=on",
                "counters": [1, "2", "0x1f", "free text", null], "unknown_key": 1, "cpu": {"nested": 1}}"#,
        )
        .unwrap();
        let ours = pyjson::dumps(&project(&value, "", 0), Style::Spaced);
        let log = "msdc0: cmd timeout error=-110\nwifi: reconnect failed ret: 5\nnothing here\nBattery charger warning\n";
        let ours_events = pyjson::dumps(&events(Some(log)), Style::Spaced);
        let script = r#"
import json, sys
sys.path.insert(0, sys.argv[1])
from y2_platform import diagnostics
print(json.dumps(diagnostics.project(json.loads(sys.argv[2])), sort_keys=True))
print(json.dumps(diagnostics.events(sys.argv[3]), sort_keys=True))
"#;
        let platform = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../tools/platform");
        let Ok(python) = std::process::Command::new("python3")
            .args(["-I", "-c", script, platform])
            .arg(pyjson::dumps(&value, Style::Spaced))
            .arg(log)
            .output()
        else {
            return;
        };
        assert!(
            python.status.success(),
            "{}",
            String::from_utf8_lossy(&python.stderr)
        );
        let text = String::from_utf8(python.stdout).unwrap();
        let mut lines = text.lines();
        assert_eq!(ours, lines.next().unwrap());
        assert_eq!(ours_events, lines.next().unwrap());
    }
}
