//! `radiofw.py`: radio firmware from the owner's own copy (RELEASE-49).
//!
//! Y2Linux images carry no MediaTek radio firmware: it may not be redistributed. Each player keeps the
//! reviewed files on Y2DATA (`/data/firmware/mt6582`, which `/lib/firmware/mediatek/mt6582` links
//! to). They come from the owner's copy: on a player that ran an older image, the recovery copies them
//! from the old root before it writes the new one (recovery.sh); otherwise the owner copies
//! `system.img` from the Innioasis Y2 firmware package into the `Firmware` folder over USB (on Internal
//! Storage, or the Firmware drive on Y2DATA while there is no Internal Storage); this module reads the
//! Android sparse ext4 image itself, checks the five files against the reviewed SHA-256 values,
//! derives the two board defaults like tools/connectivity/provision.py, installs them, deletes the copy
//! and starts the radios. Only names, sizes and hashes are in this file; never firmware bytes.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    image::{self, Ext4, Image},
    python::{mountinfo, py_eq, realpath, Mount},
};
use crate::reply::{atomic_json as publish, Failure, Outcome, Reply};
use serde_json::{json, Map, Value};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use y2p_core::{clock, command, json::Ordered, py, record, service_digest::sha256_hex, Root};

/// docs/knowledge/evidence/m5-firmware-inventory.json (stock v3.2.0 FM, /etc/firmware).
pub const FILES: [(&str, u64, &str); 5] = [
    (
        "WIFI_RAM_CODE_MT6582",
        160368,
        "4def37522e2bfd2d4b78e01bf9f567a37e3f67ab3d7b6646ffb317f5b2b49b82",
    ),
    (
        "WMT_SOC.cfg",
        80,
        "cea0a94b8a88e9c24e6835446ebc3106d7b100c3709ea98e8bb7368689c8a2b5",
    ),
    (
        "mt6572_82_patch_e1_0_hdr.bin",
        33120,
        "fa3be4a92540296de512d64f56272746be10bdfa2db571c8ceada2d40cd616f0",
    ),
    (
        "mt6572_82_patch_e1_1_hdr.bin",
        17476,
        "96eb25489305bd472805db1da33bcc0acd37b5bb21cceda98d7e88ba1179ca37",
    ),
    (
        "modem_1_2g_n.img",
        2424376,
        "5059775975cbf6ab74c43978ca8f65d9a274b83585f456134e39b09f6dc7a4f1",
    ),
];
/// provision.py: generic board defaults derived from the stock reader library.
pub const DEFAULTS: [(&str, u64, &str); 2] = [
    (
        "WIFI.defaults",
        512,
        "eb5e87224e3a493e825179f547fc13924f564a42730ea821b4359b5b36f4defe",
    ),
    (
        "BT.defaults",
        64,
        "f224ff4abfb45a612ef03249e87e4ecfa44b7c1f9f19ee3cc78ae354764d9472",
    ),
];
pub const CUSTOM: (&str, u64, &str) = (
    "/lib/libcustom_nvram.so",
    51224,
    "c6460e9649f539b56449dc19f8d20771402f539be9e73340a74a9ccf45287397",
);

/// The reviewed tables (a test swaps them for its fixture files', as tests/test_radio_firmware.py).
#[derive(Clone)]
pub struct Reviewed {
    pub files: Vec<(String, u64, String)>,
    pub defaults: Vec<(String, u64, String)>,
    pub custom: (String, u64, String),
}

impl Reviewed {
    pub fn stock() -> Self {
        let own = |t: &[(&str, u64, &str)]| {
            t.iter()
                .map(|(n, s, h)| (n.to_string(), *s, h.to_string()))
                .collect()
        };
        Self {
            files: own(&FILES),
            defaults: own(&DEFAULTS),
            custom: (CUSTOM.0.into(), CUSTOM.1, CUSTOM.2.into()),
        }
    }

    fn all(&self) -> impl Iterator<Item = &(String, u64, String)> {
        self.files.iter().chain(self.defaults.iter())
    }
}

