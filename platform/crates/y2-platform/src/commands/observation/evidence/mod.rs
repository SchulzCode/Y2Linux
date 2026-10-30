//! `y2-platform boot-evidence` (`boot.capture_previous`, `boot_retained.capture`): snapshot the
//! bounded syslog of the previous boot and archive the retained PM/pstore bytes once per boot,
//! without clearing them. No new logger/poll loop, retained-RAM allocation or guessed reset register.
//! A panic may stop userspace before logging; absence of a marker proves nothing.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::At;
mod archive;

use super::{
    system::{reset_cause, RESET_STATUS},
    util::{self, splitlines},
};
use crate::{
    regex,
    reply::{atomic_json, private_directory, Failure, Outcome, Reply},
};
use archive::{code_points, Archive};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::{
    ffi::{CString, OsStr, OsString},
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Path, PathBuf},
};
use y2p_core::digest::sha256_hex as hex;
use y2p_core::mounts::mounts_at;
use y2p_core::{
    json::{dumps, dumps_sorted},
    py::{self, is_false, truthy},
    Root,
};

const BOOT_METADATA: &str = "/sys/firmware/y2_boot/metadata";
const PMIC_POWER_STATUS: &str = "/sys/firmware/y2_pmic/power_status";
/// battery.WARM_BOOT_REASONS.
const WARM_BOOT_REASONS: [i64; 4] = [3, 4, 5, 6];
/// TOP_RST_MISC PWRKEY_RST_TD, as the vendor keypad header (KPD_PMIC_LPRST_TD) names its selectors;
/// not timed on a Y2.
const LONG_PRESS_SECONDS: [i64; 4] = [8, 11, 14, 5];
const MAX_BYTES: usize = 2 * 1024 * 1024;
const KEEP: usize = 8;
const PM_FILES: [&str; 6] = [
    "previous",
    "devices_previous",
    "state",
    "devices",
    "reset_status",
    "backstop_s",
];

/// F56: how LK says this boot started. The RGU status is cleared before Linux reads it (0x0 in
/// every report), but a SoC reset (the watchdog, a restart) keeps the PMIC on: boot_reason 3-6 is
/// warm, anything else cold. After a previous boot without shutdown_complete, warm means the
/// watchdog or a kernel restart ended it, cold that power was cut or Power was held.
fn loader_boot(text: Option<&str>) -> Value {
    let mut fields = std::collections::BTreeMap::new();
    for word in util::split_ws(text.unwrap_or_default()) {
        if let Some((k, v)) = word.split_once('=') {
            fields.insert(k, v);
        }
    }
    let reason = fields.get("boot_reason").copied().unwrap_or("");
    if fields.get("valid") != Some(&"1")
        || reason.is_empty()
        || !reason.chars().all(|c| c.is_ascii_digit())
    {
        return json!({"boot_reason": null, "reset": null});
    }
    let Some(reason) = util::int(reason) else {
        return json!({"boot_reason": null, "reset": null});
    };
    json!({"boot_reason": reason, "reset": if WARM_BOOT_REASONS.contains(&reason) { "warm" } else { "cold" }})
}

