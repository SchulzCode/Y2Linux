//! What the storage group's Python leans on outside its own modules, as this group's own copies:
//! Python value rules on parsed records, `Path.resolve`, `observe.mountinfo` (all fields),
//! `observe.device_instance`, `ctx.record`, `os.statvfs`, the USB responder hand-off
//! (`usb.status`, `release_sd`, `release_view`, `wake_service`) and the `Maintenance` workload lease.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::{atomic_json, mkdir_parents, private_directory, Failure};
use serde_json::{json, Map, Value};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Component, Path, PathBuf},
    sync::{mpsc, Arc},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
pub use y2p_core::mounts::{mounts, Mount};
use y2p_core::py::PyValue;
pub use y2p_core::sys::statvfs;
use y2p_core::{
    lock,
    root::{read as read_text, READ_LIMIT},
    Root,
};

pub type Object = Map<String, Value>;

// ---- Python values ---------------------------------------------------------------------------

/// `value.<attribute>` on a value that has no such attribute: Python's uncaught AttributeError.
pub fn no_attribute(value: &Value, attribute: &str) -> Failure {
    Failure::attribute_error(format!(
        "'{}' object has no attribute '{attribute}'",
        PyValue::type_name(value)
    ))
}

/// `d.get(key)` (None when missing).
pub fn get<'a>(map: &'a Object, key: &str) -> &'a Value {
    map.get(key).unwrap_or(&Value::Null)
}

/// `d.get(key) == value`.
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

/// An optional text as Python's `str | None`.
pub fn text(value: &Option<String>) -> Value {
    value.clone().map_or(Value::Null, Value::String)
}

/// `str.isdigit()` for the ASCII text sysfs gives (accepted difference: other Unicode digits).
pub fn isdigit(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_digit())
}

/// `ctx.json(name, {}) or {}` and `ctx.json(name, {})`: an object, empty for anything else.
pub fn object(root: &Root, name: &str) -> Object {
    root.json_object(name)
}

// ---- paths -------------------------------------------------------------------------------------

/// `common.read` of a host path.
pub fn read(path: &Path) -> Option<String> {
    read_text(path, READ_LIMIT)
}

/// `Path.resolve()` (not strict): symlinks followed as far as they exist, the rest kept as written.
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

/// `Path.exists()` (follows symlinks).
pub fn exists(path: &Path) -> bool {
    fs::metadata(path).is_ok()
}

/// `Path.is_symlink()`.
pub fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// The file name as `Path.name`.
pub fn name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `Path.mkdir(mode=mode, parents=True, exist_ok=True)`.
pub fn mkdir(path: &Path, mode: u32) -> Result<(), Failure> {
    mkdir_parents(path, mode)
}

// ---- mounts and block devices ------------------------------------------------------------------

/// `observe.device_instance`: one gendisk lifetime, stable across kernfs inode reclamation.
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

/// `time.time()`.
pub fn wall() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// `int(time.time())`.
pub fn wall_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// `datetime.datetime.now(datetime.timezone.utc).isoformat()`.
pub fn utc_isoformat() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let (seconds, micros) = (now.as_secs() as i64, now.subsec_micros());
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let clock = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    );
    if micros == 0 {
        format!("{clock}+00:00")
    } else {
        format!("{clock}.{micros:06}+00:00")
    }
}

/// `ctx.record(workload)`.
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

/// `fcntl.flock(fd, LOCK_EX | LOCK_NB)` on a platform lock file; a busy lock is Python's uncaught
/// `BlockingIOError`.
pub fn try_lock(path: &Path) -> Result<Option<File>, Failure> {
    lock::try_exclusive(path).map_err(|e| Failure::os(e, path))
}

pub fn busy() -> Failure {
    let (text, class) = y2p_core::py::os_error(&io::Error::from_raw_os_error(libc::EAGAIN), None);
    Failure::Os { text, class }
}

/// `try_lock` that raises `BlockingIOError` when busy.
pub fn lock_now(path: &Path) -> Result<File, Failure> {
    try_lock(path)?.ok_or_else(busy)
}

/// A busy lock retried every 0.2 s until `seconds` passed; `None` when it stayed busy.
pub fn lock_within(path: &Path, seconds: f64) -> Result<Option<File>, Failure> {
    lock::exclusive_within(path, Duration::from_secs_f64(seconds)).map_err(|e| Failure::os(e, path))
}

