//! `update.py`: signed full-system OTA (OTA-35): check, download, install request, boot health.
//!
//! The small native core (`/usr/sbin/y2-update-core`, C) is the shared authentication/policy
//! authority. It writes the RECOVERY image and the MISC request; the Y2 recovery image performs the
//! offline backup/write/readback of BOOTIMG and Y2ROOT. This module never opens a block device itself.
//! `/data/updates/state.json` is the core's journal; `download.json`, `last-check.json` and
//! `/run/y2/update-progress.json` are this module's records, read by Reborn (`client.rs`).
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    download::{self, Error as Net, Response, CANCELLED},
    python::{int_or_float, is_one_of, key_order, lock_held, prefix, str_of},
    storage,
};
use crate::reply::{atomic_json, private_directory, Failure, Outcome, Reply};
use serde_json::{json, Map, Value};
use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::Duration,
};
use y2p_core::{
    clock, command,
    json::Ordered,
    py::{self, is_true, truthy},
    record,
    service_digest::Sha256,
    Root,
};

const FILES: [&str; 5] = [
    "manifest.json",
    "manifest.sig",
    "rootfs.ext4.gz",
    "bootimg.img.gz",
    "recovery.img.gz",
];
const TERMINAL: [&str; 4] = ["Idle", "Acknowledged", "RolledBack", "Failed"];
const RESERVE: i128 = 96 * 1024 * 1024;
const CHANNEL: &str = "/etc/y2linux/update-channel.json";
/// Owner gate before the device turns off to install (decision 2026-10-15).
const MINIMUM_PERCENT: i64 = 50;
const CHECK_INTERVAL: f64 = 24.0 * 3600.0;
/// Wi-Fi needs 15-20 s to reconnect after the player wakes; a Check for Updates pressed meanwhile
/// failed at once ("name resolution") and hid Update Now.
const NETWORK_WAIT_S: f64 = 30.0;
const MANIFEST_ATTEMPTS: u32 = 3;
const PROGRESS: &str = "/run/y2/update-progress.json";
const CHECK_PROGRESS: &str = "/run/y2/update-check-progress";
/// Rollback copies of the running system. The recovery deletes them right before it writes the new
/// ones, so they count as space the update can use.
const BACKUPS: [&str; 2] = ["previous.ext4.gz", "previous-bootimg.gz"];
/// Kept in /data/logs: the live system log and the last installer run.
const KEPT_LOGS: [&str; 4] = [
    "system.log",
    "recovery-last.log",
    "recovery-display.txt",
    "recovery-previous-boot.txt",
];
/// Owner decision (2026-10-28): the running system restarts 10 s after a kernel panic; init sets it
/// before the handover on every boot (the command line keeps panic=0 for the rescue).
const RUNNING_PANIC_SECONDS: &str = "10";

fn payload_file(key: &str) -> Option<&'static str> {
    match key {
        "rootfs" => Some("rootfs.ext4.gz"),
        "bootimg" => Some("bootimg.img.gz"),
        "recovery" => Some("recovery.img.gz"),
        _ => None,
    }
}

/// What the Python code raises, by the class its callers catch.
#[derive(Debug)]
pub enum Raise {
    Value(String),
    Os(String),
    /// `NoSpace(needed)`: a ValueError `update_data_space_reserve` with the bytes the update needs.
    NoSpace(i128),
    Net(Net),
    /// Let the Python platform do the whole command (nothing changed yet).
    Fallback,
}

impl Raise {
    pub fn text(&self) -> String {
        match self {
            Raise::Value(text) | Raise::Os(text) => text.clone(),
            Raise::NoSpace(_) => "update_data_space_reserve".into(),
            Raise::Net(error) => error.text(),
            Raise::Fallback => String::new(),
        }
    }

    fn value(text: &str) -> Raise {
        Raise::Value(text.into())
    }

    fn os(error: std::io::Error, path: &Path) -> Raise {
        Raise::Os(py::os_error(&error, Some(&path.to_string_lossy())).0)
    }

    fn cancelled(&self) -> bool {
        matches!(self, Raise::Net(Net::Cancelled))
    }
}

impl From<Failure> for Raise {
    fn from(failure: Failure) -> Raise {
        match failure {
            Failure::Fallback => Raise::Fallback,
            Failure::Value(text) => Raise::Value(text),
            Failure::Os { text, .. } => Raise::Os(text),
            // An uncaught exception in a helper of another group: Python crashes the same way.
            Failure::Crash { .. } => Raise::Fallback,
        }
    }
}

type Result<T> = std::result::Result<T, Raise>;

