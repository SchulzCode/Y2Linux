//! What the storage commands share beyond their records: reading parsed records, paths below the
//! root (symlinks resolved as far as they exist), block device instances, the observation record of
//! `space`, the media locks, the hand-off with the USB responder (its state, releasing the SD card
//! or a view before an eject) and the `Maintenance` workload lease.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::At;
use crate::reply::{atomic_json, mkdir_parents, Failure};
use serde_json::{json, Map, Value};
use std::{
    fs::{self, File},
    io,
    path::{Component, Path, PathBuf},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
pub use y2p_core::clock::wall;
pub use y2p_core::mounts::{mounts, Mount};
pub use y2p_core::paths::name;
use y2p_core::py::PyValue;
pub use y2p_core::sys::statvfs;
use y2p_core::{
    lock,
    root::{read as read_text, READ_LIMIT},
    Root,
};

pub type Object = Map<String, Value>;

// ---- parsed records ----------------------------------------------------------------------------

/// A field of a record, `null` when it is missing.
pub fn get<'a>(map: &'a Object, key: &str) -> &'a Value {
    map.get(key).unwrap_or(&Value::Null)
}

/// A field equals `value` by JSON value (`1`, `1.0` and `true` alike: records written by older
/// platforms compare as before).
pub fn get_eq(map: &Object, key: &str, value: &Value) -> bool {
    get(map, key).py_eq(value)
}

/// A record struct as the JSON object it serializes to (every struct of this group does).
pub fn to_object<T: serde::Serialize>(value: &T) -> Object {
    match serde_json::to_value(value) {
        Ok(Value::Object(map)) => map,
        _ => Object::new(),
    }
}

/// An optional text as a record keeps it (`null` when missing).
pub fn text(value: &Option<String>) -> Value {
    value.clone().map_or(Value::Null, Value::String)
}

/// Only ASCII digits, at least one (sysfs numbers).
pub fn isdigit(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_digit())
}

/// The JSON object at a device path; empty when it is missing, unreadable or no object.
pub fn object(root: &Root, name: &str) -> Object {
    root.json_object(name)
}

// ---- paths -------------------------------------------------------------------------------------

/// A host path's text, trimmed; `None` when it cannot be read or is too long.
pub fn read(path: &Path) -> Option<String> {
    read_text(path, READ_LIMIT)
}

/// Whether a `/sys/class/block` entry's link names the eMMC controller: its device is then no SD card
/// (sysfs class links name their device's canonical path). One readlink, where `resolve` takes one per
/// path component; the SD inventory and the USB tick look at every mmcblk entry every pass.
pub fn emmc_entry(entry: &Path) -> bool {
    fs::read_link(entry).is_ok_and(|link| {
        link.to_string_lossy()
            .contains(&format!("/{}/", y2p_core::volume::EMMC_CONTROLLER))
    })
}

/// The path with its symlinks followed as far as they exist, the rest kept as written (a removed
/// device's sysfs path still names its controller).
pub fn resolve(path: &Path) -> PathBuf {
    if let Ok(real) = fs::canonicalize(path) {
        return real;
    }
    let mut pending: Vec<PathBuf> = Vec::new();
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    pending.extend(
        absolute
            .components()
            .rev()
            .map(|c| PathBuf::from(c.as_os_str())),
    );
    let mut result = PathBuf::from("/");
    let mut hops = 0;
    while let Some(part) = pending.pop() {
        match Path::new(&part).components().next() {
            Some(Component::RootDir) | Some(Component::CurDir) | None => continue,
            Some(Component::ParentDir) => {
                result.pop();
                continue;
            }
            _ => {}
        }
        let next = result.join(&part);
        match fs::read_link(&next) {
            Ok(target) if hops < 40 => {
                hops += 1;
                if target.is_absolute() {
                    result = PathBuf::from("/");
                }
                pending.extend(
                    target
                        .components()
                        .rev()
                        .map(|c| PathBuf::from(c.as_os_str())),
                );
            }
            _ => result = next,
        }
    }
    result
}

/// The path exists (a symlink to it counts).
pub fn exists(path: &Path) -> bool {
    fs::metadata(path).is_ok()
}

