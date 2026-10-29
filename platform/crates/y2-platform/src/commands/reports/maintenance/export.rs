//! `export-state`: the owner's private backup of settings (network credentials and the library on
//! request) as one archive with a manifest.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    atomic_json, data_volume, free, fsync_path, inspect, not_found, open_data, os, os_plain,
    private_directory, regular_bytes, sha256_hex, tarball, uuid4, volume, Failure, Lease, OwnedFd,
    Path, Py, Root, SETTINGS,
};
use std::io::Write;
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::fs::PermissionsExt;

/// `urllib.parse.quote_from_bytes(path)` with `/` kept (`Path.as_uri()`).
fn quote(path: &str) -> String {
    path.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"_.-~/".contains(&b) {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// `tarfile.add(path, arcname)` metadata: the file's own (user and group names by lookup, mtime a
/// float as os.stat gives it).
fn tar_info(path: &Path, name: &str) -> Result<tarball::Info, Failure> {
    let meta = path.symlink_metadata().map_err(|e| os(e, path))?;
    let lookup = |uid: bool, id: u32| -> String {
        // SAFETY: getpwuid/getgrgid return static records or NULL.
        unsafe {
            let name = if uid {
                let p = libc::getpwuid(id);
                if p.is_null() {
                    std::ptr::null()
                } else {
                    (*p).pw_name
                }
            } else {
                let g = libc::getgrgid(id);
                if g.is_null() {
                    std::ptr::null()
                } else {
                    (*g).gr_name
                }
            };
            if name.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(name)
                    .to_string_lossy()
                    .into_owned()
            }
        }
    };
    Ok(tarball::Info {
        name: name.to_owned(),
        mode: meta.mode(),
        uid: u64::from(meta.uid()),
        gid: u64::from(meta.gid()),
        size: meta.len(),
        mtime: tarball::Mtime::Float(meta.mtime() as f64 + meta.mtime_nsec() as f64 * 1e-9),
        uname: lookup(true, meta.uid()),
        gname: lookup(false, meta.gid()),
    })
}

/// manifest.json of a state export: what it holds and how consistent it is (written sorted).
#[derive(serde::Serialize)]
struct Manifest<'a> {
    schema: u8,
    /// The workload record of the export (time, boot, versions).
    record: y2p_core::json::Object,
    contains_network_credentials: bool,
    excludes_bonds_and_private_keys: bool,
    consistency: &'static str,
    files: Vec<ExportedFile<'a>>,
    /// The sources that do not exist on this player.
    absent: Vec<&'a str>,
}

#[derive(serde::Serialize)]
struct ExportedFile<'a> {
    /// The member's name in the archive.
    name: &'a str,
    /// Its path below /data.
    original: &'a str,
    sha256: String,
    bytes: u64,
}

fn file_digest(path: &Path) -> Result<String, Failure> {
    let data = std::fs::read(path).map_err(|e| os(e, path))?;
    Ok(sha256_hex(&data))
}

/// `export-state` (a Maintenance workload): the settings; network credentials and the library
/// database on request.
pub(super) fn export(
    root: &Root,
    include_database: bool,
    include_network: bool,
) -> Result<Py, Failure> {
    let _lease = Lease::take(root, "Maintenance")?;
    let volume = data_volume(root)?;
    let directory = root.path("/data/exports");
    private_directory(&directory)?;
    let token = uuid4()?.replace('-', "");
    let scratch = directory.join(format!(".export-{token}"));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&scratch)
        .map_err(|e| os(e, &scratch))?;
    let data = match open_data(root) {
        Ok(data) => data,
        Err(error) => {
            let _ = std::fs::remove_dir(&scratch);
            return Err(error);
        }
    };
    let result = export_into(
        root,
        &data,
        &directory,
        &scratch,
        &token,
        &volume,
        include_database,
        include_network,
    );
    drop(data);
    for entry in std::fs::read_dir(&scratch).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.symlink_metadata().is_ok_and(|m| m.is_file()) {
            let _ = std::fs::remove_file(&path);
        }
    }
    std::fs::remove_dir(&scratch)
        .map_err(|e| os(e, &scratch))
        .and(result)
}

