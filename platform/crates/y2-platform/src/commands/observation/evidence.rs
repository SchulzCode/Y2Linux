//! `y2-platform boot-evidence` (`boot.capture_previous`, `boot_retained.capture`): snapshot the
//! bounded syslog of the previous boot and archive the retained PM/pstore bytes once per boot,
//! without clearing them. No new logger/poll loop, retained-RAM allocation or guessed reset register.
//! A panic may stop userspace before logging; absence of a marker proves nothing.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    snapshot::{mountinfo, mounts_at},
    system::{reset_cause, RESET_STATUS},
    util::{self, splitlines},
};
use crate::{
    regex,
    reply::{atomic_json, private_directory, Failure, Outcome, Reply},
};
use serde_json::{json, Map, Value};
use std::{
    ffi::{CString, OsStr, OsString},
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Path, PathBuf},
};
use y2p_core::{
    clock,
    json::{dumps, dumps_sorted, Separators},
    observation_support::sha2::{Digest, Sha256},
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

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

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
        if name != "valid" && regex!(r"\A0x[0-9a-fA-F]{1,4}\z").is_match(value) {
            regs.insert(
                name.clone(),
                i64::from_str_radix(&value[2..], 16).expect("hex"),
            );
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

// ---- the tar archive, byte for byte as Python's tarfile (PAX format) writes it -------------------

const BLOCK: usize = 512;
const RECORD: usize = 20 * BLOCK;

/// `tarfile.itn` for a value that fits: zero-padded octal and a NUL.
fn itn(value: u64, digits: usize) -> Vec<u8> {
    format!("{value:0width$o}\0", width = digits - 1).into_bytes()
}

/// `tarfile.stn`: the text cut or NUL-padded to its field.
fn stn(text: &[u8], length: usize) -> Vec<u8> {
    let mut field = text[..text.len().min(length)].to_vec();
    field.resize(length, 0);
    field
}

/// `TarInfo._create_header(info, USTAR_FORMAT)` for a file or a pax header.
fn ustar_header(name: &[u8], mode: u64, size: u64, kind: u8) -> Vec<u8> {
    let mut block = Vec::with_capacity(BLOCK);
    block.extend(stn(name, 100));
    block.extend(itn(mode & 0o7777, 8));
    block.extend(itn(0, 8));
    block.extend(itn(0, 8));
    block.extend(itn(size, 12));
    block.extend(itn(0, 12));
    block.extend(b"        ");
    block.push(kind);
    block.extend(stn(b"", 100));
    block.extend(b"ustar\x0000");
    block.extend(stn(b"", 32));
    block.extend(stn(b"", 32));
    block.extend(stn(b"", 8));
    block.extend(stn(b"", 8));
    block.extend(stn(b"", 155));
    block.resize(BLOCK, 0);
    let checksum: u32 = block.iter().map(|b| u32::from(*b)).sum();
    let field = format!("{checksum:06o}\0");
    block[148..155].copy_from_slice(field.as_bytes());
    block
}

/// The name as Python's str (invalid bytes as surrogates), for its ASCII/length tests and sorting.
fn code_points(name: &[u8]) -> Vec<u32> {
    let mut points = Vec::new();
    let mut rest = name;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(text) => {
                points.extend(text.chars().map(u32::from));
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                let text = std::str::from_utf8(&rest[..valid]).expect("valid prefix");
                points.extend(text.chars().map(u32::from));
                let bad = error.error_len().unwrap_or(rest.len() - valid);
                points.extend(
                    rest[valid..valid + bad]
                        .iter()
                        .map(|b| 0xdc00 + u32::from(*b)),
                );
                rest = &rest[valid + bad..];
            }
        }
    }
    points
}

/// `TarInfo.tobuf(PAX_FORMAT)` for a regular file of `size` bytes, mode 0o600, mtime 0, owner 0.
fn file_header(name: &[u8], size: u64) -> Vec<u8> {
    {
        let points = code_points(name);
        let ascii = points.iter().all(|p| *p < 0x80);
        let mut out = Vec::new();
        if !ascii || points.len() > 100 {
            // A pax extended header carries the path; surrogates force hdrcharset=BINARY.
            let binary = std::str::from_utf8(name).is_err();
            let mut records = Vec::new();
            if binary {
                records.extend(b"21 hdrcharset=BINARY\n");
            }
            let l = "path".len() + name.len() + 3;
            let mut p = 0;
            loop {
                let n = l + p.to_string().len();
                if n == p {
                    break;
                }
                p = n;
            }
            records.extend(format!("{p} path=").as_bytes());
            records.extend(name);
            records.push(b'\n');
            out.extend(ustar_header(
                b"././@PaxHeader",
                0,
                records.len() as u64,
                b'x',
            ));
            let padded = records.len().div_ceil(BLOCK) * BLOCK;
            records.resize(padded, 0);
            out.extend(records);
        }
        // The ustar name in ASCII with '?' for every other character.
        let ustar: Vec<u8> = points
            .iter()
            .map(|p| if *p < 0x80 { *p as u8 } else { b'?' })
            .collect();
        out.extend(ustar_header(&ustar, 0o600, size, b'0'));
        out
    }
}