fn object(value: Option<Value>) -> Map<String, Value> {
    match value {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

fn boot_id(root: &Root) -> Value {
    json!(root.read("/proc/sys/kernel/random/boot_id"))
}

/// `update.channel`: the shipped release channel, or the owner's override on /data.
pub fn channel(root: &Root) -> Map<String, Value> {
    let shipped = root.json_object(CHANNEL);
    let mut value = shipped.clone();
    for (key, item) in root.json_object("/data/updates/channel.json") {
        if key == "manifest_url" || key == "name" {
            value.insert(key, item);
        }
    }
    let hosts = match shipped.get("allowed_redirect_hosts") {
        Some(Value::Array(items))
            if items
                .iter()
                .all(|h| matches!(h, Value::String(s) if !s.is_empty())) =>
        {
            Value::Array(items.clone())
        }
        _ => json!([]),
    };
    value.insert("allowed_redirect_hosts".into(), hosts);
    value
}

fn hosts(root: &Root) -> Vec<String> {
    match channel(root).get("allowed_redirect_hosts") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|h| h.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// `update.download_record`: download.json, with a download or check whose process is gone shown as
/// interrupted. A user's player (8.17) said "Downloading Update… Keep Wi-Fi on." for a day: the record
/// a download writes first stays `Staging` when the download dies without reaching its own handler
/// (power loss, a kill), and that page offered neither Cancel nor Check for Updates; a restart kept
/// it (it is on /data).
fn download_record(root: &Root) -> Value {
    let record = root
        .json("/data/updates/download.json")
        .unwrap_or(Value::Null);
    if is_one_of(record.get("state"), &["Staging", "Checking"])
        && record.is_object()
        && !lock_held(root, &root.path("/data/updates/.stage.lock"))
    {
        return json!({"schema": 1, "state": "Failed", "failure": "download_interrupted"});
    }
    record
}

/// `update.status`.
pub fn status(root: &Root) -> Map<String, Value> {
    let path = root.path("/data/updates/state.json");
    let mut value = root.json("/data/updates/state.json");
    if value.is_none() && !path.exists() && path.symlink_metadata().is_err() {
        let ready = root.read("/data/.y2data-schema").as_deref() == Some("1")
            && root.path("/data/updates").is_dir();
        value = Some(
            json!({"schema": 1, "state": if ready { "Idle" } else { "Unavailable" },
                            "failure": if ready { Value::Null } else { json!("data_state_unavailable") }}),
        );
    }
    // The native core writes journal schema 2 since OTA-35 (BOOTIMG + root).
    let mut value = match value {
        Some(Value::Object(map))
            if py::eq_int(map.get("schema"), 1) || py::eq_int(map.get("schema"), 2) =>
        {
            map
        }
        _ => {
            return object(Some(
                json!({"schema": 1, "state": "Failed", "failure": "journal_unreadable"}),
            ))
        }
    };
    value.insert("download".into(), download_record(root));
    value.insert(
        "last_check".into(),
        root.json("/data/updates/last-check.json")
            .unwrap_or(Value::Null),
    );
    value.insert(
        "channel".into(),
        channel(root).get("name").cloned().unwrap_or(Value::Null),
    );
    value.insert("full_system".into(), json!(true));
    value.insert("physical_qualification".into(), json!(false));
    value
}

fn state_is(map: &Map<String, Value>, choices: &[&str]) -> bool {
    is_one_of(map.get("state"), choices)
}

/// Python's `json.loads` error text for the core's output (a ValueError).
fn json_error(text: &str, error: &serde_json::Error) -> String {
    let column = error.column();
    let line = error.line();
    let offset: usize = text
        .split('\n')
        .take(line.saturating_sub(1))
        .map(|l| l.chars().count() + 1)
        .sum::<usize>()
        + column.saturating_sub(1);
    if text.trim().is_empty() {
        return "Expecting value: line 1 column 1 (char 0)".into();
    }
    if error.to_string().starts_with("trailing characters") {
        return format!("Extra data: line {line} column {column} (char {offset})");
    }
    format!("Expecting value: line {line} column {column} (char {offset})")
}

/// `update.core`: one action of the native core; its refusal becomes the error text.
pub fn core(
    root: &Root,
    action: &str,
    directory: Option<&str>,
    timeout: f64,
) -> Result<(Map<String, Value>, String)> {
    let mut argv = vec!["/usr/sbin/y2-update-core", action];
    argv.extend(directory);
    let result = command::run_with(root, &argv, timeout, 20000, true);
    if !result.ok {
        let refusal = serde_json::from_str::<Value>(
            result
                .output
                .as_deref()
                .filter(|o| !o.is_empty())
                .unwrap_or("{}"),
        )
        .ok()
        .and_then(|v| v.get("failure").cloned())
        .filter(truthy);
        return Err(Raise::Value(match refusal {
            Some(value) => str_of(&value),
            None => format!(
                "update_core_{}",
                result
                    .reason
                    .as_deref()
                    .filter(|r| !r.is_empty())
                    .unwrap_or("refused")
            ),
        }));
    }
    let output = result.output.unwrap_or_default();
    match serde_json::from_str::<Value>(&output) {
        Ok(Value::Object(map)) => Ok((map, output)),
        Ok(_) => Err(Raise::value("invalid_update_core_response")),
        Err(error) => Err(Raise::Value(json_error(&output, &error))),
    }
}

// ---- space ---------------------------------------------------------------------------------------

struct Space {
    readonly: bool,
    available: i128,
    files: u64,
    favail: u64,
}

fn fstatvfs(file: &File) -> Result<Space> {
    use std::os::fd::AsRawFd;
    // SAFETY: a zeroed statvfs filled by the kernel for an open descriptor.
    let mut info: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstatvfs(file.as_raw_fd(), &mut info) } != 0 {
        return Err(Raise::Os(
            py::os_error(&std::io::Error::last_os_error(), None).0,
        ));
    }
    Ok(Space {
        readonly: info.f_flag & libc::ST_RDONLY != 0,
        available: info.f_bavail as i128 * info.f_frsize as i128,
        files: info.f_files as u64,
        favail: info.f_favail as u64,
    })
}

/// `update.free`: the volume must keep the reserve after `size` more bytes.
fn free(file: &File, size: i128) -> Result<()> {
    let info = fstatvfs(file)?;
    if info.readonly || info.available < size + RESERVE || (info.files != 0 && info.favail < 128) {
        return Err(Raise::value("update_data_space_reserve"));
    }
    Ok(())
}

fn sorted_entries(directory: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = fs::read_dir(directory)
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    entries.sort();
    entries
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `update.disposable`: data the player can lose without losing anything a user sees: rotated and
/// diagnostic logs, rotated platform journals and finished qualification runs. Never music,
/// settings, the library or the rollback copies.
fn disposable(root: &Root) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let logs = root.path("/data/logs");
    if logs.is_dir() {
        found.extend(
            sorted_entries(&logs)
                .into_iter()
                .filter(|p| !KEPT_LOGS.contains(&name_of(p).as_str())),
        );
    }
    let platform = root.path("/data/system/platform");
    if platform.is_dir() {
        found.extend(sorted_entries(&platform).into_iter().filter(|p| {
            let name = name_of(p);
            name.ends_with(".previous.jsonl")
                || name == "cpu-idle-qualification"
                || name.starts_with("fix01-")
                || name.starts_with("fix02-")
        }));
    }
    let system = root.path("/data/system");
    if system.is_dir() {
        found.extend(
            sorted_entries(&system)
                .into_iter()
                .filter(|p| y2p_core::root::fnmatch("cpu-final-qualification-*", &name_of(p))),
        );
    }
    found
}

fn tree_bytes(path: &Path) -> u64 {
    let mut total = 0;
    for entry in fs::read_dir(path).into_iter().flatten().flatten() {
        let Ok(meta) = entry.path().symlink_metadata() else {
            continue;
        };
        if meta.file_type().is_dir() {
            total += tree_bytes(&entry.path());
        } else if meta.file_type().is_file() {
            total += meta.len();
        }
    }
    total
}

/// `update.remove_disposable`: one entry without following links; the bytes freed.
fn remove_disposable(path: &Path) -> std::io::Result<u64> {
    let info = path.symlink_metadata()?;
    if info.file_type().is_dir() {
        let size = tree_bytes(path);
        fs::remove_dir_all(path)?;
        return Ok(size);
    }
    fs::remove_file(path)?;
    Ok(if info.file_type().is_file() {
        info.len()
    } else {
        0
    })
}

/// `update.make_room` (OTA-46): count the rollback copies the recovery replaces, then remove
/// disposable data until the download fits. Returns the bytes removed; `NoSpace` with the total the
/// update needs when even that is not enough.
fn make_room(root: &Root, file: &File, size: i128) -> Result<i128> {
    let directory = root.path("/data/updates");
    let mut reclaimable: i128 = 0;
    for name in BACKUPS {
        if let Ok(meta) = directory.join(name).symlink_metadata() {
            if meta.file_type().is_file() {
                reclaimable += meta.len() as i128;
            }
        }
    }
    let need = size + RESERVE - reclaimable;
    let fits = || -> Result<bool> {
        let info = fstatvfs(file)?;
        Ok(!info.readonly && info.available >= need && !(info.files != 0 && info.favail < 128))
    };
    let mut removed: i128 = 0;
    if fits()? {
        return Ok(removed);
    }
    for path in disposable(root) {
        if let Ok(size) = remove_disposable(&path) {
            removed += size as i128;
        }
        if fits()? {
            return Ok(removed);
        }
    }
    // Last: the kept package of the running release (OTA-50). Without it the recovery backs up the
    // live root instead, which is only slower.
    let kept = directory.join("current");
    let size: Option<i128> = if kept.is_dir() {
        fs::read_dir(&kept).ok().map(|entries| {
            entries
                .flatten()
                .map(|e| e.path().symlink_metadata().map_or(0, |m| m.len() as i128))
                .sum()
        })
    } else {
        Some(0)
    };
    if let Some(size) = size {
        if remove_package(&kept).is_ok() {
            removed += size;
            if fits()? {
                return Ok(removed);
            }
        }
    }
    Err(Raise::NoSpace(need))
}

/// `update.remove_package`: only these known disposable files; unexpected contents stop cleanup.
fn remove_package(directory: &Path) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    let meta = directory
        .symlink_metadata()
        .map_err(|e| Raise::os(e, directory))?;
    if !meta.file_type().is_dir() {
        return Err(Raise::value("unsafe_staging_directory"));
    }
    let names: Vec<String> = fs::read_dir(directory)
        .map_err(|e| Raise::os(e, directory))?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    if names.iter().any(|n| !FILES.contains(&n.as_str())) {
        return Err(Raise::value("unknown_staging_contents"));
    }
    for name in &names {
        let file = directory.join(name);
        let info = file.symlink_metadata().map_err(|e| Raise::os(e, &file))?;
        let links = fs::metadata(&file)
            .map_err(|e| Raise::os(e, &file))?
            .nlink();
        if !info.file_type().is_file() || links != 1 {
            return Err(Raise::value("unsafe_staging_file"));
        }
    }
    for name in &names {
        let file = directory.join(name);
        fs::remove_file(&file).map_err(|e| Raise::os(e, &file))?;
    }
    fs::remove_dir(directory).map_err(|e| Raise::os(e, directory))
}

// ---- downloads -----------------------------------------------------------------------------------

/// Live download progress for Reborn in RAM (OTA-48): bytes, speed over the last few seconds and time
/// left, at most four times a second. The durable journal on Y2DATA only records the start and the
/// result.
struct Progress {
    path: PathBuf,
    release: Value,
    total: i64,
    samples: VecDeque<(f64, i64)>,
    written: f64,
    done: i64,
}

impl Progress {
    const WINDOW: f64 = 4.0;

    fn new(root: &Root, release: Value, total: i64) -> Self {
        Self {
            path: root.path(PROGRESS),
            release,
            total,
            samples: VecDeque::new(),
            written: 0.0,
            done: 0,
        }
    }

    fn publish(&mut self, phase: &str, force: bool) {
        let now = clock::monotonic();
        if !force && now - self.written < 0.25 {
            return;
        }
        self.written = now;
        self.samples.push_back((now, self.done));
        while self.samples.len() > 2 && now - self.samples[0].0 > Self::WINDOW {
            self.samples.pop_front();
        }
        let ((t0, b0), (t1, b1)) = (self.samples[0], self.samples[self.samples.len() - 1]);
        let speed = (t1 - t0 >= 0.5).then(|| ((b1 - b0) as f64 / (t1 - t0)) as i64);
        let left = speed
            .filter(|s| *s != 0 && phase == "download")
            .map(|s| ((self.total - self.done) as f64 / s as f64) as i64);
        let value = json!({"schema": 1, "phase": phase, "release_version": self.release,
                           "bytes_done": self.done, "bytes_total": self.total,
                           "speed_bytes_per_s": speed, "seconds_left": left});
        let _ = record::atomic_json(&self.path, &value, false, 0o644);
    }

    fn finish(&mut self, phase: &str) {
        self.publish(phase, true);
    }
}

/// A source of package files: the owner's local directory or the release channel over HTTPS.
enum Source {
    Local(PathBuf),
    Remote { base: String, hosts: Vec<String> },
}

enum Stream {
    File(File),
    Http(Response),
}

impl Stream {
    fn read(&mut self, limit: usize, cancellable: bool) -> Result<Vec<u8>> {
        match self {
            Stream::File(file) => {
                if cancellable && CANCELLED.load(Ordering::SeqCst) {
                    return Err(Raise::Net(Net::Cancelled));
                }
                let mut out = Vec::new();
                Read::by_ref(file)
                    .take(limit as u64)
                    .read_to_end(&mut out)
                    .map_err(|e| Raise::Os(py::os_error(&e, None).0))?;
                Ok(out)
            }
            Stream::Http(response) => response.read(limit, cancellable).map_err(Raise::Net),
        }
    }
}

/// `str(directory / name)` as pathlib spells it (no empty or `.` components).
fn pathlib(directory: &Path, name: &str) -> PathBuf {
    let text = directory.to_string_lossy();
    let mut out = String::new();
    if text.starts_with('/') {
        out.push('/');
    }
    let parts: Vec<&str> = text
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .chain([name])
        .collect();
    out.push_str(&parts.join("/"));
    PathBuf::from(out)
}

impl Source {
    /// `get(name)`: the opened response or file.
    fn get(&self, name: &str, cancellable: bool) -> Result<Stream> {
        match self {
            Source::Remote { base, hosts } => {
                let response = download::open(&format!("{base}{name}"), hosts, cancellable)
                    .map_err(Raise::Net)?;
                if response.status != 200 {
                    return Err(Raise::value("HTTP_status"));
                }
                Ok(Stream::Http(response))
            }
            Source::Local(directory) => {
                let path = pathlib(directory, name);
                let file = OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                    .open(&path)
                    .map_err(|e| Raise::os(e, &path))?;
                let meta = file
                    .metadata()
                    .map_err(|e| Raise::Os(py::os_error(&e, None).0))?;
                if !meta.file_type().is_file() {
                    return Err(Raise::value("local_package_regular_files_required"));
                }
                Ok(Stream::File(file))
            }
        }
    }
}

/// `update.copy_stream`: at most `maximum` bytes (exactly `exact`), within `seconds`, keeping the
/// reserve; returns the count and the SHA-256. Interrupted bytes stay for explicit diagnostics.
fn copy_stream(
    source: &mut Stream,
    destination: &Path,
    maximum: i64,
    exact: Option<i64>,
    seconds: f64,
    cancellable: bool,
    mut report: impl FnMut(i64),
) -> Result<(i64, String)> {
    let mut digest = Sha256::new();
    let mut stream = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(destination)
        .map_err(|e| Raise::os(e, destination))?;
    let mut count: i64 = 0;
    let deadline = clock::monotonic() + seconds;
    loop {
        if clock::monotonic() >= deadline {
            return Err(Raise::value("download_deadline"));
        }
        let want = (1024 * 1024).min(maximum - count + 1).max(0) as usize;
        let chunk = source.read(want, cancellable)?;
        if chunk.is_empty() {
            break;
        }
        count += chunk.len() as i64;
        if count > maximum {
            return Err(Raise::value("download_size_limit"));
        }
        free(&stream, chunk.len() as i128)?;
        stream
            .write_all(&chunk)
            .map_err(|e| Raise::Os(py::os_error(&e, None).0))?;
        digest.update(&chunk);
        report(count);
    }
    if exact.is_some_and(|e| e != count) {
        return Err(Raise::value("incomplete_download"));
    }
    stream
        .sync_all()
        .map_err(|e| Raise::Os(py::os_error(&e, None).0))?;
    Ok((count, digest.hexdigest()))
}

/// `update.wait_online`: true once Wi-Fi is Online (DNS answered), waiting at most `seconds`.
fn wait_online(root: &Root, seconds: f64) -> bool {
    let deadline = clock::monotonic() + seconds;
    while root.json_object("/run/y2/network.json").get("state") != Some(&json!("Online")) {
        if clock::monotonic() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    true
}

/// The SIGTERM disposition while `stage` runs (`signal.signal(SIGTERM, …)`), restored when it ends.
struct Terminate {
    previous: libc::sigaction,
}

extern "C" fn cancel(_: libc::c_int) {
    CANCELLED.store(true, Ordering::SeqCst);
}

impl Terminate {
    fn save() -> Self {
        // SAFETY: reading the current action into a zeroed struct.
        let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
        unsafe { libc::sigaction(libc::SIGTERM, std::ptr::null(), &mut previous) };
        Self { previous }
    }

    fn set(handler: libc::sighandler_t) {
        // SAFETY: a plain handler (an atomic store) or SIG_IGN, no SA_RESTART so reads are interrupted.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = handler;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut());
        }
    }
}

impl Drop for Terminate {
    fn drop(&mut self) {
        // SAFETY: restoring the action saved at the start.
        unsafe { libc::sigaction(libc::SIGTERM, &self.previous, std::ptr::null_mut()) };
    }
}

fn installed_release(root: &Root) -> Value {
    root.json_object("/etc/y2linux/versions.json")
        .get("release_version")
        .cloned()
        .unwrap_or(Value::Null)
}

/// The staging token, `.incoming-<uuid4 hex>`; below a fixture root a test may name it
/// (`Y2_FIXTURE_TOKEN`, tests/platform_cases/service.py patches Python's uuid4 the same way).
fn token(root: &Root) -> Result<String> {
    if !root.is_real() {
        if let Ok(fixed) = std::env::var("Y2_FIXTURE_TOKEN") {
            return Ok(format!(".incoming-{fixed}"));
        }
    }
    let id = py::uuid4().map_err(|e| Raise::Os(py::os_error(&e, None).0))?;
    Ok(format!(".incoming-{}", id.replace('-', "")))
}

struct Staging<'a> {
    root: &'a Root,
    directory: PathBuf,
    incoming: PathBuf,
    token: String,
    check_only: bool,
    live: Option<Progress>,
    summary: Value,
    previous: Value,
}

