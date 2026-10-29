//! `common.atomic_json` and `boot.private_directory`: how the platform publishes its records.
// SPDX-License-Identifier: GPL-2.0-only
use crate::{
    clock,
    json::{dumps_sorted, Separators},
};
use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions, Permissions},
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

/// Publish one complete record: sorted compact JSON and a newline, written to a temporary file in the
/// same directory and renamed over `path`. `durable` adds fsync of the file and then of the directory.
/// The temporary name is `.<name>.<pid>.<monotonic_ns>.tmp`, without randomness: the kernel's random
/// pool may not be ready early in boot (`test_platform_records_do_not_need_the_random_pool`).
pub fn atomic_json<T: Serialize + ?Sized>(
    path: &Path,
    value: &T,
    durable: bool,
    mode: u32,
) -> io::Result<()> {
    let text = dumps_sorted(value, Separators::Compact).map_err(io::Error::other)? + "\n";
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let temporary = parent.join(format!(
        ".{}.{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id(),
        clock::monotonic_ns()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&temporary)?;
        file.set_permissions(Permissions::from_mode(mode))?;
        file.write_all(text.as_bytes())?;
        if durable {
            file.sync_all()?;
        }
        drop(file);
        fs::rename(&temporary, path)?;
        if durable {
            File::open(parent)?.sync_all()?;
        }
        Ok(())
    })();
    match fs::remove_file(&temporary) {
        Err(error) if error.kind() != io::ErrorKind::NotFound && result.is_ok() => Err(error),
        _ => result,
    }
}

/// `boot.private_directory`: create each missing component 0700 (never through a symlink or a
/// non-directory) and set the directory itself to 0700, also when it already existed.
pub fn private_directory(path: &Path) -> io::Result<()> {
    let mut components: Vec<&Path> = path.ancestors().collect();
    components.reverse();
    for component in components.into_iter().filter(|c| !c.as_os_str().is_empty()) {
        match fs::symlink_metadata(component) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "state_directory_not_directory",
                ))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                fs::DirBuilder::new().mode(0o700).create(component)?;
            }
            Err(error) => return Err(error),
        }
    }
    fs::set_permissions(path, Permissions::from_mode(0o700))
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod record_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::root::tests::tempdir::Dir;
    use serde_json::json;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn records_are_written_as_python_writes_them() {
        let dir = Dir::new();
        let path = dir.path().join("power.json");
        atomic_json(
            &path,
            &json!({"state": "Normal", "boot_id": "b", "monotonic_ns": 5, "v": 4.2}),
            true,
            0o600,
        )
        .unwrap();
        // python3: atomic_json(path, {...}, durable=True)
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{\"boot_id\":\"b\",\"monotonic_ns\":5,\"state\":\"Normal\",\"v\":4.2}\n"
        );
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o7777, 0o600);
        atomic_json(&path, &json!([]), false, 0o644).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "[]\n");
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o7777, 0o644);
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            1,
            "no temporary file left"
        );
    }

    #[test]
    fn private_directories_refuse_files_and_tighten_modes() {
        let dir = Dir::new();
        let state = dir.path().join("data/system/platform");
        private_directory(&state).unwrap();
        assert_eq!(fs::metadata(&state).unwrap().mode() & 0o7777, 0o700);
        assert_eq!(
            fs::metadata(dir.path().join("data")).unwrap().mode() & 0o7777,
            0o700
        );
        fs::set_permissions(&state, Permissions::from_mode(0o755)).unwrap();
        private_directory(&state).unwrap();
        assert_eq!(fs::metadata(&state).unwrap().mode() & 0o7777, 0o700);
        fs::write(dir.path().join("file"), "").unwrap();
        assert_eq!(
            private_directory(&dir.path().join("file/below"))
                .unwrap_err()
                .to_string(),
            "state_directory_not_directory"
        );
    }
}