/// F56/F57: the MT6323's record of the last power-down (kernel pmic-lowpower-policy.h), as the
/// loaders left it. LK clears a latched Power long-press reset by setting STRUP_CON8 bit 4 and
/// leaves it set, so a set bit may stay from an earlier boot: only a change from the previous
/// boot's record (`earlier`) proves a long-press reset now. A clear bit rules one out; brownout and
/// a dead battery stay unrecorded.
fn pmic_power_status(text: Option<&str>, earlier: Option<&Value>) -> Value {
    let fields = util::findall_map(regex!(r"(\w+)=(\S+)"), text.unwrap_or_default());
    let mut regs = std::collections::BTreeMap::new();
    for (name, value) in &fields {
        if name == "valid" || !regex!(r"\A0x[0-9a-fA-F]{1,4}\z").is_match(value) {
            continue;
        }
        if let Ok(register) = i64::from_str_radix(&value[2..], 16) {
            regs.insert(name.clone(), register);
        }
    }
    let Some(&con8) = regs
        .get("strup_con8")
        .filter(|_| fields.get("valid").map(String::as_str) == Some("1"))
    else {
        return json!({"state": "Unavailable", "long_press_reset": null,
                      "long_press_reset_reason": "not_observed"});
    };
    let flag = con8 & 0x0010 != 0;
    let (verdict, reason) = if !flag {
        (json!(false), "no_long_press_record")
    } else if is_false(earlier) {
        (json!(true), "lk_cleared_long_press_reset")
    } else if earlier.is_some_and(truthy) {
        (Value::Null, "record_set_before")
    } else {
        (Value::Null, "first_record")
    };
    let raw: Map<String, Value> = regs
        .iter()
        .map(|(n, v)| (n.clone(), json!(format!("0x{v:04x}"))))
        .collect();
    let mut result = json!({"state": "Ready", "raw": raw, "lk_long_press_record": flag,
        "long_press_reset": verdict, "long_press_reset_reason": reason,
        "just_pwrkey_rst": con8 & 0x4000 != 0});
    if let Some(misc) = regs.get("top_rst_misc") {
        result["long_press_reset_enabled"] = json!(misc & 0x0040 != 0);
        result["long_press_reset_seconds"] = json!(LONG_PRESS_SECONDS[((misc >> 8) & 3) as usize]);
    }
    if let Some(v) = regs.get("chr_con16") {
        result["uvlo_selector"] = json!(v & 3);
    }
    if let Some(v) = regs.get("strup_con5") {
        result["thermal_status"] = json!((v >> 8) & 7);
    }
    if let Some(v) = regs.get("chrstatus") {
        result["power_key_down"] = json!(v & 0x0002 == 0);
    }
    result
}

fn open_flags(path: &Path, flags: i32) -> io::Result<File> {
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags | libc::O_CLOEXEC)
        .open(path)
}

/// Open `name` relative to `directory` (close-on-exec).
fn openat(directory: &File, name: &OsStr, flags: i32) -> io::Result<File> {
    let name =
        CString::new(name.as_bytes()).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?;
    y2p_core::sys::openat(
        Some(std::os::fd::AsFd::as_fd(directory)),
        &name,
        flags | libc::O_CLOEXEC,
        0,
    )
    .map(File::from)
}

/// Read up to `limit` bytes (a buffered `read(limit)`).
fn read_up_to(file: &mut File, limit: usize) -> io::Result<Vec<u8>> {
    let mut data = Vec::new();
    file.take(limit as u64).read_to_end(&mut data)?;
    Ok(data)
}

/// The refusal of a directory where a file was expected: its text names the descriptor, which stays
/// open, so the next descriptors and the numbers in later texts are the ones the records always had.
fn directory_error(file: File) -> String {
    use std::os::fd::IntoRawFd;
    format!("[Errno 21] Is a directory: {}", file.into_raw_fd())
}

/// A retained source, opened without following a link or blocking: regular files only, at most
/// `limit` bytes.
fn source(path: &Path, limit: usize) -> Result<Vec<u8>, SourceError> {
    let mut file =
        open_flags(path, libc::O_NOFOLLOW | libc::O_NONBLOCK).map_err(SourceError::Os)?;
    let kind = file.metadata().map_err(SourceError::Os)?.file_type();
    if kind.is_dir() {
        return Err(SourceError::Text(directory_error(file)));
    }
    if !kind.is_file() {
        return Err(SourceError::Text("retained_source_not_regular".into()));
    }
    let raw = read_up_to(&mut file, limit + 1).map_err(SourceError::Os)?;
    if raw.len() > limit {
        return Err(SourceError::Text(
            "retained_source_exceeds_archive_bound".into(),
        ));
    }
    Ok(raw)
}

enum SourceError {
    Os(io::Error),
    Text(String),
}