const TARGET: &str = "/data/firmware/mt6582";
/// While the firmware is missing the computer sees this folder as the MTP storage "Firmware"
/// (usb.media_views); nothing else of Y2DATA is exported.
pub(crate) const INBOX: &str = "/data/firmware/inbox";
pub const STATE: &str = "/run/y2/radio-firmware.json";
const FOLDER: &str = "Firmware";
const SOURCE: &str = "system.img";
/// With Internal Storage the copy goes there (#39: Y2DATA has room for the zip's 518 MB image, never
/// for an unpacked 860 MB one).
const INTERNAL_INBOX: &str = "/media/internal/music/Firmware";
const SD_INBOX: &str = "/media/sd/Firmware";
const ENOSPC: i64 = 28;
const README: &str =
    "Wi-Fi and Bluetooth need the radio firmware from the Innioasis Y2 firmware package.\n\
Copy (do not move) the file system.img from that package into this folder (about\n\
520 MB). The player checks it, keeps the five small radio files and deletes the copy.\n\
A larger system.img (about 860 MB) is an unpacked copy: if there is not enough space\n\
here, put it into a folder named Firmware on the SD card instead.\n";

/// What radiofw raises (its callers catch OSError and ValueError alike, by text).
#[derive(Debug)]
pub enum Error {
    Value(String),
    Os(String),
}

impl Error {
    pub fn text(&self) -> String {
        match self {
            Error::Value(t) | Error::Os(t) => t.clone(),
        }
    }
}

impl From<image::Error> for Error {
    fn from(error: image::Error) -> Error {
        match error {
            image::Error::Value(t) => Error::Value(t),
            image::Error::Os(t) => Error::Os(t),
            // FileNotFoundError(path): only from a lookup, which every caller turns into a ValueError.
            image::Error::NotFound => Error::Value("not_the_y2_firmware_package".into()),
        }
    }
}

impl From<Failure> for Error {
    fn from(failure: Failure) -> Error {
        match failure {
            Failure::Value(t) => Error::Value(t),
            other => Error::Os(other.message()),
        }
    }
}

fn os_path(error: std::io::Error, path: &Path) -> Error {
    Error::Os(py::os_error(&error, Some(&path.to_string_lossy())).0)
}

type Result<T> = std::result::Result<T, Error>;
type Files = Vec<(String, Vec<u8>)>;

// ---- the five files ------------------------------------------------------------------------------

/// `provision.defaults`: the generic Wi-Fi/BT board records of the stock reader library, with the
/// default addresses cleared.
fn defaults(custom: &[u8]) -> Result<Files> {
    let slice = |a: usize, b: usize| custom.get(a..b.min(custom.len())).unwrap_or(&[]).to_vec();
    let mut wifi = slice(0x5a8e, 0x5c8e);
    let mut bt = slice(0x3a18, 0x3a58);
    if wifi.len() < 4 {
        return Err(Error::Value(
            "unpack_from requires a buffer of at least 4 bytes".into(),
        ));
    }
    let header = (
        u16::from_le_bytes([wifi[0], wifi[1]]),
        u16::from_le_bytes([wifi[2], wifi[3]]),
    );
    if !custom.starts_with(b"\x7fELF\x01\x01\x01")
        || header != (0x104, 0)
        || wifi.get(196..198) != Some(&b"\x01\x00"[..])
    {
        return Err(Error::Value("stock_reader_library_layout".into()));
    }
    for byte in wifi.iter_mut().take(10).skip(4) {
        *byte = 0;
    }
    for byte in bt.iter_mut().take(6) {
        *byte = 0;
    }
    Ok(vec![
        ("WIFI.defaults".into(), wifi),
        ("BT.defaults".into(), bt),
    ])
}

fn get<'a>(files: &'a Files, name: &str) -> Option<&'a Vec<u8>> {
    files.iter().find(|(n, _)| n == name).map(|(_, d)| d)
}

/// `radiofw.checked`: every reviewed file present, with the reviewed size and SHA-256.
fn checked(reviewed: &Reviewed, files: Files) -> Result<Files> {
    for (name, size, sha) in reviewed.all() {
        match get(&files, name) {
            Some(data) if data.len() as u64 == *size && sha256_hex(data) == *sha => {}
            _ => return Err(Error::Value(format!("firmware_mismatch:{name}"))),
        }
    }
    // The two Bluetooth patches in their download order (provision.py).
    let mut order: Vec<u8> = Vec::new();
    for (name, _, _) in &reviewed.files {
        if name.ends_with("_hdr.bin") {
            let data = get(&files, name).expect("checked");
            let byte = *data
                .get(24)
                .ok_or_else(|| Error::Value("index out of range".into()))?;
            order.push(byte & 15);
        }
    }
    order.sort();
    if order != [1, 2] {
        return Err(Error::Value("firmware_patch_order".into()));
    }
    Ok(files)
}

