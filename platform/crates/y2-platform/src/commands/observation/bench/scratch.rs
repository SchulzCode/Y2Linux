//! `bench.Scratch`: a private run directory below `<volume>/.y2-bench`, held by descriptor from the
//! verified volume's mount; every step first checks the mount is still the one measured.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    fd::{check, cname, fstat, fstatvfs, openat},
    Error, Result,
};
use crate::commands::observation::snapshot::{device_instance, storage};
use crate::reply::Failure;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs::File,
    io,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::fs::OpenOptionsExt,
    },
};
use y2p_core::{py, Root};

/// The mount a scratch directory lives on: boot, mount, device and block instance.
#[derive(Clone, PartialEq)]
pub(super) struct Identity(
    pub(super) Value,
    pub(super) Value,
    pub(super) String,
    pub(super) Option<String>,
);

/// `bench.Scratch`: never resolves workload names against a replaceable mountpoint path.
pub(super) struct Scratch<'a> {
    root: &'a Root,
    volume: String,
    volume_fd: Option<File>,
    base_fd: Option<File>,
    fd: Option<File>,
    name: String,
    files: BTreeSet<String>,
    pub(super) child_outputs: bool,
    pub(super) initial: Identity,
    pub(super) initial_uuid: Value,
}

impl<'a> Scratch<'a> {
    pub(super) fn new(root: &'a Root, volume: &str, budget: u64) -> Result<Scratch<'a>> {
        if volume != "/data" && volume != "/media/sd" {
            return Err(Error::value("scratch_volume_not_allowed"));
        }
        let name = format!(
            "run-{}",
            py::uuid4()
                .map_err(|e| Error::os(e, None))?
                .replace('-', "")
        );
        let mut scratch = Scratch {
            root,
            volume: volume.into(),
            volume_fd: None,
            base_fd: None,
            fd: None,
            name,
            files: BTreeSet::new(),
            child_outputs: false,
            initial: Identity(Value::Null, Value::Null, String::new(), None),
            initial_uuid: Value::Null,
        };
        scratch.initial = scratch.identity()?;
        let observed = observe_storage(root)?;
        let item = observed["volumes"]
            .as_array()
            .and_then(|v| v.iter().find(|v| v["path"] == volume))
            .cloned()
            .unwrap_or(Value::Null);
        if item["state"] != "Ready" || item["space_state"] != "Normal" {
            return Err(Error::value("healthy_identity_verified_volume_required"));
        }
        scratch.initial_uuid = item["uuid"].clone();
        match scratch.open_directories(budget) {
            Ok(()) => Ok(scratch),
            Err(error) => {
                scratch.close();
                Err(error)
            }
        }
    }

    fn open_directories(&mut self, budget: u64) -> Result<()> {
        let path = self.root.path(&self.volume);
        let volume = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)
            .map_err(|e| Error::os(e, Some(&path.to_string_lossy())))?;
        let vfs = fstatvfs(volume.as_raw_fd())?;
        self.volume_fd = Some(volume);
        let frsize = u128::from(vfs.frsize);
        let reserve = (96u128 * 1024 * 1024).max(u128::from(vfs.blocks) * frsize / 10);
        if u128::from(vfs.bavail) * frsize < u128::from(budget) + reserve {
            return Err(Error::value("insufficient_scratch_reserve"));
        }
        self.ensure()?;
        let volume_fd = self.volume_fd.as_ref().expect("volume").as_raw_fd();
        let bench = cname(".y2-bench");
        // SAFETY: a NUL-terminated name relative to the volume descriptor.
        match check(unsafe { libc::mkdirat(volume_fd, bench.as_ptr(), 0o700) }) {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(Error::os(e, Some(".y2-bench"))),
            Ok(_) => {}
        }
        let base = openat(
            volume_fd,
            ".y2-bench",
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
            0,
        )?;
        let meta = fstat(base.as_raw_fd())?;
        self.base_fd = Some(base);
        // SAFETY: geteuid has no preconditions.
        if meta.uid != unsafe { libc::geteuid() } || meta.mode & 0o077 != 0 {
            return Err(Error::value("unsafe_scratch_permissions"));
        }
        let base_fd = self.base_fd.as_ref().expect("base").as_raw_fd();
        let run = cname(&self.name);
        // SAFETY: a NUL-terminated name relative to the base descriptor.
        check(unsafe { libc::mkdirat(base_fd, run.as_ptr(), 0o700) })
            .map_err(|e| Error::os(e, Some(&self.name)))?;
        self.fd = Some(openat(
            base_fd,
            &self.name,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
            0,
        )?);
        self.ensure()
    }

    fn identity(&self) -> Result<Identity> {
        let found = y2p_core::mounts::mounts_at(self.root, &self.volume);
        if found.len() != 1 {
            return Err(Error::raised(libc::ENODEV, "source_disappeared"));
        }
        let mount = &found[0];
        let device = mount.device_id.clone();
        let read_only = mount.has_option("ro");
        if !self.root.path(&format!("/sys/dev/block/{device}")).exists() || read_only {
            return Err(Error::raised(libc::ENODEV, "source_unavailable"));
        }
        Ok(Identity(
            Value::from(self.root.boot_id()),
            json!(mount.mount_id),
            device.clone(),
            device_instance(self.root, &device),
        ))
    }