struct Archive {
    file: File,
    offset: usize,
}

impl Archive {
    fn add(&mut self, name: &[u8], data: &[u8]) -> io::Result<()> {
        let header = file_header(name, data.len() as u64);
        self.file.write_all(&header)?;
        self.file.write_all(data)?;
        let padding = data.len().div_ceil(BLOCK) * BLOCK - data.len();
        self.file.write_all(&vec![0; padding])?;
        self.offset += header.len() + data.len() + padding;
        Ok(())
    }

    /// `TarFile.close()`: two zero blocks, then zeros up to the next 10240-byte record.
    fn close(&mut self) -> io::Result<()> {
        self.offset += 2 * BLOCK;
        let remainder = self.offset % RECORD;
        let tail = 2 * BLOCK + if remainder > 0 { RECORD - remainder } else { 0 };
        self.file.write_all(&vec![0; tail])
    }
}

fn open_flags(path: &Path, flags: i32) -> io::Result<File> {
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags | libc::O_CLOEXEC)
        .open(path)
}

/// `os.open(name, flags, dir_fd=directory)`.
fn openat(directory: &File, name: &OsStr, flags: i32) -> io::Result<File> {
    let name =
        CString::new(name.as_bytes()).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?;
    // SAFETY: a valid directory descriptor and a NUL-terminated name.
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat returned a new descriptor this process owns.
    Ok(File::from(unsafe { OwnedFd::from_raw_fd(fd) }))
}

/// Read up to `limit` bytes (a buffered `read(limit)`).
fn read_up_to(file: &mut File, limit: usize) -> io::Result<Vec<u8>> {
    let mut data = Vec::new();
    file.take(limit as u64).read_to_end(&mut data)?;
    Ok(data)
}

/// `os.fdopen(fd, 'rb')` of a directory: io.FileIO refuses it naming the descriptor, which it does
/// not close (it did not open it), so later descriptors get higher numbers. Kept as Python does it.
fn directory_error(file: File) -> String {
    use std::os::fd::IntoRawFd;
    format!("[Errno 21] Is a directory: {}", file.into_raw_fd())
}

/// `os.open(path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK)` of a retained source, regular files only.
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

/// `boot_retained.capture`, or its error text (an OSError or ValueError the caller records).
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
    let mounts = mountinfo(root.read("/proc/self/mountinfo").as_deref());
    let pstore_mounted = mounts_at(&mounts, "/sys/fs/pstore")
        .iter()
        .any(|m| m.get("filesystem").and_then(Value::as_str) == Some("pstore"));
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
        let mut archive = Archive { file, offset: 0 };
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
        let manifest = dumps_sorted(&result, Separators::Spaced).map_err(|e| e.to_string())?;
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
    result.insert("captured_monotonic_s".into(), json!(clock::monotonic()));
    Ok(result)
}