fn malformed() -> Raise {
    // A core answer without the manifest fields is a traceback in Python (KeyError/TypeError):
    // CONTRACT.md, accepted differences (only a broken core gives one).
    Raise::value("invalid_update_core_response")
}

impl Staging<'_> {
    fn run(
        &mut self,
        file: &File,
        url: Option<&str>,
        source: Option<&str>,
        volume: &storage::Volume,
    ) -> Result<Value> {
        let root = self.root;
        let download = self.directory.join("download.json");
        // DEFECT (CONTRACT.md): a second `update check/stage` meeting the busy .stage.lock overwrites
        // download.json with a Failed `[Errno 11]` while the first download continues.
        match file.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(Raise::Os(
                    py::os_error(&std::io::Error::from_raw_os_error(libc::EAGAIN), None).0,
                ))
            }
            Err(std::fs::TryLockError::Error(e)) => {
                return Err(Raise::Os(py::os_error(&e, None).0))
            }
        }
        if !state_is(&status(root), &TERMINAL) {
            return Err(Raise::value("update_already_pending"));
        }
        // An earlier interrupted download is disposable; bounded, named cleanup.
        let old: Vec<PathBuf> = fs::read_dir(&self.directory)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| {
                        y2p_core::root::fnmatch(".incoming-*", &e.file_name().to_string_lossy())
                    })
                    .map(|e| e.path())
                    .collect()
            })
            .unwrap_or_default();
        if old.len() > 8 {
            return Err(Raise::value("excess_staging_owner_inspection_required"));
        }
        for item in &old {
            remove_package(item)?;
        }
        // A cancelled or failed package is never installed; free its space before this download is
        // measured (OTA-46).
        if !self.check_only {
            remove_package(&self.directory.join("pending"))?;
        }
        // Even the manifest needs the reserve: a nearly full volume is cleaned first, so a check can
        // still find the release (OTA-46).
        let mut cleaned = make_room(root, file, 64 * 1024)?;
        // What the Software Update page showed: a failed check keeps it.
        self.previous = root
            .json("/data/updates/download.json")
            .unwrap_or(Value::Null);
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&self.incoming)
            .map_err(|e| Raise::os(e, &self.incoming))?;
        // A check only reads the signed manifest; it never shows as a download.
        atomic_json(
            &download,
            &json!({"schema": 1, "state": if self.check_only { "Checking" } else { "Staging" },
                    "id": self.token}),
            true,
        )?;
        let source = match url {
            Some(url) => {
                let clock = super::time_status(root);
                if !clock.get("tls_ready").is_some_and(truthy)
                    || !clock.get("entropy_ready").is_some_and(truthy)
                {
                    return Err(Raise::value("clock_or_entropy_not_ready_for_TLS"));
                }
                if !wait_online(root, NETWORK_WAIT_S) {
                    return Err(Raise::value("network_not_online"));
                }
                let base = download::base_url(url).map_err(Raise::Value)?;
                Source::Remote {
                    base,
                    hosts: hosts(root),
                }
            }
            None => {
                let directory = PathBuf::from(source.unwrap_or_default());
                let shown = pathlib(&directory, "");
                let shown = PathBuf::from(shown.to_string_lossy().trim_end_matches('/'));
                let meta = directory
                    .symlink_metadata()
                    .map_err(|e| Raise::os(e, &shown))?;
                if !meta.file_type().is_dir() {
                    return Err(Raise::value("local_package_directory_required"));
                }
                Source::Local(directory)
            }
        };
        let remote = matches!(source, Source::Remote { .. });
        for (name, maximum) in [("manifest.json", 16384), ("manifest.sig", 64)] {
            for attempt in 1..=MANIFEST_ATTEMPTS {
                let copied = source.get(name, false).and_then(|mut stream| {
                    let exact = name.ends_with(".sig").then_some(64);
                    copy_stream(
                        &mut stream,
                        &self.incoming.join(name),
                        maximum,
                        exact,
                        30.0,
                        false,
                        |_| {},
                    )
                });
                match copied {
                    Ok(_) => break,
                    Err(Raise::Net(Net::Http { code, .. }))
                        if self.check_only && code == 404 && name == "manifest.json" =>
                    {
                        // Nothing published on the channel yet: nothing newer to install.
                        remove_package(&self.incoming)?;
                        let installed = installed_release(root);
                        atomic_json(
                            &download,
                            &json!({"schema": 1, "state": "Checked", "up_to_date": true,
                                    "release_version": installed, "published": false}),
                            false,
                        )?;
                        return Ok(json!({"state": "UpToDate", "release_version": installed}));
                    }
                    Err(Raise::Net(Net::Url(_) | Net::Timeout))
                        if remote && attempt != MANIFEST_ATTEMPTS =>
                    {
                        // A lookup or handshake that failed while Wi-Fi was still settling: try again
                        // once it is online.
                        let _ = fs::remove_file(self.incoming.join(name));
                        std::thread::sleep(Duration::from_secs(3));
                        wait_online(root, 10.0);
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        let device = format!("/data/updates/{}", self.token);
        let (manifest, text) = match core(root, "check-manifest", Some(&device), 15.0) {
            Ok(answer) => answer,
            Err(Raise::Value(error)) if self.check_only && error.contains("downgrade/replay") => {
                // The channel offers this release (or an older one): up to date.
                remove_package(&self.incoming)?;
                let installed = installed_release(root);
                atomic_json(
                    &download,
                    &json!({"schema": 1, "state": "Checked", "up_to_date": true,
                            "release_version": installed}),
                    false,
                )?;
                return Ok(json!({"state": "UpToDate", "release_version": installed}));
            }
            Err(error) => return Err(error),
        };
        let Some(Value::Object(payloads)) = manifest.get("payloads") else {
            return Err(malformed());
        };
        let mut items = Vec::new();
        let mut total: i64 = 0;
        for key in key_order(&text, "payloads") {
            let item = payloads.get(&key).ok_or_else(malformed)?;
            let bytes = py::exact_int(item.get("bytes")).ok_or_else(malformed)?;
            total += bytes;
            items.push((
                key,
                bytes,
                item.get("sha256").cloned().unwrap_or(Value::Null),
            ));
        }
        let (Some(release), Some(sequence)) =
            (manifest.get("release_version"), manifest.get("sequence"))
        else {
            return Err(malformed());
        };
        self.summary = json!({"schema": 1, "state": "Checked", "release_version": release,
                              "sequence": sequence, "download_bytes": total,
                              "notes": manifest.get("notes").cloned().unwrap_or(json!("")),
                              "reborn_version": manifest.get("reborn_version").cloned().unwrap_or(json!(""))});
        if self.check_only {
            remove_package(&self.incoming)?;
            atomic_json(&download, &self.summary, false)?;
            return Ok(Value::Object(manifest));
        }
        let raw = payloads
            .get("rootfs")
            .and_then(|r| py::exact_int(r.get("raw_bytes")))
            .ok_or_else(malformed)?;
        // Until the package is committed below, SIGTERM (Reborn's Cancel) stops the download and
        // leaves the release offered as before.
        Terminate::set(cancel as *const () as libc::sighandler_t);
        // Root payload plus its backup during installation; refuse early, not in recovery.
        cleaned += make_room(root, file, total as i128 + raw.div_euclid(4) as i128)?;
        if CANCELLED.load(Ordering::SeqCst) {
            return Err(Raise::Net(Net::Cancelled));
        }
        // Music and the screen come first: download and check yield the CPU.
        // SAFETY: nice(2) only lowers this process's priority.
        unsafe { libc::nice(10) };
        let mut live = Progress::new(root, release.clone(), total);
        live.finish("download");
        self.live = Some(live);
        let mut count: i64 = 0;
        for (key, bytes, sha) in &items {
            let name = payload_file(key).ok_or_else(malformed)?;
            let mut stream = source.get(name, true)?;
            let live = self.live.as_mut().expect("progress");
            let (got, digest) = copy_stream(
                &mut stream,
                &self.incoming.join(name),
                *bytes,
                Some(*bytes),
                900.0,
                true,
                |done| {
                    live.done = count + done;
                    live.publish("download", false);
                },
            )?;
            drop(stream);
            if sha.as_str() != Some(digest.as_str()) {
                return Err(Raise::value("download_hash_mismatch"));
            }
            count += got;
        }
        let live = self.live.as_mut().expect("progress");
        live.done = count;
        // The queue below decodes and checks every payload once (OTA-48: the separate check before it
        // decoded the same files a second time).
        let _ = fs::remove_file(root.path(CHECK_PROGRESS));
        live.finish("verify");
        if CANCELLED.load(Ordering::SeqCst) {
            return Err(Raise::Net(Net::Cancelled));
        }
        if storage::data_volume(root)?.generation != volume.generation {
            return Err(Raise::value("update_data_generation_changed"));
        }
        // The commit point: from here the package is queued, never half-cancelled.
        Terminate::set(libc::SIG_IGN);
        if CANCELLED.load(Ordering::SeqCst) {
            return Err(Raise::Net(Net::Cancelled));
        }
        let pending = self.directory.join("pending");
        remove_package(&pending)?;
        fs::rename(&self.incoming, &pending).map_err(|e| {
            Raise::Os(format!(
                "{} -> {}",
                py::os_error(&e, Some(&self.incoming.to_string_lossy())).0,
                py::repr(&pending.to_string_lossy())
            ))
        })?;
        let parent = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(&self.directory)
            .map_err(|e| Raise::os(e, &self.directory))?;
        parent
            .sync_all()
            .map_err(|e| Raise::Os(py::os_error(&e, None).0))?;
        drop(parent);
        let (result, _) = core(root, "queue", None, 300.0)?;
        if let Some(live) = self.live.as_mut() {
            live.finish("done");
        }
        atomic_json(
            &download,
            &json!({"schema": 1, "state": "Verified", "bytes": count,
                    "release_version": release,
                    "notes": manifest.get("notes").cloned().unwrap_or(json!("")),
                    "backup_budget": "recovery captures the compressed previous root and BOOTIMG before any write",
                    "reserve_bytes": RESERVE as i64, "cleaned_bytes": cleaned as i64}),
            true,
        )?;
        Ok(Value::Object(result))
    }
}

/// `update.stage`: check the signed manifest, and unless `check_only` download the payloads and queue
/// the package with the core.
pub fn stage(
    root: &Root,
    source: Option<&str>,
    url: Option<&str>,
    check_only: bool,
) -> Result<Value> {
    let source = source.filter(|s| !s.is_empty());
    let url = url.filter(|u| !u.is_empty());
    if source.is_some() == url.is_some() {
        return Err(Raise::value(
            "exactly_one_local_package_or_HTTPS_manifest_required",
        ));
    }
    let volume = storage::data_volume(root)?;
    if root
        .path("/data/system/platform/maintenance-pending")
        .exists()
    {
        return Err(Raise::value("maintenance_in_progress"));
    }
    let directory = root.path("/data/updates");
    private_directory(&directory)?;
    let lock = directory.join(".stage.lock");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&lock)
        .map_err(|e| Raise::os(e, &lock))?;
    let token = token(root)?;
    let terminate = Terminate::save();
    CANCELLED.store(false, Ordering::SeqCst);
    let mut staging = Staging {
        root,
        incoming: directory.join(&token),
        directory,
        token,
        check_only,
        live: None,
        summary: Value::Null,
        previous: Value::Null,
    };
    let result = staging.run(&file, url, source, &volume);
    let download = staging.directory.join("download.json");
    let outcome = match result {
        Ok(value) => Ok(value),
        Err(error) if error.cancelled() => {
            let _ = remove_package(&staging.incoming);
            atomic_json(&download, &staging.summary, true)?;
            if let Some(live) = staging.live.as_mut() {
                live.finish("failed");
            }
            Err(error)
        }
        Err(Raise::Fallback) => Err(Raise::Fallback),
        Err(error) => {
            let text = error.text();
            let mut record = json!({"schema": 1, "state": "Failed", "failure": text});
            if check_only && is_one_of(staging.previous.get("state"), &["Checked"]) {
                // A check that couldn't reach the server keeps the release the page already offered
                // (Update Now stays); the error goes to the caller.
                let mut kept = staging.previous.as_object().cloned().unwrap_or_default();
                kept.insert("last_check_failure".into(), json!(prefix(&text, 120)));
                record = Value::Object(kept);
            }
            if let Raise::NoSpace(needed) = error {
                record["needed_bytes"] = json!(needed as i64);
                let _ = remove_package(&staging.incoming);
            }
            atomic_json(&download, &record, true)?;
            if let Some(live) = staging.live.as_mut() {
                live.finish("failed");
            }
            Err(error)
        }
    };
    drop(terminate);
    drop(file);
    outcome
}

/// `update.restart_mode`: restart into the installer and back (proven on 43 and 45). Owner override:
/// /data/updates/restart.json {"mode": "poweroff"} turns off instead, and Power starts the installer and
/// the new system.
fn restart_mode(root: &Root) -> &'static str {
    if root.json_object("/data/updates/restart.json").get("mode") == Some(&json!("poweroff")) {
        "poweroff"
    } else {
        "reboot"
    }
}