/// `radiofw.from_system_image`: the seven files from the stock system.img (sparse or raw ext4).
fn from_system_image(reviewed: &Reviewed, path: &Path) -> Result<Files> {
    let mut fs = Ext4::new(Image::open(path)?)?;
    let mut files: Files = Vec::new();
    for (name, size, _) in &reviewed.files {
        files.push((
            name.clone(),
            fs.read(&format!("/etc/firmware/{name}"), *size)?,
        ));
    }
    let custom = fs.read(&reviewed.custom.0, reviewed.custom.1)?;
    if custom.len() as u64 != reviewed.custom.1 || sha256_hex(&custom) != reviewed.custom.2 {
        return Err(Error::Value("firmware_mismatch:libcustom_nvram.so".into()));
    }
    files.extend(defaults(&custom)?);
    checked(reviewed, files)
}

/// `radiofw.from_directory`: the seven files from a directory (a root's /lib/firmware/mediatek/mt6582).
fn from_directory(reviewed: &Reviewed, path: &Path) -> Result<Files> {
    let mut files = Vec::new();
    for (name, size, _) in reviewed.all() {
        let file = path.join(name);
        let info = file.symlink_metadata().map_err(|e| os_path(e, &file))?;
        if !info.file_type().is_file() || info.len() != *size {
            return Err(Error::Value(format!("firmware_mismatch:{name}")));
        }
        files.push((
            name.clone(),
            fs::read(&file).map_err(|e| os_path(e, &file))?,
        ));
    }
    checked(reviewed, files)
}

/// `radiofw.present`: cheap check for status polling: every file there with its size.
pub fn present(root: &Root) -> bool {
    let target = root.path(TARGET);
    Reviewed::stock().all().all(|(name, size, _)| {
        target
            .join(name)
            .symlink_metadata()
            .is_ok_and(|m| m.len() == *size && m.file_type().is_file())
    })
}

/// `radiofw.installed`: every reviewed file is in place with the reviewed hash.
fn installed(root: &Root, reviewed: &Reviewed) -> bool {
    from_directory(reviewed, &root.path(TARGET)).is_ok()
}

/// `shutil.rmtree(path, ignore_errors=True)`.
fn rmtree(path: &Path) {
    if path
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_dir())
    {
        let _ = fs::remove_dir_all(path);
    }
}

/// `radiofw.install`: write the checked files to Y2DATA: a fresh directory, then one rename.
fn install(root: &Root, reviewed: &Reviewed, files: Files) -> Result<()> {
    let files = checked(reviewed, files)?;
    let target = root.path(TARGET);
    let parent = target.parent().expect("parent").to_path_buf();
    crate::reply::mkdir_parents(&parent, 0o700)?;
    let pid = std::process::id();
    let staging = parent.join(format!(".mt6582.{pid}"));
    rmtree(&staging);
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&staging)
        .map_err(|e| os_path(e, &staging))?;
    for (name, data) in &files {
        let path = staging.join(name);
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .map_err(|e| os_path(e, &path))?;
        out.write_all(data)
            .and_then(|()| out.sync_all())
            .map_err(|e| Error::Os(py::os_error(&e, None).0))?;
    }
    let meta = target.symlink_metadata();
    if meta.as_ref().is_ok_and(|m| m.file_type().is_symlink())
        || (target.exists() && !target.is_dir())
    {
        return Err(Error::Value("firmware_target_unsafe".into()));
    }
    let old = parent.join(format!(".mt6582.old.{pid}"));
    if target.exists() {
        fs::rename(&target, &old).map_err(|e| rename_error(e, &target, &old))?;
    }
    fs::rename(&staging, &target).map_err(|e| rename_error(e, &staging, &target))?;
    rmtree(&old);
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY)
        .open(&parent)
        .map_err(|e| os_path(e, &parent))?;
    directory
        .sync_all()
        .map_err(|e| Error::Os(py::os_error(&e, None).0))
}

fn rename_error(error: std::io::Error, from: &Path, to: &Path) -> Error {
    Error::Os(format!(
        "{} -> {}",
        py::os_error(&error, Some(&from.to_string_lossy())).0,
        py::repr(&to.to_string_lossy())
    ))
}

// ---- the owner's copy ----------------------------------------------------------------------------