// ---- the USB responder (usb.py) ----------------------------------------------------------------

const USB_SCHEMA: &str = "org.y2linux.usb-transfer/v1";

/// `usb.status`.
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

/// `any(v.get('path') == path for v in current.get('views', []))`, with Python's crashes: a view
/// that is not a dict (also each character of a str, each key of a dict) has no `get`, a number,
/// bool or null is not iterable.
fn has_view(current: &Object, path: &str) -> Result<bool, Failure> {
    let empty = Value::Array(Vec::new());
    let views = current.get("views").unwrap_or(&empty);
    let string = Value::String(String::new());
    match views {
        Value::Array(items) => {
            for item in items {
                match item {
                    Value::Object(view) if get(view, "path").as_str() == Some(path) => {
                        return Ok(true)
                    }
                    Value::Object(_) => {}
                    other => return Err(no_attribute(other, "get")),
                }
            }
            Ok(false)
        }
        Value::String(s) if s.is_empty() => Ok(false),
        Value::Object(m) if m.is_empty() => Ok(false),
        Value::String(_) | Value::Object(_) => Err(no_attribute(&string, "get")),
        other => Err(Failure::type_error(format!(
            "'{}' object is not iterable",
            PyValue::type_name(other)
        ))),
    }
}

/// `usb.wake_service`: ask the readiness service to re-read the media views now.
pub fn wake_service(root: &Root) -> bool {
    let pid = root.integer("/run/y2/readiness.pid");
    if let Some(pid) = pid.filter(|p| *p > 1) {
        if root.is_real() {
            let cmdline = root
                .read(&format!("/proc/{pid}/cmdline"))
                .unwrap_or_default();
            if cmdline.contains("service-daemon") && cmdline.contains("y2-platform") {
                // SAFETY: a plain signal to the service daemon.
                if unsafe { libc::kill(pid as libc::pid_t, libc::SIGUSR1) } != 0
                    && io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
                {
                    return false;
                }
            }
        }
    }
    true
}

/// `usb.release_sd`: drop responder root pins before a device-side eject; never force umount.
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

/// `usb.release_view`: wait until the responder has dropped `path` (its view already left
/// media_views); True when it is not pinned (also when no responder runs).
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

// ---- workload.workload('Maintenance') ------------------------------------------------------------

/// The scoped `Maintenance` QoS lease (workload.py): the shared activity lease (a mandatory
/// exclusion of sleep) and the optional `/dev/y2-workload` hint, renewed every second on its own
/// thread while the work runs. Released when dropped.
pub struct Lease {
    _activity: File,
    stop: Option<mpsc::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for Lease {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn maintenance(root: &Root) -> Result<Lease, Failure> {
    // Unlike optional frequency hints this is a mandatory operation
    // exclusion. flock dies with the process; a leftover file is inert.
    let directory = root.path("/run/y2");
    private_directory(&directory)?;
    let path = directory.join("activity.lock");
    let activity = match lock::try_shared(&path) {
        Ok(Some(file)) => file,
        Ok(None) => return Err(Failure::value("sleep_operation_in_progress")),
        Err(error) => return Err(Failure::os(error, &path)),
    };
    let payload = b"Maintenance 3000\n";
    let mut lease = Lease {
        _activity: activity,
        stop: None,
        worker: None,
    };
    // QoS is optional; never fail a transfer on a hint.
    if let Ok(fd) = OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(root.path("/dev/y2-workload"))
    {
        let fd = Arc::new(fd);
        if (&*fd).write(payload).is_ok_and(|n| n == payload.len()) {
            // Acquire before work starts; renew on an independent worker
            // even when a blocking transfer subprocess runs for minutes.
            let (sender, receiver) = mpsc::channel::<()>();
            let writer = Arc::clone(&fd);
            lease.worker = Some(thread::spawn(move || loop {
                if !(&*writer).write(payload).is_ok_and(|n| n == payload.len()) {
                    return;
                }
                if !matches!(
                    receiver.recv_timeout(Duration::from_secs(1)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    return;
                }
            }));
            lease.stop = Some(sender);
        }
    }
    Ok(lease)
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