/// `update.battery_gate`: charger connected, or at least MINIMUM_PERCENT from this boot's estimate.
fn battery_gate(root: &Root) -> Result<()> {
    let power = root.json_object("/run/y2/power.json");
    let mut battery = match power.get("battery") {
        Some(Value::Object(map)) => map.clone(),
        // A battery record that is not an object is a traceback in Python (CONTRACT.md, accepted
        // differences: the power daemon never writes one); read as no battery estimate.
        _ => Map::new(),
    };
    let owner = power.get("boot_id").cloned().unwrap_or(Value::Null);
    if !owner.is_null() && owner != boot_id(root) {
        battery = Map::new();
    }
    if is_true(battery.get("charging")) || is_one_of(battery.get("status"), &["Charging", "Full"]) {
        return Ok(());
    }
    match py::exact_int(battery.get("percent")) {
        Some(percent) if percent >= MINIMUM_PERCENT => Ok(()),
        _ => Err(Raise::value("update_needs_charger_or_half_battery")),
    }
}

/// `power.rpc`: one request to the power daemon's socket.
fn rpc(root: &Root, action: &str) -> Result<Value> {
    use std::os::unix::net::UnixStream;
    let request = Ordered::new()
        .with("action", action)
        .with("reason", "update");
    let data = y2p_core::json::dumps(&request, y2p_core::json::Separators::Spaced)
        .map_err(|e| Raise::Value(e.to_string()))?
        + "\n";
    if data.len() > 2048 {
        return Err(Raise::value("request_too_large"));
    }
    let os = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
            Raise::Os("timed out".into())
        }
        _ => Raise::Os(py::os_error(&e, None).0),
    };
    let mut client = UnixStream::connect(root.path("/run/y2/power.sock")).map_err(os)?;
    client
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(os)?;
    client
        .set_write_timeout(Some(Duration::from_secs(1)))
        .map_err(os)?;
    client.write_all(data.as_bytes()).map_err(os)?;
    let mut answer = Vec::new();
    while !answer.contains(&b'\n') {
        let mut part = [0u8; 4096];
        let n = client.read(&mut part).map_err(os)?;
        if n == 0 || answer.len() + n > 8192 {
            return Err(Raise::value("invalid_power_response"));
        }
        answer.extend_from_slice(&part[..n]);
    }
    let text = String::from_utf8_lossy(&answer).into_owned();
    let result: Value = serde_json::from_str(text.trim_end_matches(['\n', '\r', ' ', '\t']))
        .map_err(|e| Raise::Value(json_error(&text, &e)))?;
    let Value::Object(result) = result else {
        return Err(Raise::value("invalid_power_response"));
    };
    if !result.get("ok").is_some_and(truthy) {
        return Err(Raise::Value(
            result
                .get("error")
                .map_or_else(|| "power_request_failed".into(), str_of),
        ));
    }
    result
        .get("result")
        .cloned()
        .ok_or_else(|| Raise::value("invalid_power_response"))
}