/// The path itself is a symlink.
pub fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// The directory with its parents (`mode` for the last one), fine when it exists.
pub fn mkdir(path: &Path, mode: u32) -> Result<(), Failure> {
    mkdir_parents(path, mode)
}

/// `mkdir` with the I/O error itself.
pub fn mkdir_io(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    if path.is_dir() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::DirBuilder::new().mode(mode).create(path) {
        Err(e) if !(e.kind() == io::ErrorKind::AlreadyExists && path.is_dir()) => Err(e),
        _ => Ok(()),
    }
}

// ---- mounts and block devices ------------------------------------------------------------------

/// One gendisk lifetime, stable across kernfs inode reclamation.
///
/// dev_t selects the partition and diskseq distinguishes removal/reinsertion,
/// including the same CID. Missing diskseq fails closed rather than falling
/// back to an inode/ctime that can change without a media change.
pub fn device_instance(root: &Root, device_id: &str) -> Option<String> {
    let path = fs::canonicalize(root.path(&format!("/sys/dev/block/{device_id}"))).ok()?;
    let disk = if path.join("partition").is_file() {
        path.parent().map(Path::to_path_buf).unwrap_or_default()
    } else {
        path.clone()
    };
    let dev = read(&path.join("dev"));
    let seq = read(&disk.join("diskseq"));
    let dev = dev.filter(|d| d == device_id)?;
    let (major, minor) = dev.split_once(':')?;
    if !isdigit(major) || !isdigit(minor) {
        return None;
    }
    let seq = seq.filter(|s| isdigit(s))?;
    if seq.chars().all(|c| c == '0') {
        return None;
    }
    Some(format!("block:{dev}:diskseq:{seq}"))
}

// ---- records -----------------------------------------------------------------------------------

/// Whole wall-clock seconds since the epoch.
pub fn wall_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Now in UTC as ISO 8601 with microseconds when there are any (`2026-10-29T12:00:00.5+00:00`).
pub use y2p_core::devices_support::utc_isoformat_now as utc_isoformat;

/// The observation record a workload's result carries (build, kernel, boot and clocks).
pub fn observation_record(root: &Root, workload: &str) -> Value {
    let versions = object(root, "/etc/y2linux/versions.json");
    json!({
        "wall_timestamp": utc_isoformat(),
        "monotonic_ns": root.clock().monotonic_ns(),
        "boot_id": Value::from(root.boot_id()),
        "y2linux_commit": get(&versions, "build_git_commit"),
        "reborn_commit": get(&versions, "reborn_source_commit"),
        "kernel": text(&root.read("/proc/sys/kernel/osrelease")),
        "rootfs_release": get(&versions, "rootfs_version"),
        "build_id": text(&root.read("/etc/y2linux/build-id")),
        "workload": workload, "parameters": {}, "units": {},
        "result": null, "failure": null, "evidence_level": "IMPLEMENTED",
    })
}

/// An exclusive lock on a platform lock file if it is free now (`None` when another holds it).
pub fn try_lock(path: &Path) -> Result<Option<File>, Failure> {
    lock::try_exclusive(path).at(path)
}

fn busy() -> Failure {
    Failure::busy()
}

/// `try_lock` with a busy lock an error (`[Errno 11] Resource temporarily unavailable`).
pub fn lock_now(path: &Path) -> Result<File, Failure> {
    try_lock(path)?.ok_or_else(busy)
}

/// A busy lock retried every 0.2 s until `seconds` passed; `None` when it stayed busy.
pub fn lock_within(path: &Path, seconds: f64) -> Result<Option<File>, Failure> {
    lock::exclusive_within(path, Duration::from_secs_f64(seconds)).at(path)
}

// ---- the USB responder ----------------------------------------------------------------------

const USB_SCHEMA: &str = "org.y2linux.usb-transfer/v1";

/// The USB responder's state this boot; one that did not report this boot is `Unavailable`.
pub fn usb_status(root: &Root) -> Object {
    let state = object(root, "/run/y2/usb-transfer.json");
    if !get_eq(&state, "schema", &json!(USB_SCHEMA))
        || !get_eq(&state, "boot_id", &Value::from(root.boot_id()))
    {
        let mut value = Object::new();
        value.insert("schema".into(), json!(USB_SCHEMA));
        value.insert("state".into(), json!("Unavailable"));
        value.insert("reason".into(), json!("service_not_observed"));
        value.insert("filesystem_owner".into(), json!("linux"));
        value.insert("block_export".into(), json!(false));
        return value;
    }
    state
}