    pub(super) fn ensure(&self) -> Result<()> {
        if self.identity()? != self.initial {
            return Err(Error::raised(libc::ENODEV, "source_generation_changed"));
        }
        if let Some(volume) = &self.volume_fd {
            let path = self.root.path(&self.volume);
            let current = std::fs::metadata(&path)
                .map_err(|e| Error::os(e, Some(&path.to_string_lossy())))?;
            use std::os::unix::fs::MetadataExt;
            if current.dev() != fstat(volume.as_raw_fd())?.dev {
                return Err(Error::raised(libc::ENODEV, "source_device_changed"));
            }
        }
        Ok(())
    }

    pub(super) fn fd(&self) -> RawFd {
        self.fd.as_ref().expect("scratch").as_raw_fd()
    }

    fn alnum(name: &str) -> bool {
        !name.is_empty() && name.chars().all(char::is_alphanumeric)
    }

    pub(super) fn open(&mut self, name: &str) -> Result<File> {
        if !Self::alnum(name) {
            return Err(Error::value("invalid_scratch_name"));
        }
        self.ensure()?;
        let file = openat(
            self.fd(),
            name,
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW,
            0o600,
        )?;
        self.files.insert(name.into());
        Ok(file)
    }

    pub(super) fn rename(&mut self, source: &str, target: &str) -> Result<()> {
        if !self.files.contains(source) || !Self::alnum(target) || self.files.contains(target) {
            return Err(Error::value("unowned_scratch_rename"));
        }
        self.ensure()?;
        let (from, to) = (cname(source), cname(target));
        // SAFETY: NUL-terminated names relative to the run directory.
        check(unsafe { libc::renameat(self.fd(), from.as_ptr(), self.fd(), to.as_ptr()) })
            .map_err(|e| {
                let (text, class) = py::os_error(&e, Some(source));
                Error {
                    class: class.into(),
                    text: format!("{text} -> {}", py::repr(target)),
                    errno: e.raw_os_error(),
                }
            })?;
        self.files.remove(source);
        self.files.insert(target.into());
        Ok(())
    }

    pub(super) fn unlink(&mut self, name: &str) -> Result<()> {
        if !self.files.contains(name) {
            return Err(Error::value("unowned_scratch_file"));
        }
        self.ensure()?;
        let path = cname(name);
        // SAFETY: a NUL-terminated name relative to the run directory.
        check(unsafe { libc::unlinkat(self.fd(), path.as_ptr(), 0) })
            .map_err(|e| Error::os(e, Some(name)))?;
        self.files.remove(name);
        Ok(())
    }

    /// Descriptor-relative cleanup can never touch a replacement filesystem.
    pub(super) fn close(&mut self) {
        if let Some(fd) = self.fd.take() {
            if self.child_outputs {
                // Only known outputs in our new private run, with fd-relative symlink-safe removal.
                // Never recurse from /data or /media/sd.
                for directory in ["scan-media", "logs"] {
                    let _ = remove_tree(fd.as_raw_fd(), directory);
                }
                for name in [
                    "library.sqlite",
                    "library.sqlite-wal",
                    "library.sqlite-shm",
                    "scan.sqlite",
                    "scan.sqlite-wal",
                    "scan.sqlite-shm",
                ] {
                    self.files.insert(name.into());
                }
            }
            for name in &self.files {
                let path = cname(name);
                // SAFETY: a NUL-terminated name relative to the run directory.
                unsafe { libc::unlinkat(fd.as_raw_fd(), path.as_ptr(), 0) };
            }
        }
        if let Some(base) = self.base_fd.take() {
            let run = cname(&self.name);
            // SAFETY: a NUL-terminated name relative to the base directory.
            unsafe { libc::unlinkat(base.as_raw_fd(), run.as_ptr(), libc::AT_REMOVEDIR) };
        }
        self.volume_fd = None;
    }
}

/// `storage(ctx)` inside a benchmark: a refused observation (an `int()` of odd sysfs text) is the
/// benchmark's failure.
pub(super) fn observe_storage(root: &Root) -> Result<Value> {
    storage(root).map_err(|error| match error {
        Failure::Os { text, class } => Error {
            class: class.into(),
            text,
            errno: None,
        },
        other => Error::value(&other.message()),
    })
}

impl Drop for Scratch<'_> {
    fn drop(&mut self) {
        self.close();
    }
}

/// `shutil.rmtree(name, dir_fd=directory)`: descriptor-relative, never following a symlink.
fn remove_tree(directory: RawFd, name: &str) -> io::Result<()> {
    let path = cname(name);
    // SAFETY: a NUL-terminated name relative to a directory descriptor.
    let fd = check(unsafe {
        libc::openat(
            directory,
            path.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    })?;
    // SAFETY: a new descriptor this process owns.
    let handle = unsafe { OwnedFd::from_raw_fd(fd) };
    let listing = format!("/proc/self/fd/{}", handle.as_raw_fd());
    for entry in std::fs::read_dir(listing)?.flatten() {
        let child = entry.file_name().to_string_lossy().into_owned();
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        if is_dir {
            remove_tree(handle.as_raw_fd(), &child)?;
        } else {
            let child = cname(&child);
            // SAFETY: a NUL-terminated name relative to the directory.
            check(unsafe { libc::unlinkat(handle.as_raw_fd(), child.as_ptr(), 0) })?;
        }
    }
    drop(handle);
    // SAFETY: a NUL-terminated name relative to the parent directory.
    check(unsafe { libc::unlinkat(directory, path.as_ptr(), libc::AT_REMOVEDIR) }).map(drop)
}