/// `workload.workload("Maintenance")`: the shared activity lease (sleep waits for it) and the
/// optional QoS hint, renewed every second while `function` runs.
fn maintenance<T>(root: &Root, function: impl FnOnce() -> Result<T>) -> Result<T> {
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    let lease_path = runtime.join("activity.lock");
    let _lease = match y2p_core::lock::try_shared(&lease_path) {
        Ok(Some(lease)) => lease,
        Ok(None) => return Err(Raise::value("sleep_operation_in_progress")),
        Err(e) => return Err(Raise::os(e, &lease_path)),
    };
    let payload = b"Maintenance 3000\n";
    let stop = std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let mut worker = None;
    if let Ok(mut fd) = OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(root.path("/dev/y2-workload"))
    {
        // Acquire before work starts; renew on an independent worker.
        if fd.write(payload).ok() == Some(payload.len()) {
            let stop = stop.clone();
            worker = Some(std::thread::spawn(move || loop {
                if *stop.0.lock().expect("lock") {
                    return;
                }
                if fd.write(payload).ok() != Some(payload.len()) {
                    return;
                }
                let guard = stop.0.lock().expect("lock");
                let (guard, _) = stop
                    .1
                    .wait_timeout_while(guard, Duration::from_secs(1), |stopped| !*stopped)
                    .expect("wait");
                if *guard {
                    return;
                }
            }));
        }
    }
    let result = function();
    *stop.0.lock().expect("lock") = true;
    stop.1.notify_all();
    if let Some(worker) = worker {
        let _ = worker.join();
    }
    result
}