#[allow(clippy::too_many_arguments)]
fn export_into(
    root: &Root,
    data: &OwnedFd,
    directory: &Path,
    scratch: &Path,
    token: &str,
    volume: &volume::Volume,
    include_database: bool,
    include_network: bool,
) -> Result<Py, Failure> {
    let mut files: Vec<(String, String)> = Vec::new();
    let mut absent: Vec<&str> = Vec::new();
    let mut sources = vec![SETTINGS, "bluetooth/preferred-audio"];
    if include_network {
        sources.extend(["network/wpa_supplicant.conf", "network/enabled"]);
    }
    for relative in sources {
        let raw = match regular_bytes(data, relative, 8 << 20) {
            Err(e) if not_found(&e) => {
                absent.push(relative);
                continue;
            }
            other => other?,
        };
        free(data, raw.len() as u64)?;
        let name = relative.replace('/', "--");
        let path = scratch.join(&name);
        std::fs::write(&path, &raw).map_err(|e| os(e, &path))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| os(e, &path))?;
        files.push((name, relative.to_owned()));
    }
    if include_database {
        let relative = "reborn/library.db";
        match inspect(data, relative)? {
            None => absent.push(relative),
            Some(before) => {
                use y2p_core::reports_sqlite::{Database, Error, Value};
                let database = root.path(&format!("/data/{relative}"));
                let target = scratch.join("library.db");
                let deadline = root.clock().monotonic() + 30.0;
                let sqlite = |error: Error| match error {
                    Error::Callback(text) => Failure::Value(text),
                    other => Failure::value(format!("sqlite3.{}", other.class())),
                };
                {
                    let source = Database::open(
                        &format!("file://{}?mode=ro", quote(&database.to_string_lossy())),
                        500,
                    )
                    .map_err(sqlite)?;
                    let dest = Database::open(&target.to_string_lossy(), 5000).map_err(sqlite)?;
                    source
                        .backup(&dest, 128, 50, |_, _, total| {
                            if root.clock().monotonic() > deadline
                                || i64::from(total) * 4096 > 128 << 20
                            {
                                return Err("database_export_limit".into());
                            }
                            free(data, 1 << 20).map_err(|e| e.message())
                        })
                        .map_err(sqlite)?;
                    let check = dest.query("PRAGMA quick_check").map_err(sqlite)?;
                    if check.first().map(Vec::as_slice) != Some(&[Value::Text("ok".into())][..]) {
                        return Err(Failure::value("database_backup_integrity"));
                    }
                }
                let after = inspect(data, relative)?;
                let key = |v: &Py| (v.get("device").cloned(), v.get("inode").cloned());
                if after.as_ref().is_none_or(|a| key(a) != key(&before)) {
                    return Err(Failure::value("database_source_replaced"));
                }
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600))
                    .map_err(|e| os(e, &target))?;
                files.push(("library.db".into(), relative.into()));
            }
        }
    }
    let mut listed = Vec::new();
    for (name, original) in &files {
        let path = scratch.join(name);
        listed.push(ExportedFile {
            name,
            original,
            sha256: file_digest(&path)?,
            bytes: std::fs::metadata(&path).map_err(|e| os(e, &path))?.len(),
        });
    }
    let manifest = Manifest {
        schema: 1,
        record: y2p_core::devices_support::record(
            root,
            "owner-state-export",
            serde_json::json!({}),
        ),
        contains_network_credentials: include_network,
        excludes_bonds_and_private_keys: true,
        consistency: "SQLite online backup; each session file atomic; no cross-file transaction",
        files: listed,
        absent,
    };
    atomic_json(&scratch.join("manifest.json"), &manifest, true)?;
    let output = directory.join(format!(".state-{token}.partial"));
    let mut stream = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&output)
        .map_err(|e| os(e, &output))?;
    let mut tar = tarball::Writer::new();
    let names: Vec<String> = std::iter::once("manifest.json".to_owned())
        .chain(files.iter().map(|(n, _)| n.clone()))
        .collect();
    for name in &names {
        let path = scratch.join(name);
        free(
            data,
            std::fs::metadata(&path).map_err(|e| os(e, &path))?.len(),
        )?;
        let body = std::fs::read(&path).map_err(|e| os(e, &path))?;
        tar.add(&tar_info(&path, name)?, &body);
    }
    let now = (root.clock().wall_ns() / 1_000_000_000) as u32;
    // tarfile 'w:gz': GzipFile at level 9 with the current time in its header.
    stream
        .write_all(&tarball::gzip(&tar.finish(), 9, now))
        .and_then(|()| stream.sync_all())
        .map_err(os_plain)?;
    drop(stream);
    if data_volume(root)?.generation != volume.generation {
        return Err(Failure::value("export_data_generation_changed"));
    }
    let complete = directory.join(format!("state-{token}.tar.gz"));
    std::fs::rename(&output, &complete).map_err(|e| os(e, &output))?;
    fsync_path(directory)?;
    let digest = file_digest(&complete)?;
    let bytes = std::fs::metadata(&complete)
        .map_err(|e| os(e, &complete))?
        .len();
    Ok(Py::dict()
        .with("state", Py::str("Complete"))
        .with(
            "path",
            Py::Str(format!("/data/exports/state-{token}.tar.gz")),
        )
        .with("sha256", Py::Str(digest))
        .with("bytes", Py::from(bytes))
        .with("contains_network_credentials", Py::Bool(include_network)))
}