/// `radiofw.roots`: the MTP roots the responder exports (its record), as device paths below the root.
fn roots(root: &Root) -> Vec<PathBuf> {
    let value = root.json_object("/run/y2/mtp-roots.json");
    // `for r in value.get('roots', [])`: a list's items, a dict's keys, a string's characters.
    let items: Vec<Value> = match value.get("roots") {
        Some(Value::Array(items)) => items.clone(),
        Some(Value::Object(map)) => map.keys().map(|k| json!(k)).collect(),
        Some(Value::String(text)) => text.chars().map(|c| json!(c.to_string())).collect(),
        _ => Vec::new(),
    };
    items
        .iter()
        .filter_map(Value::as_str)
        .filter(|r| r.starts_with('/'))
        .map(|r| root.path(r))
        .collect()
}

fn mounts_at(root: &Root, path: &str) -> Vec<Mount> {
    mountinfo(&root.read("/proc/self/mountinfo").unwrap_or_default())
        .into_iter()
        .filter(|m| m.path == path)
        .collect()
}

fn boot_id(root: &Root) -> Value {
    json!(root.read("/proc/sys/kernel/random/boot_id"))
}

fn get_or_null<'a>(map: &'a Map<String, Value>, key: &str) -> &'a Value {
    map.get(key).unwrap_or(&Value::Null)
}

/// `internal_media.ready`: true only while the recorded mount is still the live, exact Y2Media mount.
pub fn internal(root: &Root) -> bool {
    let claim = root.json_object("/run/y2/internal-media.json");
    let current = mounts_at(root, "/media/internal");
    claim.get("state") == Some(&json!("Ready"))
        && current.len() == 1
        && *get_or_null(&claim, "boot_id") == boot_id(root)
        && py::eq_int(claim.get("mount_id"), current[0].mount_id)
        && claim.get("device_id") == Some(&json!(current[0].device_id))
}

/// `Path.mkdir(mode, exist_ok=True)`.
fn mkdir_exist_ok(path: &Path, mode: u32) -> Result<()> {
    match fs::DirBuilder::new().mode(mode).create(path) {
        Err(e) if !(e.kind() == std::io::ErrorKind::AlreadyExists && path.is_dir()) => {
            Err(os_path(e, path))
        }
        _ => Ok(()),
    }
}

/// `radiofw.inbox`: where the copy goes, created with its note when needed: the Firmware folder on
/// Internal Storage, else the Firmware drive's folder on Y2DATA.
pub(crate) fn inbox(root: &Root) -> Result<PathBuf> {
    let folder = if internal(root) {
        let folder = root.path(INTERNAL_INBOX);
        mkdir_exist_ok(&folder, 0o755)?;
        folder
    } else {
        let folder = root.path(INBOX);
        crate::reply::mkdir_parents(folder.parent().expect("parent"), 0o700)?;
        mkdir_exist_ok(&folder, 0o700)?;
        folder
    };
    let note = folder.join("README.txt");
    if !note.exists() {
        fs::write(&note, README).map_err(|e| os_path(e, &note))?;
    }
    Ok(folder)
}

/// `radiofw.candidates`: where an owner's copy may be: the Firmware drive, or a Firmware folder on
/// Internal Storage or the SD card (also while no computer is connected).
pub fn candidates(root: &Root) -> Vec<PathBuf> {
    let mut places = vec![root.path(INBOX), root.path(SD_INBOX)];
    if internal(root) {
        places.insert(0, root.path(INTERNAL_INBOX));
    }
    let inbox = root.path(INBOX);
    places.extend(
        roots(root)
            .into_iter()
            .filter(|r| *r != inbox)
            .map(|r| r.join(FOLDER)),
    );
    let mut seen: Vec<PathBuf> = Vec::new();
    for folder in places {
        if !seen.contains(&folder) {
            seen.push(folder);
        }
    }
    seen.into_iter().map(|f| f.join(SOURCE)).collect()
}

// ---- the SD card's write window (media.write_window / sd_writable, holder `radiofw`) ------------

const SD_WRITE: &str = "/run/y2/sd-write.json";
const WRITE_HOLDERS: [&str; 4] = ["usb", "reborn", "report", "radiofw"];

/// `media.owned_mount`.
fn owned_mount(root: &Root, mounted: &Mount) -> bool {
    let device = root.path(&format!("/sys/dev/block/{}", mounted.device_id));
    if device.exists() {
        return realpath(&device)
            .to_string_lossy()
            .contains("/11240000.mmc/");
    }
    let claim = root.json_object("/run/y2/media-mount.json");
    // Removed cards no longer have a sysfs controller path. The retained claim permits only the exact
    // mount we created, never an arbitrary mountpoint.
    *get_or_null(&claim, "boot_id") == boot_id(root)
        && py::eq_int(claim.get("mount_id"), mounted.mount_id)
        && claim.get("device_id") == Some(&json!(mounted.device_id))
        && claim.get("device") == Some(&json!(mounted.source))
}

