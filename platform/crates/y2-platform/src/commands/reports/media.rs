//! What `report save` needs from observe.py and media.py: `mountinfo`, the SD card's state
//! (`_sd_state`, `owned_mount`, `can_write`), its write window (`write_window`, `sd_writable`), and
//! report.py's `save_to` / `_write_into`.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    boot_id, build, clock, json_dict, or_dict,
    pyjson::Py,
    state::{self, DESTINATIONS},
};
use crate::reply::{atomic_json, Failure};
use std::{
    ffi::CString,
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    path::Path,
    time::Duration,
};
use y2p_core::{clock as kernel, command, Root};

pub struct Mount {
    pub mount_id: i64,
    pub device_id: String,
    pub path: String,
    pub options: Vec<String>,
    pub source: String,
    pub super_options: Vec<String>,
}

fn unescape(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(index) = rest.find('\\') {
        out.push_str(&rest[..index]);
        let code = &rest[index + 1..];
        let replaced = ["040", "011", "012", "134"]
            .iter()
            .find(|c| code.starts_with(*c))
            .map(|c| char::from(u8::from_str_radix(c, 8).expect("octal")));
        match replaced {
            Some(c) => {
                out.push(c);
                rest = &code[3..];
            }
            None => {
                out.push('\\');
                rest = code;
            }
        }
    }
    out + rest
}

/// `observe.mountinfo(text)`.
pub fn mountinfo(text: Option<&str>) -> Vec<Mount> {
    let mut result = Vec::new();
    for line in super::pyre::splitlines(text.unwrap_or("")) {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        let a: Vec<&str> = left
            .split(super::pyre::isspace)
            .filter(|s| !s.is_empty())
            .collect();
        let b: Vec<&str> = right
            .split(super::pyre::isspace)
            .filter(|s| !s.is_empty())
            .collect();
        if a.len() < 6 || b.len() < 3 || !a[0].bytes().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(mount_id) = a[0].parse() else {
            continue;
        };
        result.push(Mount {
            mount_id,
            device_id: a[2].to_owned(),
            path: unescape(a[4]),
            options: a[5].split(',').map(str::to_owned).collect(),
            source: unescape(b[1]),
            super_options: b[2].split(',').map(str::to_owned).collect(),
        });
    }
    result
}

/// `media.mounts(ctx)`: the mounts on /media/sd.
fn mounts(root: &Root) -> Vec<Mount> {
    mountinfo(root.read("/proc/self/mountinfo").as_deref())
        .into_iter()
        .filter(|m| m.path == "/media/sd")
        .collect()
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
    eq(claim.get("boot_id"), &text(boot_id(root)))
        && eq(claim.get("mount_id"), &Py::int(mounted.mount_id))
        && eq(claim.get("device_id"), &Py::str(mounted.device_id.clone()))
        && eq(claim.get("device"), &Py::str(mounted.source.clone()))
}

/// `media._sd_state`: (claim, mount) of our current SD mount, else (claim, None).
fn sd_state(root: &Root) -> (Py, Option<Mount>) {
    let claim = or_dict(json_dict(root, "/run/y2/media-mount.json"));
    let mut old = mounts(root);
    if old.len() == 1
        && owned_mount(root, &old[0])
        && eq(claim.get("mount_id"), &Py::int(old[0].mount_id))
        && eq(claim.get("boot_id"), &text(boot_id(root)))
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
    if !eq(value.get("boot_id"), &text(boot_id(root)))
        || !eq(value.get("mount_id"), &Py::int(mount.mount_id))
    {
        return Vec::new();
    }
    let now = kernel::monotonic();
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
        .with("boot_id", text(boot_id(root)))
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

/// `media.write_window(ctx, 'report', seconds, release)`; its answer's holders.
fn write_window(root: &Root, seconds: i64, release: bool) -> Result<Vec<String>, Failure> {
    let runtime = root.path("/run/y2");
    crate::reply::mkdir_parents(&runtime, 0o700)?;
    let path = runtime.join("media.lock");
    let deadline = kernel::monotonic() + 5.0;
    let _lock = loop {
        match y2p_core::lock::try_exclusive(&path).map_err(|e| super::os(e, &path))? {
            Some(lock) => break lock,
            // A check or a repair holds the card; a short admission passes.
            None if kernel::monotonic() >= deadline => return Ok(Vec::new()),
            None => std::thread::sleep(Duration::from_millis(200)),
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
        holders.push(("report".into(), Py::Float(kernel::monotonic() + hold)));
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
    let device = fstat(&dir)?.st_dev;
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
    if fstat(&folder)?.st_dev != device {
        return Err(io::Error::from_raw_os_error(libc::EXDEV)); // report_folder_elsewhere
    }
    // SAFETY: a zeroed statvfs is a valid output buffer.
    let mut space: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: a valid fd and buffer.
    if unsafe { libc::fstatvfs(folder.as_raw_fd(), &mut space) } != 0 {
        return Err(io::Error::last_os_error());
    }
    if (space.f_bavail as u128) * (space.f_frsize as u128) < data.len() as u128 + (1 << 20) {
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
    let base = dir.map_or(libc::AT_FDCWD, |d| d.as_raw_fd());
    // SAFETY: a valid directory fd (or AT_FDCWD) and a NUL-terminated name.
    let fd = unsafe { libc::openat(base, c.as_ptr(), flags, mode as libc::c_uint) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat returned a new descriptor we own.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

pub fn fstat(fd: &OwnedFd) -> io::Result<libc::stat> {
    // SAFETY: a zeroed stat is a valid output buffer.
    let mut meta: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: a valid fd and buffer.
    if unsafe { libc::fstat(fd.as_raw_fd(), &mut meta) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(meta)
}

pub fn stat_at(dir: &OwnedFd, name: &str) -> io::Result<libc::stat> {
    let c = CString::new(name).map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?;
    // SAFETY: a zeroed stat is a valid output buffer.
    let mut meta: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: a valid fd, name and buffer.
    if unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            c.as_ptr(),
            &mut meta,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(meta)
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

/// `save_to(ctx, destination, topic, progress)`: build a report and save it onto Internal Storage or
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
    let value = state::state(root);
    let now = clock::Instant::now();
    let install = if matches!(value.get("enabled"), Some(Py::Bool(true))) {
        value.get("install_id").cloned().unwrap_or(Py::None)
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
    progress.update("save", 0.0, true, &[("bytes_total", Py::from(data.len()))]);
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
            progress.update("save", 1.0, true, &[("bytes_total", Py::from(data.len()))]);
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
    let mut value = state::state(root);
    value.set(
        "last",
        Py::dict()
            .with(
                "report_id",
                index.get("report_id").cloned().unwrap_or(Py::None),
            )
            .with("trigger", Py::str("manual"))
            .with("state", Py::str("saved"))
            .with("destination", Py::str(destination))
            .with("file", Py::str(file.clone()))
            .with(
                "created_utc",
                index.get("created_utc").cloned().unwrap_or(Py::None),
            )
            .with("bytes", Py::from(data.len())),
    );
    state::save(root, &value)?;
    Ok(Py::dict()
        .with("saved", Py::Bool(true))
        .with("destination", Py::str(destination))
        .with("file", Py::str(file))
        .with(
            "report_id",
            index.get("report_id").cloned().unwrap_or(Py::None),
        )
        .with("bytes", Py::from(data.len())))
}
