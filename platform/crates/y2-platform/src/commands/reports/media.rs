//! `report save`: the SD card's state as the storage group records it (our mount, its claim,
//! `can_write`), the report's own write window on a protected card, and writing the file below a
//! destination without following a link out of it.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    build, clock, json_dict, or_dict,
    pyjson::Py,
    state::{self, Reports, DESTINATIONS},
};
use crate::reply::{atomic_json, Failure};
use std::os::fd::AsFd;
use std::{
    ffi::CString,
    io,
    os::fd::{AsRawFd, OwnedFd},
    path::Path,
    time::Duration,
};
pub use y2p_core::mounts::{mountinfo, mounts_at, Mount};
use y2p_core::{command, Root};

/// The mounts on /media/sd.
fn mounts(root: &Root) -> Vec<Mount> {
    mounts_at(root, "/media/sd")
}

fn eq(value: Option<&Py>, other: &Py) -> bool {
    value.unwrap_or(&Py::None).py_eq(other)
}

fn text(value: Option<String>) -> Py {
    value.map_or(Py::None, Py::Str)
}

/// `media.owned_mount`.
fn owned_mount(root: &Root, mounted: &Mount) -> bool {
    let device = root.path(&format!("/sys/dev/block/{}", mounted.device_id));
    if device.exists() {
        return std::fs::canonicalize(&device)
            .is_ok_and(|real| real.to_string_lossy().contains("/11240000.mmc/"));
    }
    let claim = json_dict(root, "/run/y2/media-mount.json");
    // Removed cards no longer have a sysfs controller path. The retained claim permits only the
    // exact mount we created, never an arbitrary mountpoint.
    eq(claim.get("boot_id"), &text(root.boot_id()))
        && eq(claim.get("mount_id"), &Py::int(mounted.mount_id))
        && eq(claim.get("device_id"), &Py::str(mounted.device_id.clone()))
        && eq(claim.get("device"), &Py::str(mounted.source.clone()))
}

/// The SD claim and our current SD mount (None when the card is not mounted by us this boot).
fn sd_state(root: &Root) -> (Py, Option<Mount>) {
    let claim = or_dict(json_dict(root, "/run/y2/media-mount.json"));
    let mut old = mounts(root);
    if old.len() == 1
        && owned_mount(root, &old[0])
        && eq(claim.get("mount_id"), &Py::int(old[0].mount_id))
        && eq(claim.get("boot_id"), &text(root.boot_id()))
    {
        let mount = old.remove(0);
        return (claim, Some(mount));
    }
    (claim, None)
}

/// `media.can_write`: True when the SD takes writes now or on a window (not for a damaged card).
pub fn can_write(root: &Root) -> bool {
    let (claim, mount) = sd_state(root);
    mount.is_some_and(|m| {
        m.options.iter().any(|o| o == "rw")
            || claim.get("read_only").and_then(Py::as_str) == Some("protected")
    })
}

const SD_WRITE: &str = "/run/y2/sd-write.json";
const WRITE_HOLD_MAX: i64 = 600;
const WRITE_HOLDERS: [&str; 4] = ["usb", "reborn", "report", "radiofw"];

/// `media._writers`.
fn writers(root: &Root, mount: &Mount) -> Vec<(String, Py)> {
    let value = or_dict(json_dict(root, SD_WRITE));
    if !eq(value.get("boot_id"), &text(root.boot_id()))
        || !eq(value.get("mount_id"), &Py::int(mount.mount_id))
    {
        return Vec::new();
    }
    let now = root.clock().monotonic();
    match value.get("holders") {
        Some(Py::Dict(holders)) => holders
            .iter()
            .filter(|(k, v)| {
                WRITE_HOLDERS.contains(&k.as_str()) && v.number().is_some_and(|t| t > now)
            })
            .cloned()
            .collect(),
        _ => Vec::new(),
    }
}

/// `media._apply_window`: remount to match the holders; records them. Returns writable now.
fn apply_window(root: &Root, mount: &Mount, holders: &[(String, Py)]) -> Result<bool, Failure> {
    let record = Py::dict()
        .with("schema", Py::int(1))
        .with("boot_id", text(root.boot_id()))
        .with("mount_id", Py::int(mount.mount_id))
        .with("holders", Py::Dict(holders.to_vec()));
    atomic_json(&root.path(SD_WRITE), &record, false)?;
    let mut writable = mount.options.iter().any(|o| o == "rw");
    if !holders.is_empty() != writable {
        let mode = if holders.is_empty() {
            "remount,ro"
        } else {
            "remount,rw"
        };
        command::run(root, &["mount", "-o", mode, "/media/sd"], 10.0);
        let now = mounts(root);
        writable = now
            .first()
            .is_some_and(|m| m.options.iter().any(|o| o == "rw"));
    }
    Ok(writable)
}