/// `media._sd_state`: (claim, mount) of our current SD mount.
fn sd_state(root: &Root) -> (Map<String, Value>, Option<Mount>) {
    let claim = root.json_object("/run/y2/media-mount.json");
    let old = mounts_at(root, "/media/sd");
    if old.len() == 1
        && owned_mount(root, &old[0])
        && py::eq_int(claim.get("mount_id"), old[0].mount_id)
        && *get_or_null(&claim, "boot_id") == boot_id(root)
    {
        let mount = old.into_iter().next();
        return (claim, mount);
    }
    (claim, None)
}

/// `media._writers`: the live holders of this mount's window.
fn writers(root: &Root, mount: &Mount) -> Vec<(String, f64)> {
    let value = root.json_object(SD_WRITE);
    if *get_or_null(&value, "boot_id") != boot_id(root)
        || !py::eq_int(value.get("mount_id"), mount.mount_id)
    {
        return Vec::new();
    }
    let now = clock::monotonic();
    let Some(Value::Object(holders)) = value.get("holders") else {
        return Vec::new();
    };
    holders
        .iter()
        .filter(|(k, _)| WRITE_HOLDERS.contains(&k.as_str()))
        .filter_map(|(k, v)| {
            py::number(Some(v))
                .filter(|v| *v > now)
                .map(|v| (k.clone(), v))
        })
        .collect()
}

/// `media._apply_window`: remount to match the holders; records them. Returns writable now.
fn apply_window(root: &Root, mount: &Mount, holders: &[(String, f64)]) -> Result<bool> {
    let map: Map<String, Value> = holders.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    publish(
        &root.path(SD_WRITE),
        &json!({"schema": 1, "boot_id": boot_id(root), "mount_id": mount.mount_id, "holders": map}),
        false,
    )?;
    let mut writable = mount.options.iter().any(|o| o == "rw");
    if !holders.is_empty() != writable {
        let option = if holders.is_empty() {
            "remount,ro"
        } else {
            "remount,rw"
        };
        command::run(root, &["mount", "-o", option, "/media/sd"], 10.0);
        let now = mounts_at(root, "/media/sd");
        writable = now
            .first()
            .is_some_and(|m| m.options.iter().any(|o| o == "rw"));
    }
    Ok(writable)
}