/// `update.apply`: install (or restore) in the Y2 recovery image: LK starts RECOVERY from the MISC
/// request on the restart (or, with the owner's override, on the next Power press).
fn apply(root: &Root, rollback: bool) -> Result<Value> {
    maintenance(root, || {
        if rollback {
            // DEFECT (CONTRACT.md): the core is asked for the rollback before the battery gate, so a
            // battery refusal leaves the journal RollbackPending.
            core(root, "rollback", None, 15.0)?;
        } else if !state_is(&status(root), &["Queued"]) {
            return Err(Raise::value("no_verified_queued_update"));
        }
        battery_gate(root)?;
        core(root, "prepare", None, 120.0)?;
        rpc(root, restart_mode(root))
    })
}

/// `update.probe`: owner test: enter the recovery image, which records that it ran and shows its
/// screen, then returns like an install (restart by default).
fn probe(root: &Root) -> Result<Value> {
    maintenance(root, || {
        core(root, "probe", None, 60.0)?;
        rpc(root, restart_mode(root))
    })
}

/// `update.cancel`: cancel a queued update and free its package right away (OTA-46).
fn cancel_update(root: &Root) -> Result<Value> {
    let (result, _) = core(root, "cancel", None, 15.0)?;
    let _ = remove_package(&root.path("/data/updates/pending"));
    Ok(Value::Object(result))
}