/// Open the report's write window for `seconds` (or close it, `release`) on a protected card; the
/// holders of the window then (none when there is no window).
fn write_window(root: &Root, seconds: i64, release: bool) -> Result<Vec<String>, Failure> {
    let runtime = root.path("/run/y2");
    crate::reply::mkdir_parents(&runtime, 0o700)?;
    let path = runtime.join("media.lock");
    // At most 5 s on the root's clock (a test's clock moves on at each sleep instead of waiting).
    let clock = root.clock();
    let deadline = clock.monotonic() + 5.0;
    let _lock = loop {
        match y2p_core::lock::try_exclusive(&path).map_err(|e| super::os(e, &path))? {
            Some(lock) => break lock,
            // A check or a repair holds the card; a short admission passes.
            None if clock.monotonic() >= deadline => return Ok(Vec::new()),
            None => clock.sleep(Duration::from_millis(200)),
        }
    };
    let (claim, mount) = sd_state(root);
    let Some(mount) = mount else {
        return Ok(Vec::new());
    };
    match claim.get("read_only") {
        None | Some(Py::None) => return Ok(Vec::new()),
        Some(Py::Str(s)) if s == "protected" => {}
        _ => return Ok(Vec::new()), // being checked or damaged: never a window
    }
    let mut holders = writers(root, &mount);
    holders.retain(|(k, _)| k != "report");
    if !release {
        let hold = seconds.clamp(1, WRITE_HOLD_MAX) as f64;
        holders.push(("report".into(), Py::Float(root.clock().monotonic() + hold)));
    }
    apply_window(root, &mount, &holders)?;
    let mut names: Vec<String> = holders.into_iter().map(|(k, _)| k).collect();
    names.sort();
    Ok(names)
}

/// `_write_into(mount, top, data, stamp)`: write data as top/FOLDER/y2linux-report-<stamp>.tar.gz
/// below mount, durably. Every folder must be a real directory on the mounted filesystem itself (no
/// symlink out of it); the file is complete before it gets its name.
fn write_into(mount: &Path, top: &str, data: &[u8], stamp: &str) -> io::Result<String> {
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    let mut dir = open_at(None, &mount.to_string_lossy(), flags, 0)?;
    let device = fstat(&dir)?.dev;
    for part in top.split('/').filter(|p| !p.is_empty()) {
        dir = open_at(Some(&dir), part, flags, 0)?;
    }
    let folder_name = CString::new(build::FOLDER).expect("no NUL");
    // SAFETY: a valid directory fd and a NUL-terminated name.
    if unsafe { libc::mkdirat(dir.as_raw_fd(), folder_name.as_ptr(), 0o755) } != 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EEXIST) {
            return Err(error);
        }
    }
    let folder = open_at(Some(&dir), build::FOLDER, flags, 0)?;
    drop(dir);
    if fstat(&folder)?.dev != device {
        return Err(io::Error::from_raw_os_error(libc::EXDEV)); // report_folder_elsewhere
    }
    let space = y2p_core::sys::fstatvfs(folder.as_fd())?;
    if (space.bavail as u128) * (space.frsize as u128) < data.len() as u128 + (1 << 20) {
        return Err(io::Error::from_raw_os_error(libc::ENOSPC)); // report_no_space
    }
    let mut chosen = None;
    for n in 1..100 {
        let name = format!(
            "y2linux-report-{stamp}{}.tar.gz",
            if n == 1 {
                String::new()
            } else {
                format!("-{n}")
            }
        );
        match lstat_at(&folder, &name) {
            Err(e) if e.raw_os_error() == Some(libc::ENOENT) => {
                chosen = Some(name);
                break;
            }
            Err(e) => return Err(e),
            Ok(()) => {}
        }
    }
    let name = chosen.ok_or_else(|| io::Error::from_raw_os_error(libc::EEXIST))?; // names taken
    let temporary = format!(".{name}.part");
    let written = (|| {
        let fd = open_at(
            Some(&folder),
            &temporary,
            libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o644,
        )?;
        let mut file = std::fs::File::from(fd);
        io::Write::write_all(&mut file, data)?;
        file.sync_all()?;
        drop(file);
        rename_at(&folder, &temporary, &name)
    })();
    if let Err(error) = written {
        let c = CString::new(temporary).expect("no NUL");
        // SAFETY: a valid fd and name.
        unsafe { libc::unlinkat(folder.as_raw_fd(), c.as_ptr(), 0) };
        return Err(error);
    }
    // An SD card may be pulled right afterwards.
    // SAFETY: a valid fd.
    unsafe { libc::fsync(folder.as_raw_fd()) };
    Ok(name)
}

