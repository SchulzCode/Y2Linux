//! The seven radio files: read from a stock system.img or a directory, checked against the reviewed
//! sizes and hashes (with the two board defaults derived from the stock reader library), installed
//! on Y2DATA in one rename.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    fs, os_path, py, sha256_hex, Error, Ext4, Files, Image, OpenOptions, Path, Result, Reviewed,
    Root, DEFAULTS, FILES, TARGET,
};
use std::io::Write;
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::fs::OpenOptionsExt;

/// The generic Wi-Fi/BT board records of the stock reader library, with the
/// default addresses cleared.
pub(super) fn defaults(custom: &[u8]) -> Result<Files> {
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

pub(super) fn get<'a>(files: &'a Files, name: &str) -> Option<&'a Vec<u8>> {
    files.iter().find(|(n, _)| n == name).map(|(_, d)| d)
}

/// Every reviewed file present, with the reviewed size and SHA-256.
pub(super) fn checked(reviewed: &Reviewed, files: Files) -> Result<Files> {
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
            let byte = get(&files, name)
                .and_then(|data| data.get(24))
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

/// The seven files from the stock system.img (sparse or raw ext4).
pub(super) fn from_system_image(reviewed: &Reviewed, path: &Path) -> Result<Files> {
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

/// The seven files from a directory (a root's /lib/firmware/mediatek/mt6582).
pub(super) fn from_directory(reviewed: &Reviewed, path: &Path) -> Result<Files> {
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

/// Cheap check for status polling: every file there with its size.
///
/// The service and the USB views ask every pass: the reviewed tables as they are, nothing copied.
pub fn present(root: &Root) -> bool {
    let target = root.path(TARGET);
    FILES.iter().chain(&DEFAULTS).all(|(name, size, _)| {
        target
            .join(name)
            .symlink_metadata()
            .is_ok_and(|m| m.len() == *size && m.file_type().is_file())
    })
}

/// Every reviewed file is in place with the reviewed hash.
pub(super) fn installed(root: &Root, reviewed: &Reviewed) -> bool {
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

/// Write the checked files to Y2DATA: a fresh directory, then one rename.
pub(super) fn install(root: &Root, reviewed: &Reviewed, files: Files) -> Result<()> {
    let files = checked(reviewed, files)?;
    let target = root.path(TARGET);
    let Some(parent) = target.parent().map(Path::to_path_buf) else {
        return Err(Error::Value("firmware_target_unsafe".into()));
    };
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
        out.write_all(data).and_then(|()| out.sync_all())?;
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
    directory.sync_all().map_err(Error::from)
}

fn rename_error(error: std::io::Error, from: &Path, to: &Path) -> Error {
    Error::Os(format!(
        "{} -> {}",
        py::os_error(&error, Some(&from.to_string_lossy())).0,
        py::repr(&to.to_string_lossy())
    ))
}