/// The manifest URL `stage` would get, or a fallback when the channel names something that is not a
/// string (Python fails on it later, after its records changed).
fn channel_url(root: &Root) -> Result<Option<String>> {
    match channel(root).get("manifest_url") {
        Some(Value::String(url)) => Ok(Some(url.clone())),
        Some(value) if truthy(value) => Err(Raise::Fallback),
        _ => Ok(None),
    }
}

/// `update.automatic_check`: daily check on Wi-Fi: only reports that an update exists, never downloads
/// a payload or installs. Called by the maintenance loop; cheap when not due.
fn automatic_check(root: &Root) -> Result<Value> {
    let url = channel_url(root)?;
    let now = super::time_wall();
    let directory = root.path("/data/updates");
    private_directory(&directory)?;
    let mut last = root.json_object("/data/updates/last-check.json");
    // `time` counts only from a check that reached the server: ≤ 8.14 kept it on a failed one too,
    // which held off every automatic check for a day (ef22a23d).
    if last.get("result") != Some(&json!("checked")) {
        last.remove("time");
    }
    if let Some(stamp) = int_or_float(last.get("time")) {
        if (0.0..CHECK_INTERVAL).contains(&(now - stamp)) {
            return Ok(Value::Null);
        }
    }
    // Any attempt is recorded, so an unready clock retries hourly, not every tick.
    let mut attempt = last.clone();
    attempt.insert("schema".into(), json!(1));
    attempt.insert("attempt".into(), json!(now));
    atomic_json(&directory.join("last-check.json"), &attempt, false)?;
    if !state_is(&status(root), &TERMINAL) || url.is_none() {
        return Ok(Value::Null);
    }
    if root.json_object("/run/y2/network.json").get("state") != Some(&json!("Online")) {
        return Ok(Value::Null);
    }
    if !super::time_status(root)
        .get("tls_ready")
        .is_some_and(truthy)
    {
        return Ok(Value::Null);
    }
    // Only a check that reached the server starts the next day; a failed one is tried again with the
    // hourly attempt (it waited a whole day).
    let mut record = json!({"schema": 1, "attempt": now});
    if let Some(time) = last.get("time").filter(|t| t.is_number()) {
        record["time"] = time.clone();
    }
    // Never an interrupted download's leftover: a failed check would write it back.
    let previous = download_record(root);
    match stage(root, None, url.as_deref(), true) {
        Ok(manifest) => {
            record["time"] = json!(now);
            record["result"] = json!("checked");
            record["release_version"] = manifest
                .get("release_version")
                .cloned()
                .unwrap_or(Value::Null);
        }
        Err(Raise::Fallback) => return Err(Raise::Fallback),
        Err(error) => {
            record["result"] = json!("failed");
            record["failure"] = json!(prefix(&error.text(), 120));
            // A failed background check is not the owner's failed update: keep what the Software
            // Update page showed before.
            let path = root.path("/data/updates/download.json");
            if previous.is_null() {
                let _ = crate::reply::unlink_missing_ok(&path);
            } else {
                let _ = atomic_json(&path, &previous, true);
            }
        }
    }
    atomic_json(&directory.join("last-check.json"), &record, true)?;
    Ok(record)
}

/// `update.application_healthy`: Reborn's own verdict (`healthy`, Beta 8): drawing, database working,
/// no stuck worker. A library scan that could not read some songs is not a reason to roll an update
/// back (V1 code review B3). Without the field (a Reborn before Beta 8), the drawing and uptime checks
/// alone.
pub fn application_healthy(app: &Value) -> bool {
    let Value::Object(app) = app else {
        return false;
    };
    if let Some(healthy) = app.get("healthy") {
        return is_true(Some(healthy));
    }
    matches!(app.get("graphics"), Some(Value::Object(g)) if is_true(g.get("available")))
        && py::exact_int(app.get("uptime_seconds")).is_some_and(|u| u >= 10)
}

/// `[0-9]+(\s+[0-9]+)*` on the stripped text.
fn numbers_only(text: &str) -> bool {
    let space = |c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c);
    !text.is_empty()
        && text
            .split(space)
            .filter(|t| !t.is_empty())
            .all(|t| t.bytes().all(|b| b.is_ascii_digit()))
        && !text.starts_with(space)
}

/// `update.restore_health_debug`: undo init's health-boot console level and stall panic (F23); best
/// effort. The panic timeout stays at 10 s: only the health boot's RCU stall panic and console level go
/// back to the saved values.
fn restore_health_debug(root: &Root) {
    // strict=False: a raw tab from /proc/sys/kernel/printk (8.22 test build).
    let Some(Value::Object(saved)) =
        super::python::loads_lenient(&root.read("/run/y2/health-debug.json").unwrap_or_default())
    else {
        return;
    };
    for name in ["printk", "panic", "panic_on_rcu_stall"] {
        let value = if name == "panic" {
            Some(RUNNING_PANIC_SECONDS.to_owned())
        } else {
            saved.get(name).and_then(Value::as_str).map(str::to_owned)
        };
        if let Some(value) = value {
            let stripped = y2p_core::root::python_strip(&value);
            if numbers_only(stripped) {
                let _ = fs::write(
                    root.path(&format!("/proc/sys/kernel/{name}")),
                    format!("{stripped}\n"),
                );
            }
        }
    }
    let _ = fs::remove_file(root.path("/run/y2/health-debug.json"));
}