pub fn open_at(dir: Option<&OwnedFd>, name: &str, flags: i32, mode: u32) -> io::Result<OwnedFd> {
    let c = CString::new(name).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?;
    y2p_core::sys::openat(dir.map(|d| d.as_fd()), &c, flags, mode as libc::mode_t)
}

pub fn fstat(fd: &OwnedFd) -> io::Result<y2p_core::sys::Stat> {
    y2p_core::sys::fstat(fd.as_fd())
}

pub fn stat_at(dir: &OwnedFd, name: &str) -> io::Result<y2p_core::sys::Stat> {
    let c = CString::new(name).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?;
    y2p_core::sys::fstatat(dir.as_fd(), &c, libc::AT_SYMLINK_NOFOLLOW)
}

fn lstat_at(dir: &OwnedFd, name: &str) -> io::Result<()> {
    stat_at(dir, name).map(|_| ())
}

pub fn rename_at(dir: &OwnedFd, from: &str, to: &str) -> io::Result<()> {
    rename_between(dir, from, dir, to)
}

pub fn rename_between(
    from_dir: &OwnedFd,
    from: &str,
    to_dir: &OwnedFd,
    to: &str,
) -> io::Result<()> {
    let (a, b) = (
        CString::new(from).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?,
        CString::new(to).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?,
    );
    // SAFETY: valid fds and NUL-terminated names.
    if unsafe {
        libc::renameat(
            from_dir.as_raw_fd(),
            a.as_ptr(),
            to_dir.as_raw_fd(),
            b.as_ptr(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Build a report and save it onto Internal Storage or
/// the SD card. Asked for by the owner on the player, so no opt-in is needed: nothing leaves the
/// player unless the owner sends the file. It carries the install ID only when error reports are on.
pub fn save_to(
    root: &Root,
    destination: &str,
    topic: Option<&str>,
    progress: &build::Progress,
) -> Result<Py, Failure> {
    let Some(&(_, mount, top)) = DESTINATIONS.iter().find(|(n, _, _)| *n == destination) else {
        return Err(Failure::value("unknown_destination"));
    };
    if !state::destinations(root).contains(&destination) {
        return Ok(Py::dict()
            .with("saved", Py::Bool(false))
            .with("reason", Py::str("not_mounted")));
    }
    let value = Reports::load(root);
    let now = clock::Instant::now(root);
    let install = if value.enabled() {
        value.field("install_id")
    } else {
        Py::None
    };
    let (data, index) = build::build(
        root,
        &["manual".to_owned()],
        "",
        topic,
        install,
        now,
        progress,
    )?;
    progress.update("save", 0.0, true, build::Bytes::total(data.len()));
    let window = if destination == "sd" {
        write_window(root, 120, false)
    } else {
        Ok(Vec::new())
    };
    let written = match window {
        Err(error) => Err(error),
        Ok(holders) => {
            let result = write_into(&root.path(mount), top, &data, &clock::local_stamp(now))
                .map_err(super::os_plain);
            // The window ends whatever happened; its own OSError replaces the write's.
            match (holders.is_empty(), result) {
                (true, result) => result,
                (false, result) => write_window(root, 0, true).and(result),
            }
        }
    };
    let name = match written {
        Ok(name) => {
            progress.update("save", 1.0, true, build::Bytes::total(data.len()));
            name
        }
        Err(Failure::Os { text, .. }) => {
            let no_space = text.starts_with(&format!("[Errno {}]", libc::ENOSPC));
            return Ok(Py::dict().with("saved", Py::Bool(false)).with(
                "reason",
                Py::str(if no_space { "no_space" } else { "write_failed" }),
            ));
        }
        Err(other) => return Err(other),
    };
    let file = format!("{}/{name}", build::FOLDER);
    let report_id = index.get("report_id").cloned().unwrap_or(Py::None);
    let mut value = Reports::load(root);
    value.set(
        "last",
        state::Last {
            report_id: report_id.clone(),
            trigger: "manual",
            state: "saved",
            saved: Some((destination, &file)),
            created_utc: index.get("created_utc").cloned().unwrap_or(Py::None),
            bytes: data.len(),
        }
        .into(),
    );
    value.save(root)?;
    Ok(Py::dict()
        .with("saved", Py::Bool(true))
        .with("destination", Py::str(destination))
        .with("file", Py::str(file))
        .with("report_id", report_id)
        .with("bytes", Py::from(data.len())))
}

#[cfg(test)]
#[path = "media_tests.rs"]
mod media_tests;