/// Archive this boot's retained PM and pstore bytes (`boot-<id>.tar`, the newest 8 kept, the kernel's
/// entries never removed), or the error text the evidence record carries instead.
fn capture(
    root: &Root,
    boot_id: &str,
    previous_boot_id: &Value,
) -> Result<Map<String, Value>, String> {
    if !regex!(r"\A[A-Za-z0-9-]{1,64}\z").is_match(boot_id) {
        return Err("invalid_boot_archive_identity".into());
    }
    let directory = root.path("/data/system/platform/retained");
    private_directory(&directory).map_err(|e| e.message())?;
    let name = format!("boot-{boot_id}.tar");
    let destination = directory.join(&name);
    let temporary = directory.join(format!(".{name}.{}.tmp", std::process::id()));
    let relative = destination
        .strip_prefix(root.path("/"))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let pstore_mounted = mounts_at(root, "/sys/fs/pstore")
        .iter()
        .any(|m| m.filesystem == "pstore");
    let mut result = Map::new();
    result.insert("archive".into(), json!(relative));
    result.insert("files".into(), json!([]));
    result.insert("captured_on_boot_id".into(), json!(boot_id));
    result.insert("previous_boot_id".into(), previous_boot_id.clone());
    result.insert("pstore_mounted".into(), json!(pstore_mounted));
    result.insert("errors".into(), json!([]));
    result.insert("source_entries_deleted".into(), json!(false));
    let os = |e: io::Error, p: &Path| py::os_error(&e, Some(&p.to_string_lossy())).0;
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&temporary)
        .map_err(|e| os(e, &temporary))?;
    let outcome = (|| -> Result<(), String> {
        let mut archive = Archive::new(file);
        let mut files = Vec::new();
        let mut errors = Vec::new();
        let mut used = 0usize;
        let mut sources: Vec<(PathBuf, Vec<u8>, usize)> = PM_FILES
            .iter()
            .map(|part| {
                (
                    root.path(&format!("/sys/firmware/y2_pm/{part}")),
                    format!("pm/{part}").into_bytes(),
                    65_536,
                )
            })
            .collect();
        if pstore_mounted {
            let pstore = root.path("/sys/fs/pstore");
            // Refuse traversal even if the mount table was stale or forged.
            let handle = open_flags(&pstore, libc::O_DIRECTORY | libc::O_NOFOLLOW)
                .map_err(|e| os(e, &pstore))?;
            let listing = format!("/proc/self/fd/{}", handle.as_raw_fd());
            let mut names: Vec<OsString> = fs::read_dir(&listing)
                .map_err(|e| py::os_error(&e, None).0)?
                .map(|entry| entry.map(|e| e.file_name()))
                .collect::<io::Result<_>>()
                .map_err(|e| py::os_error(&e, None).0)?;
            names.sort_by_key(|n| code_points(n.as_bytes()));
            if names.len() > 32 {
                errors.push(json!({"source": "pstore", "error": "entry_count_exceeds_32"}));
            }
            for part in names.into_iter().take(32) {
                let mut entry = b"pstore/".to_vec();
                entry.extend(part.as_bytes());
                sources.push((pstore.join(&part), entry, MAX_BYTES));
            }
        }
        for (path, entry, limit) in sources {
            let label = String::from_utf8_lossy(&entry).into_owned();
            match source(&path, limit.min(MAX_BYTES - used)) {
                Ok(raw) => {
                    archive
                        .add(&entry, &raw)
                        .map_err(|e| py::os_error(&e, None).0)?;
                    used += raw.len();
                    files.push(json!({"name": label, "bytes": raw.len(), "sha256": hex(&raw)}));
                }
                Err(SourceError::Os(e)) if e.kind() == io::ErrorKind::NotFound => {
                    if entry.starts_with(b"pstore/") {
                        errors
                            .push(json!({"source": label, "error": "disappeared_during_capture"}));
                    }
                }
                Err(SourceError::Os(e)) => {
                    errors.push(json!({"source": label, "error": os(e, &path)}))
                }
                Err(SourceError::Text(text)) => {
                    errors.push(json!({"source": label, "error": text}))
                }
            }
        }
        result.insert("files".into(), Value::Array(files));
        result.insert("errors".into(), Value::Array(errors));
        let manifest = dumps_sorted(&result).map_err(|e| e.to_string())?;
        archive
            .add(b"manifest.json", manifest.as_bytes())
            .and_then(|()| archive.close())
            .and_then(|()| archive.file.sync_all())
            .map_err(|e| py::os_error(&e, None).0)?;
        drop(archive);
        fs::rename(&temporary, &destination).map_err(|e| os(e, &temporary))?;
        // Only archives owned by this helper rotate. Kernel pstore is never removed; raw bytes
        // remain available to the owner after boot capture.
        let mut archives: Vec<(PathBuf, i128)> = Vec::new();
        for entry in fs::read_dir(&directory)
            .map_err(|e| os(e, &directory))?
            .flatten()
        {
            let path = entry.path();
            let matches = entry
                .file_name()
                .to_str()
                .is_some_and(|n| y2p_core::root::fnmatch("boot-*.tar", n));
            if matches
                && path
                    .symlink_metadata()
                    .is_ok_and(|m| m.file_type().is_file())
            {
                let meta = fs::metadata(&path).map_err(|e| os(e, &path))?;
                archives.push((
                    path,
                    i128::from(meta.mtime()) * 1_000_000_000 + i128::from(meta.mtime_nsec()),
                ));
            }
        }
        archives.sort_by_key(|(_, mtime)| *mtime);
        let count = archives.len().saturating_sub(KEEP);
        for (old, _) in archives.into_iter().take(count) {
            if old != destination {
                fs::remove_file(&old).map_err(|e| os(e, &old))?;
            }
        }
        open_flags(&directory, libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .and_then(|d| d.sync_all())
            .map_err(|e| os(e, &directory))?;
        Ok(())
    })();
    let _ = fs::remove_file(&temporary);
    outcome?;
    result.insert(
        "captured_monotonic_s".into(),
        json!(root.clock().monotonic()),
    );
    Ok(result)
}