/// Python `a == b` for an int (or None) read from /proc and a value from a record.
fn same_number(read: Option<i64>, recorded: Option<&Value>) -> bool {
    match (read, recorded) {
        (None, None | Some(Value::Null)) => true,
        (Some(n), Some(_)) => py::eq_int(recorded, n),
        _ => false,
    }
}

/// `update.health_ack`: the first boot of an installed update confirms itself once Reborn draws and
/// answers, or asks for the rollback after 180 s.
fn health_ack(root: &Root) -> Result<Value> {
    let pending = status(root);
    if !state_is(&pending, &["PendingHealth"]) {
        return Ok(Value::Object(pending));
    }
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    let mut progress = root.json_object("/run/y2/update-health.json");
    let boot = boot_id(root);
    if progress.get("boot_id").cloned().unwrap_or(Value::Null) != boot {
        progress = object(Some(json!({"boot_id": boot,
                                      "deadline_monotonic_s": clock::monotonic() + 180.0})));
        atomic_json(&runtime.join("update-health.json"), &progress, false)?;
    }
    // A deadline that is not a number is a TypeError in Python; only possible for a record this call
    // did not write, so nothing changed yet.
    let deadline = py::number(progress.get("deadline_monotonic_s")).ok_or(Raise::Fallback)?;
    if clock::monotonic() >= deadline {
        // Ask LK for the recovery image now, so even a later hang before the rescue restores on the
        // next power-on. Never an automatic power-off.
        core(root, "rollback", None, 15.0)?;
        return Ok(Value::Object(core(root, "prepare", None, 120.0)?.0));
    }
    // A successful frame alone is insufficient: app must still answer, core mounts/devices must be
    // usable, and the same boot/process must own readiness.
    let ready = root.json_object("/run/y2/application-ready.json");
    if ready.get("boot_id").cloned().unwrap_or(Value::Null) != boot_id(root)
        || !is_true(ready.get("first_frame"))
    {
        return Ok(json!({"state": "PendingHealth", "reason": "application_first_frame_missing"}));
    }
    let Some(pid) = py::exact_int(ready.get("pid")).filter(|p| *p > 0) else {
        return Ok(json!({"state": "PendingHealth", "reason": "application_pid_missing"}));
    };
    if pid > 4_194_304 {
        return Err(Raise::value("invalid pid"));
    }
    let stat = root.read(&format!("/proc/{pid}/stat")).unwrap_or_default();
    let tail = stat.rsplit_once(") ").map_or(stat.as_str(), |(_, t)| t);
    let fields: Vec<&str> = tail.split_whitespace().collect();
    let start_ticks = fields.get(19).and_then(|f| y2p_core::root::python_int(f));
    if !same_number(start_ticks, ready.get("start_ticks")) {
        return Ok(json!({"state": "PendingHealth", "reason": "application_generation_changed"}));
    }
    if !core_health_ok(root)? {
        return Ok(json!({"state": "PendingHealth", "reason": "core_health_not_OK"}));
    }
    let answer = command::run(root, &["/usr/bin/rebornctl", "status", "--json"], 2.0);
    let app = if answer.ok {
        serde_json::from_str(answer.output.as_deref().unwrap_or("")).unwrap_or(json!({}))
    } else {
        json!({})
    };
    if !application_healthy(&app) {
        return Ok(json!({"state": "PendingHealth", "reason": "application_not_ready"}));
    }
    super::super::boot::mark(root, "application_ready")?;
    let (result, _) = core(root, "ack", None, 15.0)?;
    restore_health_debug(root);
    // The core keeps the installed package as current/ (the next install's rollback copy); anything
    // left in pending/ is no longer needed.
    let _ = remove_package(&root.path("/data/updates/pending"));
    Ok(Value::Object(result))
}

/// `health.check`: root, data, filesystem_errors, display, render_node, audio and input all `OK`.
/// It is the whole `observe.snapshot` (port group observation), not ported yet: the Python platform
/// does this health-ack. Nothing it would not write the same way has changed by then (it reads back
/// this boot's update-health.json unchanged).
fn core_health_ok(_root: &Root) -> Result<bool> {
    Err(Raise::Fallback)
}

/// The routed form: `update <action> [--url URL] [--package DIR]`.
pub fn run(root: &Root, action: &str, url: Option<&str>, package: Option<&str>) -> Outcome {
    let result = (|| -> Result<Value> {
        match action {
            "check" | "stage" => {
                let url = match url.filter(|u| !u.is_empty()) {
                    Some(url) => Some(url.to_owned()),
                    None if package.is_none_or(str::is_empty) => channel_url(root)?,
                    None => None,
                };
                stage(root, package, url.as_deref(), action == "check")
            }
            // DEFECT (CONTRACT.md): `update auto-check` prints `null` when nothing was due.
            "auto-check" => automatic_check(root),
            "probe-recovery" => probe(root),
            "recovery-status" => Ok(Value::Object(core(root, "misc", None, 15.0)?.0)),
            "apply" | "rollback" => apply(root, action == "rollback"),
            "cancel" => cancel_update(root),
            "health-ack" => health_ack(root),
            _ => Ok(Value::Object(status(root))),
        }
    })();
    match result {
        Ok(value) => Reply::sorted(&value, 0),
        Err(Raise::Fallback) => Err(Failure::Fallback),
        Err(error) => Reply::unsorted(
            &Ordered::new()
                .with("state", "Failed")
                .with("failure", error.text()),
            1,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_debug_values_are_numbers_only() {
        assert!(numbers_only("4\t4\t1\t7"));
        assert!(numbers_only("10"));
        assert!(!numbers_only(""));
        assert!(!numbers_only("4 x"));
        assert!(!numbers_only("-1"));
    }

    #[test]
    fn reborns_verdict_wins() {
        assert!(application_healthy(&json!({"healthy": true})));
        assert!(!application_healthy(
            &json!({"healthy": 1, "graphics": {"available": true}})
        ));
        assert!(application_healthy(
            &json!({"graphics": {"available": true}, "uptime_seconds": 10})
        ));
        assert!(!application_healthy(
            &json!({"graphics": {"available": true}, "uptime_seconds": 9.5})
        ));
        assert!(!application_healthy(&json!([])));
    }

    #[test]
    fn pathlib_spells_paths() {
        assert_eq!(pathlib(Path::new("/a//b/./"), "x"), PathBuf::from("/a/b/x"));
        assert_eq!(pathlib(Path::new("./pkg"), "x"), PathBuf::from("pkg/x"));
    }

    #[test]
    fn json_errors_read_as_python() {
        let error = serde_json::from_str::<Value>("").unwrap_err();
        assert_eq!(
            json_error("", &error),
            "Expecting value: line 1 column 1 (char 0)"
        );
        let error = serde_json::from_str::<Value>("{} x").unwrap_err();
        assert_eq!(
            json_error("{} x", &error),
            "Extra data: line 1 column 4 (char 3)"
        );
    }
}