/// Whether the USB responder shows `path` to the computer (`views`: a list of `{"path": …}`
/// objects, read up to the first match). Views that cannot be read are refused
/// (`usb_transfer_views_invalid`) rather than guessed: the computer might still hold the card.
fn has_view(current: &Object, path: &str) -> Result<bool, Failure> {
    let invalid = || Failure::value("usb_transfer_views_invalid");
    match current.get("views") {
        None => Ok(false),
        Some(Value::Array(items)) => {
            for item in items {
                match item {
                    Value::Object(view) if get(view, "path").as_str() == Some(path) => {
                        return Ok(true)
                    }
                    Value::Object(_) => {}
                    _ => return Err(invalid()),
                }
            }
            Ok(false)
        }
        // An empty text or object holds no views.
        Some(Value::String(text)) if text.is_empty() => Ok(false),
        Some(Value::Object(map)) if map.is_empty() => Ok(false),
        Some(_) => Err(invalid()),
    }
}

/// Ask the readiness service to re-read the media views now; false when it is gone.
pub fn wake_service(root: &Root) -> bool {
    let pid = root.integer("/run/y2/readiness.pid");
    if let Some(pid) = pid.filter(|p| *p > 1) {
        if root.is_real() {
            let cmdline = root
                .read(&format!("/proc/{pid}/cmdline"))
                .unwrap_or_default();
            if cmdline.contains("service-daemon") && cmdline.contains("y2-platform") {
                let sent = libc::pid_t::try_from(pid)
                    .map_err(|_| io::Error::from_raw_os_error(libc::ESRCH))
                    .and_then(|pid| y2p_core::sys::kill(pid, libc::SIGUSR1));
                if sent.is_err_and(|e| e.raw_os_error() == Some(libc::ESRCH)) {
                    return false;
                }
            }
        }
    }
    true
}

/// Drop the responder's pins on the SD card before a device-side eject; never force an unmount.
///
/// An automatic eject refuses while a host cable is attached. An explicit owner
/// command (format/grow, which name the card by CID) may release the SD view
/// from the responder anyway: the host only sees the storage leave, exactly as
/// it would on removal.
pub fn release_sd(root: &Root, mount_id: i64, explicit: bool) -> Result<bool, Failure> {
    let current = usb_status(root);
    if !has_view(&current, "/media/sd")? {
        return Ok(true);
    }
    if !explicit && get(&current, "cable").as_str() != Some("not attached") {
        return Ok(false);
    }
    atomic_json(
        &root.path("/run/y2/usb-release-sd.json"),
        &json!({"mount_id": mount_id}),
        false,
    )?;
    if !wake_service(root) {
        return Ok(false);
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        let current = usb_status(root);
        if get(&current, "state").as_str() != Some("Failed") && !has_view(&current, "/media/sd")? {
            return Ok(true);
        }
        thread::sleep(Duration::from_millis(50));
    }
    Ok(false)
}

/// Wait until the responder has dropped `path` (its view already left the media views); true when
/// it is not pinned (also when no responder runs).
pub fn release_view(root: &Root, path: &str) -> bool {
    if !wake_service(root) {
        return false;
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let roots = object(root, "/run/y2/mtp-roots.json");
        match roots.get("roots") {
            Some(Value::Array(items)) if items.iter().any(|r| r.as_str() == Some(path)) => {}
            _ => return true,
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

// ---- the Maintenance workload lease ------------------------------------------------------------

/// The scoped `Maintenance` lease (commands::workload), released when dropped.
pub fn maintenance(root: &Root) -> Result<crate::commands::workload::Lease, Failure> {
    crate::commands::workload::Lease::take(root, "Maintenance")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_iso() {
        let stamp = utc_isoformat();
        assert!(
            stamp.ends_with("+00:00") && stamp.as_bytes()[10] == b'T',
            "{stamp}"
        );
    }
}