/// `media.write_window(ctx, 'radiofw', seconds, release)`: whether holders were set (`answer.get('holders')`).
fn write_window(root: &Root, seconds: i64, release: bool) -> Result<bool> {
    let runtime = root.path("/run/y2");
    crate::reply::mkdir_parents(&runtime, 0o700)?;
    let lock_path = runtime.join("media.lock");
    let deadline = clock::monotonic() + 5.0;
    let _lock = loop {
        match y2p_core::lock::try_exclusive(&lock_path) {
            Ok(Some(lock)) => break lock,
            Ok(None) => {
                // A check or a repair holds the card; a short admission passes.
                if clock::monotonic() >= deadline {
                    return Ok(false);
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(os_path(e, &lock_path)),
        }
    };
    let (claim, mount) = sd_state(root);
    let Some(mount) = mount else {
        return Ok(false);
    };
    match claim.get("read_only") {
        // Writable once its background check passes; damaged cards get no window; an unprotected card
        // needs none.
        Some(Value::String(s)) if s == "protected" => {}
        _ => return Ok(false),
    }
    let mut holders = writers(root, &mount);
    holders.retain(|(k, _)| k != "radiofw");
    if !release {
        holders.push((
            "radiofw".into(),
            clock::monotonic() + seconds.clamp(1, 600) as f64,
        ));
    }
    apply_window(root, &mount, &holders)?;
    Ok(!holders.is_empty())
}

/// `radiofw.remove`: delete the used system.img; on the SD card inside a write window (the card is
/// read-only between writes, media.py).
fn remove(root: &Root, source: &Path) {
    let on_sd = source
        .to_string_lossy()
        .starts_with(&*root.path(SD_INBOX).to_string_lossy());
    if !on_sd {
        let _ = fs::remove_file(source);
        return;
    }
    let Ok(held) = write_window(root, 30, false) else {
        return;
    };
    let _ = fs::remove_file(source);
    if held {
        let _ = write_window(root, 30, true);
    }
}

/// `radiofw.tidy`: after the install the Internal Storage Firmware folder goes away again (only its
/// note; anything the owner put there stays).
fn tidy(root: &Root) {
    let folder = root.path(INTERNAL_INBOX);
    if fs::remove_file(folder.join("README.txt")).is_ok() {
        let _ = fs::remove_dir(&folder);
    }
}

fn record_state(root: &Root, value: &Value) -> Result<()> {
    let path = root.path(STATE);
    record::atomic_json(&path, value, false, 0o644).map_err(|e| os_path(e, &path))
}

/// `radiofw.state`.
fn state(root: &Root) -> Value {
    let value = root.json_object(STATE);
    if present(root) {
        return json!({"state": "Ready"});
    }
    if super::python::is_one_of(value.get("state"), &["Installing", "Failed"]) {
        return json!({"state": get_or_null(&value, "state"), "reason": get_or_null(&value, "reason")});
    }
    let mut missing = json!({"state": "Missing", "folder": FOLDER, "file": SOURCE,
                             "location": if internal(root) { "internal" } else { "drive" }});
    // The responder refuses a copy that does not fit before writing anything (mtp-safe space()), so
    // nothing is left behind; only say why (#39).
    if py::eq_int(
        root.json_object("/run/y2/mtp-events.json").get("error"),
        ENOSPC,
    ) {
        missing["reason"] = json!("no_space");
    }
    missing
}

/// `[st_size, st_mtime_ns]` and whether the copy is still arriving (changed within 5 s).
fn seen(meta: &fs::Metadata) -> (Value, bool) {
    let mtime_ns = meta.mtime() as i128 * 1_000_000_000 + meta.mtime_nsec() as i128;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    let mtime = meta.mtime() as f64 + meta.mtime_nsec() as f64 / 1e9;
    (json!([meta.len(), mtime_ns as i64]), now - mtime < 5.0)
}

fn regular(path: &Path) -> Option<fs::Metadata> {
    path.symlink_metadata()
        .ok()
        .filter(|m| m.file_type().is_file())
}

/// `radiofw.scan`: one pass of the owner flow: make the Firmware folder visible, then take a finished
/// system.img copy from it. Returns the new state.
fn scan(root: &Root, reviewed: &Reviewed) -> Result<Value> {
    if installed(root, reviewed) {
        return leftover(root, reviewed);
    }
    let previous = root.json_object(STATE);
    let _ = inbox(root);
    for source in candidates(root) {
        let Some(info) = regular(&source) else {
            continue;
        };
        let (seen, arriving) = seen(&info);
        // A copy still arriving over USB changes size or time between passes.
        if !py_eq(get_or_null(&previous, "seen"), &seen) || arriving {
            record_state(
                root,
                &json!({"schema": 1, "state": "Missing", "seen": seen}),
            )?;
            return Ok(json!({"state": "Missing"}));
        }
        if previous.get("state") == Some(&json!("Failed")) {
            return Ok(json!({"state": "Failed", "reason": get_or_null(&previous, "reason")}));
        }
        record_state(
            root,
            &json!({"schema": 1, "state": "Installing", "seen": seen}),
        )?;
        if let Err(error) =
            from_system_image(reviewed, &source).and_then(|files| install(root, reviewed, files))
        {
            let reason: String = error.text().chars().take(80).collect();
            record_state(
                root,
                &json!({"schema": 1, "state": "Failed", "reason": reason, "seen": seen}),
            )?;
            return Ok(json!({"state": "Failed", "reason": reason}));
        }
        remove(root, &source);
        tidy(root);
        record_state(
            root,
            &json!({"schema": 1, "state": "Ready", "installed_from": "system.img"}),
        )?;
        start(root);
        return Ok(json!({"state": "Ready"}));
    }
    if previous.get("state") == Some(&json!("Failed")) {
        // The rejected copy is gone: back to asking for it.
        record_state(root, &json!({"schema": 1, "state": "Missing"}))?;
    }
    Ok(json!({"state": "Missing"}))
}

/// `radiofw.leftover`: with the firmware in place, a finished system.img copy that arrives later
/// (copied again, or after the recovery kept the files) holds 520–860 MB of the owner's storage for
/// nothing: delete it like the first one. Only the firmware package's image goes; any other file of
/// that name stays.
fn leftover(root: &Root, reviewed: &Reviewed) -> Result<Value> {
    let previous = root.json_object(STATE);
    let mut value = Ordered::new()
        .with("schema", 1)
        .with("state", "Ready")
        .with("kept", get_or_null(&previous, "kept").clone());
    for source in candidates(root) {
        let Some(info) = regular(&source) else {
            continue;
        };
        let (seen, arriving) = seen(&info);
        if py_eq(&seen, get_or_null(&previous, "kept")) {
            continue;
        }
        // A copy still arriving changes size or time between passes.
        if !py_eq(get_or_null(&previous, "leftover"), &seen) || arriving {
            value.set("leftover", seen);
            break;
        }
        if from_system_image(reviewed, &source).is_err() {
            value.set("kept", seen);
            break;
        }
        remove(root, &source);
        tidy(root);
    }
    let differs = value
        .0
        .iter()
        .any(|(k, v)| !py_eq(v, get_or_null(&previous, k)))
        || previous.keys().any(|k| value.get(k).is_none());
    if differs {
        record_state(root, &serde_json::to_value(&value).expect("value"))?;
    }
    Ok(json!({"state": "Ready"}))
}

/// `radiofw.leftover_waiting`: cheap check for the service: a copy leftover() has not judged yet.
pub fn leftover_waiting(root: &Root) -> bool {
    let kept = root
        .json_object(STATE)
        .get("kept")
        .cloned()
        .unwrap_or(Value::Null);
    candidates(root)
        .iter()
        .any(|source| regular(source).is_some_and(|info| !py_eq(&seen(&info).0, &kept)))
}

/// `radiofw.start`: (re)start the radios with the installed firmware. Stopping running radios can take
/// longer than a short command limit, which on 49 cut the combined restart off after its stop half
/// (archive 154).
fn start(root: &Root) {
    command::run(root, &["/etc/init.d/S41y2-connectivity", "stop"], 90.0);
    command::run(root, &["/etc/init.d/S41y2-connectivity", "start"], 30.0);
}

/// `y2-platform firmware <action> [path]`.
pub fn run(root: &Root, action: &str, path: Option<&str>) -> Outcome {
    let reviewed = Reviewed::stock();
    let result = (|| -> Result<Value> {
        match action {
            "status" => Ok(state(root)),
            "scan" => scan(root, &reviewed),
            _ => {
                let path = path.filter(|p| !p.is_empty()).ok_or_else(|| {
                    Error::Value("install needs a system.img or a directory".into())
                })?;
                let source = PathBuf::from(path);
                let files = if source.is_dir() {
                    from_directory(&reviewed, &source)?
                } else {
                    from_system_image(&reviewed, &source)?
                };
                install(root, &reviewed, files)?;
                start(root);
                Ok(json!({"state": "Ready"}))
            }
        }
    })();
    match result {
        // A scan whose copy could not be installed exits 1 as an install does (it exited 0 with
        // state Failed); the service ignores the code and reads the state record.
        Ok(value) => Reply::sorted(
            &value,
            u8::from(value.get("state") == Some(&json!("Failed"))),
        ),
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
    use crate::testing::Fixture;

    #[test]
    fn a_scan_whose_copy_cannot_be_installed_exits_1() {
        let fixture = Fixture::new();
        let image = format!("{INBOX}/{SOURCE}");
        fixture.put(&image, "not a system image");
        // A finished copy: older than the arrival window, and seen unchanged on the pass before.
        let file = fs::File::options()
            .write(true)
            .open(fixture.path(&image))
            .unwrap();
        file.set_modified(SystemTime::now() - std::time::Duration::from_secs(60))
            .unwrap();
        let (seen, arriving) = seen(&file.metadata().unwrap());
        assert!(!arriving);
        fixture.put_json(
            STATE,
            &json!({"schema": 1, "state": "Missing", "seen": seen}),
        );
        let reply = run(&fixture.root, "scan", None).unwrap();
        assert_eq!(reply.code, 1, "{}", reply.stdout);
        assert_eq!(fixture.json(STATE)["state"], json!("Failed"));
        // The next pass reports the same failure, still as one.
        assert_eq!(run(&fixture.root, "scan", None).unwrap().code, 1);
    }

    fn patch(sequence: u8) -> Vec<u8> {
        let mut data = vec![7u8; 64];
        data[18..20].copy_from_slice(b"PS");
        data[24] = 0x20 | sequence;
        data
    }

    fn custom_library() -> Vec<u8> {
        let mut data: Vec<u8> = (0..51224u32).map(|i| (i * 13 % 256) as u8).collect();
        data[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
        data[0x5a8e..0x5a92].copy_from_slice(&[0x04, 0x01, 0, 0]);
        data[0x5a8e + 196..0x5a8e + 198].copy_from_slice(b"\x01\x00");
        data
    }

    /// tests/test_radio_firmware.py: fixture files with the reviewed tables swapped for theirs.
    fn fixture() -> (Reviewed, Files, Vec<u8>) {
        let files: Files = vec![
            ("WIFI_RAM_CODE_MT6582".into(), vec![1u8; 5000]),
            ("WMT_SOC.cfg".into(), b"coex_wmt_ant_mode=1\n".to_vec()),
            ("mt6572_82_patch_e1_0_hdr.bin".into(), patch(2)),
            ("mt6572_82_patch_e1_1_hdr.bin".into(), patch(1)),
            ("modem_1_2g_n.img".into(), vec![9u8; 7000]),
        ];
        let custom = custom_library();
        let derived = defaults(&custom).unwrap();
        let table = |f: &Files| {
            f.iter()
                .map(|(n, d)| (n.clone(), d.len() as u64, sha256_hex(d)))
                .collect()
        };
        let reviewed = Reviewed {
            files: table(&files),
            defaults: table(&derived),
            custom: (CUSTOM.0.into(), custom.len() as u64, sha256_hex(&custom)),
        };
        (reviewed, files, custom)
    }

    #[test]
    fn a_system_image_installs_and_its_copy_goes() {
        let (reviewed, files, custom) = fixture();
        let dir = std::env::temp_dir().join(format!("y2-radiofw-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let tree = dir.join("tree");
        fs::create_dir_all(tree.join("etc/firmware")).unwrap();
        fs::create_dir_all(tree.join("lib")).unwrap();
        for (name, data) in &files {
            fs::write(tree.join("etc/firmware").join(name), data).unwrap();
        }
        fs::write(tree.join("lib/libcustom_nvram.so"), &custom).unwrap();
        let root = Root::new(dir.join("root"));
        fs::create_dir_all(root.path("/run/y2")).unwrap();
        let image = root.path(INBOX).join(SOURCE);
        fs::create_dir_all(image.parent().unwrap()).unwrap();
        let made = std::process::Command::new("mke2fs")
            .args(["-q", "-F", "-b", "4096", "-d"])
            .arg(&tree)
            .arg(&image)
            .arg("8M")
            .output()
            .is_ok_and(|o| o.status.success());
        if !made {
            let _ = fs::remove_dir_all(&dir);
            return; // no host mke2fs
        }
        let old = SystemTime::now() - Duration::from_secs(60);
        fs::File::options()
            .write(true)
            .open(&image)
            .unwrap()
            .set_modified(old)
            .unwrap();
        // The first pass only notes the copy; the second takes it.
        assert_eq!(scan(&root, &reviewed).unwrap(), json!({"state": "Missing"}));
        assert_eq!(scan(&root, &reviewed).unwrap(), json!({"state": "Ready"}));
        assert!(!image.exists());
        assert!(installed(&root, &reviewed));
        let wifi = fs::read(root.path(TARGET).join("WIFI.defaults")).unwrap();
        assert_eq!(wifi.len(), 512);
        assert_eq!(&wifi[4..10], &[0; 6]);
        assert_eq!(
            root.json(STATE).unwrap(),
            json!({"schema": 1, "state": "Ready", "installed_from": "system.img"})
        );
        // A second copy is judged and deleted by leftover() (the stock tables reject it: kept).
        assert_eq!(scan(&root, &reviewed).unwrap(), json!({"state": "Ready"}));
        // from_directory reads the installed set back; checked() refuses a wrong patch order.
        let back = from_directory(&reviewed, &root.path(TARGET)).unwrap();
        let mut swapped = back.clone();
        for item in &mut swapped {
            if item.0.ends_with("_hdr.bin") {
                item.1[24] = 0x21;
            }
        }
        let mut reordered = reviewed.clone();
        for (name, size, sha) in &mut reordered.files {
            if let Some(data) = get(&swapped, name) {
                *size = data.len() as u64;
                *sha = sha256_hex(data);
            }
        }
        assert_eq!(
            checked(&reordered, swapped).unwrap_err().text(),
            "firmware_patch_order"
        );
        let stock = Reviewed::stock();
        assert_eq!(
            checked(&stock, back).unwrap_err().text(),
            "firmware_mismatch:WIFI_RAM_CODE_MT6582"
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