/// `y2-platform boot-evidence`.
pub fn run(root: &Root) -> Outcome {
    if root.read("/data/.y2data-schema").as_deref() != Some("1") {
        return Err(Failure::value("data_schema_unavailable"));
    }
    let mounts = mountinfo(root.read("/proc/self/mountinfo").as_deref());
    let data = mounts_at(&mounts, "/data");
    if data.len() != 1
        || Some(super::snapshot::field(data[0], "source"))
            != root.read("/run/y2-data-device").as_deref()
    {
        return Err(Failure::value("verified_data_mount_unavailable"));
    }
    let directory = root.path("/data/system/platform");
    private_directory(&directory)?;
    let boot = root
        .read("/proc/sys/kernel/random/boot_id")
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
    let mut result = Map::new();
    result.insert("schema".into(), json!(1));
    result.insert("captured_on_boot_id".into(), json!(boot));
    result.insert("previous_boot_id".into(), journal_get("boot_id"));
    result.insert("previous_last_stage".into(), journal_get("last_stage"));
    result.insert(
        "previous_orderly_shutdown".into(),
        journal_get("orderly_shutdown"),
    );
    result.insert("reset_cause".into(), Value::Null);
    result.insert(
        "reset_cause_reason".into(),
        json!("no_qualified_retained_register_or_pstore_backend"),
    );
    result.insert("kernel_panic_retention_guaranteed".into(), json!(false));
    result.insert("logs_are_private".into(), json!(true));
    result.insert("reset_status_raw".into(), Value::Null);
    result.insert(
        "record".into(),
        Value::Object(util::record(root, "previous-boot-evidence", Map::new())),
    );
    if let Some(text) = root.read(RESET_STATUS) {
        result.extend(reset_cause(Some(&text), "not_observed"));
    }
    result.insert(
        "loader".into(),
        loader_boot(root.read(BOOT_METADATA).as_deref()),
    );
    let earlier = match previous.get("pmic") {
        Some(Value::Object(pmic)) => pmic.get("lk_long_press_record").cloned(),
        _ => None,
    };
    result.insert(
        "pmic".into(),
        pmic_power_status(root.read(PMIC_POWER_STATUS).as_deref(), earlier.as_ref()),
    );
    let retained = match capture(root, &boot, &journal_get("boot_id")) {
        Ok(value) => Value::Object(value),
        Err(error) => json!({"error": error, "source_entries_deleted": false}),
    };
    result.insert("retained".into(), retained);
    let data_path = root.path("/data");
    let data = open_flags(&data_path, libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .map_err(|e| Failure::os(e, &data_path))?;
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
        if dev(logs).map_err(|e| Failure::os(e, Path::new("logs")))?
            != dev(&data).map_err(|e| Failure::os(e, &data_path))?
        {
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
            let info = file
                .metadata()
                .map_err(|e| Failure::os(e, Path::new(name)))?;
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
                .map_err(|e| Failure::os(e, Path::new(name)))?;
            let text = String::from_utf8_lossy(&raw);
            let lines = splitlines(&text);
            let lines: Vec<Value> = lines[lines.len().saturating_sub(32)..]
                .iter()
                .map(|line| json!(line.chars().take(500).collect::<String>()))
                .collect();
            entries.push(json!({"name": name, "tail_sha256": hex(&raw),
                "source_bytes_at_capture": info.len(), "tail_lines": lines,
                "bounded_tail_not_complete_log": true}));
        }
    }
    result.insert("logs".into(), Value::Array(entries));
    loop {
        let size = dumps(&result, Separators::Spaced)
            .map_err(Failure::value)?
            .len();
        if size <= 96_000 {
            break;
        }
        let logs = result["logs"].as_array_mut().expect("logs");
        let Some(largest) = logs.iter_mut().fold(None::<&mut Value>, |best, item| {
            let length = item["tail_lines"].as_array().map_or(0, Vec::len);
            match best {
                Some(b) if b["tail_lines"].as_array().map_or(0, Vec::len) >= length => Some(b),
                _ => Some(item),
            }
        }) else {
            return Err(Failure::value("max() iterable argument is empty"));
        };
        let lines = largest["tail_lines"].as_array_mut().expect("lines");
        if lines.is_empty() {
            super::crash(&Failure::index_error("pop from empty list"));
        }
        lines.remove(0);
    }
    atomic_json(
        &directory.join("previous-boot-evidence.json"),
        &result,
        true,
    )?;
    Reply::sorted(&result, 0)
}

#[cfg(test)]
#[path = "evidence_tests.rs"]
mod evidence_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archives_match_python_tarfile() {
        // The bytes host Python's tarfile wrote for these members (recorded, Python 3.15).
        let dir = std::env::temp_dir().join(format!("y2-tar-{}", std::process::id()));
        let file = File::create(&dir).unwrap();
        let mut archive = Archive { file, offset: 0 };
        let long = format!("pstore/{}", "x".repeat(120));
        for (name, data) in [
            (&b"pm/state"[..], &b"abc"[..]),
            (long.as_bytes(), &b""[..]),
            ("pstore/\u{e9}".as_bytes(), &b"z"[..]),
            (&b"manifest.json"[..], "{}".repeat(300).as_bytes()),
        ] {
            archive.add(name, data).unwrap();
        }
        archive.close().unwrap();
        drop(archive);
        let ours = fs::read(&dir).unwrap();
        fs::remove_file(&dir).unwrap();
        y2p_core::recorded::check(env!("CARGO_MANIFEST_DIR"), "evidence-tarfile.tar", &ours);
    }

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
