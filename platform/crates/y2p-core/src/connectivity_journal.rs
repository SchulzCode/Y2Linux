//! Durable bounded journals on Y2DATA (`power_journal.append_bounded`), written only where /data is the
//! platform's own verified mount (`mounts::data_ready`). The Wi-Fi journal
//! (`/data/system/platform/wifi-journal.jsonl`) uses both; the power journal and the sleep diagnostics
//! append the same way (platform/CONTRACT.md, "Files": one `.previous` rotation under their lock).
// SPDX-License-Identifier: GPL-2.0-only
use crate::{record::private_directory, Root};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

/// `power_journal.append_bounded`: append one complete row to a private file on Y2DATA; rotate once
/// to `<stem>.previous.jsonl` when the row would pass `limit`. The lock is `.<stem>.lock` next to it,
/// taken without waiting (a busy lock is `EWOULDBLOCK`: the row is dropped, never waited for). A
/// directory that is not one, or a journal that is not a regular file, is `InvalidData`.
pub fn append_bounded(
    root: &Root,
    name: &str,
    encoded: &[u8],
    limit: u64,
    durable: bool,
) -> io::Result<()> {
    let path = root.path(name);
    let parent = path.parent().unwrap_or(Path::new("/"));
    private_directory(parent)?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let _lock = crate::lock::try_exclusive(&parent.join(format!(".{stem}.lock")))?
        .ok_or_else(|| io::Error::from_raw_os_error(libc::EWOULDBLOCK))?;
    append(&path, encoded, limit, durable)
}

fn open(path: &Path, nonblock: bool) -> io::Result<File> {
    OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(
            libc::O_NOFOLLOW | libc::O_CLOEXEC | if nonblock { libc::O_NONBLOCK } else { 0 },
        )
        .open(path)
}

fn append(path: &Path, encoded: &[u8], limit: u64, durable: bool) -> io::Result<()> {
    let mut file = open(path, true)?;
    let meta = file.metadata()?;
    if !meta.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "power_journal_not_regular",
        ));
    }
    if meta.len() + encoded.len() as u64 > limit {
        drop(file);
        let mut previous = path.to_path_buf();
        previous.set_extension("previous.jsonl");
        fs::rename(path, &previous)?;
        file = open(path, false)?;
    }
    // Regular-file append is bounded; write_all handles short writes without dropping a row.
    file.write_all(encoded)?;
    if durable {
        file.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{mounts::data_ready, root::tests::tempdir::Dir};
    use std::os::unix::fs::MetadataExt;

    fn put(dir: &Dir, name: &str, text: &str) {
        let path = dir.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn data_is_ready_only_on_the_verified_mount() {
        let dir = Dir::new();
        let root = Root::new(dir.path());
        assert!(!data_ready(&root));
        put(&dir, "data/.y2data-schema", "1\n");
        put(&dir, "run/y2-data-device", "/dev/mmcblk0p9\n");
        put(
            &dir,
            "proc/self/mountinfo",
            "22 1 179:2 / / ro - ext4 /dev/mmcblk0p2 ro\n30 22 179:9 / /data rw - ext4 /dev/mmcblk0p9 rw\n",
        );
        assert!(data_ready(&root));
        put(&dir, "run/y2-data-device", "/dev/sda1\n");
        assert!(!data_ready(&root));
    }

    #[test]
    fn rows_append_and_rotate_once() {
        let dir = Dir::new();
        let root = Root::new(dir.path());
        let name = "/data/system/platform/wifi-journal.jsonl";
        append_bounded(&root, name, b"{\"a\":1}\n", 16, true).unwrap();
        append_bounded(&root, name, b"{\"b\":2}\n", 16, false).unwrap();
        let platform = dir.path().join("data/system/platform");
        assert_eq!(
            fs::read_to_string(platform.join("wifi-journal.jsonl")).unwrap(),
            "{\"a\":1}\n{\"b\":2}\n"
        );
        append_bounded(&root, name, b"{\"c\":3}\n", 16, false).unwrap();
        assert_eq!(
            fs::read_to_string(platform.join("wifi-journal.jsonl")).unwrap(),
            "{\"c\":3}\n"
        );
        assert_eq!(
            fs::read_to_string(platform.join("wifi-journal.previous.jsonl")).unwrap(),
            "{\"a\":1}\n{\"b\":2}\n"
        );
        assert!(platform.join(".wifi-journal.lock").exists());
        assert_eq!(
            fs::metadata(platform.join("wifi-journal.jsonl"))
                .unwrap()
                .mode()
                & 0o777,
            0o600
        );
        let busy = crate::lock::exclusive(&platform.join(".wifi-journal.lock")).unwrap();
        let error = append_bounded(&root, name, b"x\n", 16, false).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(libc::EWOULDBLOCK));
        drop(busy);
    }
}