/// previous-boot-evidence.json: what the first capture of a boot found of the boot before (printed
/// and written sorted, so the field order is free). Read by `status` (system) and the error reports.
#[derive(Serialize)]
struct Evidence {
    schema: u8,
    captured_on_boot_id: String,
    /// The boot journal's fields as it has them (any JSON).
    previous_boot_id: Value,
    previous_last_stage: Value,
    previous_orderly_shutdown: Value,
    reset_cause: Value,
    reset_cause_reason: Value,
    kernel_panic_retention_guaranteed: bool,
    logs_are_private: bool,
    reset_status_raw: Value,
    record: Map<String, Value>,
    loader: Value,
    pmic: Value,
    retained: Value,
    logs: Vec<LogTail>,
}

/// The bounded tail of one system log (never the whole log).
#[derive(Serialize)]
struct LogTail {
    name: &'static str,
    tail_sha256: String,
    source_bytes_at_capture: u64,
    tail_lines: Vec<String>,
    bounded_tail_not_complete_log: bool,
}

/// `y2-platform boot-evidence`.
pub fn run(root: &Root) -> Outcome {
    if root.read("/data/.y2data-schema").as_deref() != Some("1") {
        return Err(Failure::value("data_schema_unavailable"));
    }
    let data = mounts_at(root, "/data");
    let verified = match &data[..] {
        [mount] => Some(mount.source.as_str()) == root.read("/run/y2-data-device").as_deref(),
        _ => false,
    };
    if !verified {
        return Err(Failure::value("verified_data_mount_unavailable"));
    }
    let directory = root.path("/data/system/platform");
    private_directory(&directory)?;
    let boot = root
        .boot_id()
        .filter(|b| !b.is_empty())
        .ok_or_else(|| Failure::value("boot_id_unavailable"))?;
    let previous = root.json_object("/data/system/platform/previous-boot-evidence.json");
    if previous.get("captured_on_boot_id").and_then(Value::as_str) == Some(boot.as_str()) {
        return Reply::sorted(&previous, 0);
    }
    let journal = root.json_object("/data/system/platform/boot.json");
    if journal.get("boot_id").and_then(Value::as_str) == Some(boot.as_str()) {
        return Err(Failure::value(
            "previous_boot_capture_missed_early_boundary",
        ));
    }
    let journal_get = |k: &str| journal.get(k).cloned().unwrap_or(Value::Null);
    let mut cause = reset_cause(None, "no_qualified_retained_register_or_pstore_backend");
    if let Some(text) = root.read(RESET_STATUS) {
        cause = reset_cause(Some(&text), "not_observed");
    }
    let earlier = match previous.get("pmic") {
        Some(Value::Object(pmic)) => pmic.get("lk_long_press_record").cloned(),
        _ => None,
    };
    let retained = match capture(root, &boot, &journal_get("boot_id")) {
        Ok(value) => Value::Object(value),
        Err(error) => json!({"error": error, "source_entries_deleted": false}),
    };
    let mut result = Evidence {
        schema: 1,
        captured_on_boot_id: boot.clone(),
        previous_boot_id: journal_get("boot_id"),
        previous_last_stage: journal_get("last_stage"),
        previous_orderly_shutdown: journal_get("orderly_shutdown"),
        reset_cause: json!(cause.reset_cause),
        reset_cause_reason: json!(cause.reset_cause_reason),
        kernel_panic_retention_guaranteed: false,
        logs_are_private: true,
        reset_status_raw: json!(cause.reset_status_raw),
        record: util::record(root, "previous-boot-evidence", Map::new()),
        loader: loader_boot(root.read(BOOT_METADATA).as_deref()),
        pmic: pmic_power_status(root.read(PMIC_POWER_STATUS).as_deref(), earlier.as_ref()),
        retained,
        logs: Vec::new(),
    };
    let data_path = root.path("/data");
    let data = open_flags(&data_path, libc::O_DIRECTORY | libc::O_NOFOLLOW).at(&data_path)?;
    let logs = match openat(
        &data,
        OsStr::new("logs"),
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
    ) {
        Ok(logs) => Some(logs),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(Failure::os(e, Path::new("logs"))),
    };
    let mut entries = Vec::new();
    if let Some(logs) = &logs {
        let dev = |f: &File| f.metadata().map(|m| m.dev());
        if dev(logs).at(Path::new("logs"))? != dev(&data).at(&data_path)? {
            return Err(Failure::value("log_submount_refused"));
        }
        for name in ["system.log.0", "system.log"] {
            let mut file = match openat(
                logs,
                OsStr::new(name),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            ) {
                Ok(file) => file,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(Failure::os(e, Path::new(name))),
            };
            let info = file.metadata().at(Path::new(name))?;
            if info.file_type().is_dir() {
                return Err(Failure::Os {
                    text: directory_error(file),
                    class: "IsADirectoryError",
                });
            }
            if !info.file_type().is_file() {
                return Err(Failure::value("invalid_log_file"));
            }
            let raw = file
                .seek(SeekFrom::Start(info.len().saturating_sub(65_536)))
                .and_then(|_| read_up_to(&mut file, 65_536))
                .at(Path::new(name))?;
            let text = String::from_utf8_lossy(&raw);
            let lines = splitlines(&text);
            let lines = lines[lines.len().saturating_sub(32)..]
                .iter()
                .map(|line| line.chars().take(500).collect())
                .collect();
            entries.push(LogTail {
                name,
                tail_sha256: hex(&raw),
                source_bytes_at_capture: info.len(),
                tail_lines: lines,
                bounded_tail_not_complete_log: true,
            });
        }
    }
    result.logs = entries;
    loop {
        let size = dumps(&result).map_err(Failure::value)?.len();
        if size <= 96_000 {
            break;
        }
        // The first of the longest tails loses its oldest line.
        let Some(largest) = result
            .logs
            .iter_mut()
            .fold(None::<&mut LogTail>, |best, item| match best {
                Some(b) if b.tail_lines.len() >= item.tail_lines.len() => Some(b),
                _ => Some(item),
            })
        else {
            return Err(Failure::value("max() iterable argument is empty"));
        };
        // Every log line dropped and still too large (a huge retained listing): refused, nothing
        // written.
        if largest.tail_lines.is_empty() {
            return Err(Failure::value("evidence_record_too_large"));
        }
        largest.tail_lines.remove(0);
    }
    atomic_json(
        &directory.join("previous-boot-evidence.json"),
        &result,
        true,
    )?;
    Reply::sorted(&result, 0)
}

#[cfg(test)]
mod capture_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pmic_records_decode_as_python() {
        let value = pmic_power_status(
            Some("valid=1 strup_con8=0x4010 top_rst_misc=0x0240 chr_con16=0x0003 chrstatus=0x0000 bad=0x12345"),
            Some(&json!(false)),
        );
        assert_eq!(value["long_press_reset"], true);
        assert_eq!(value["long_press_reset_seconds"], 14);
        assert_eq!(value["just_pwrkey_rst"], true);
        assert_eq!(value["power_key_down"], true);
        assert_eq!(value["raw"]["strup_con8"], "0x4010");
        assert!(value["raw"].get("bad").is_none());
        assert_eq!(
            loader_boot(Some("valid=1 boot_reason=4")),
            json!({"boot_reason": 4, "reset": "warm"})
        );
    }
}
