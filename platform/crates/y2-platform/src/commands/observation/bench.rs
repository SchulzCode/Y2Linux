//! `bench.py`: scratch-only storage measurements, descriptor-pinned across media removal
//! (`bench-storage`, `bench-library`).
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    snapshot::{device_instance, storage, storage_mode},
    util::{self, read, split_ws, splitlines},
};
use crate::{
    regex,
    reply::{Failure, Outcome, Reply},
};
use serde_json::{json, Map, Value};
use std::os::fd::BorrowedFd;
use std::{
    collections::BTreeSet,
    ffi::CString,
    fs::File,
    io,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
        unix::fs::OpenOptionsExt,
    },
};
use y2p_core::{
    observation_support::{
        run_passing_fd,
        sha2::{Digest, Sha256},
        PyRandom,
    },
    py, Root,
};

/// An exception the benchmarks catch: its class, `str()` and `errno`.
#[derive(Debug)]
struct Error {
    class: String,
    text: String,
    errno: Option<i32>,
}

impl Error {
    fn value(text: &str) -> Error {
        Error {
            class: "ValueError".into(),
            text: text.into(),
            errno: None,
        }
    }

    /// `OSError(errno, text)`.
    fn raised(errno: i32, text: &str) -> Error {
        Error {
            class: py::os_error_class(errno).into(),
            text: format!("[Errno {errno}] {text}"),
            errno: Some(errno),
        }
    }

    /// An OSError from a call (with the name Python passed it, if any).
    fn os(error: io::Error, name: Option<&str>) -> Error {
        let (text, class) = py::os_error(&error, name);
        Error {
            class: class.into(),
            text,
            errno: error.raw_os_error(),
        }
    }
}

type Result<T> = std::result::Result<T, Error>;

fn check(result: libc::c_int) -> io::Result<libc::c_int> {
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

fn cname(name: &str) -> CString {
    CString::new(name).expect("scratch names have no NUL")
}

/// `os.open(name, flags, mode, dir_fd=directory)`.
fn openat(directory: RawFd, name: &str, flags: i32, mode: libc::mode_t) -> Result<File> {
    let path = cname(name);
    // SAFETY: a NUL-terminated name relative to a descriptor this process holds.
    let fd = check(unsafe {
        libc::openat(
            directory,
            path.as_ptr(),
            flags | libc::O_CLOEXEC,
            libc::c_uint::from(mode),
        )
    })
    .map_err(|e| Error::os(e, Some(name)))?;
    // SAFETY: a new descriptor this process owns.
    Ok(File::from(unsafe { OwnedFd::from_raw_fd(fd) }))
}

/// A descriptor of this run (the volume, its run directory, a file) for one call.
fn borrowed(fd: RawFd) -> BorrowedFd<'static> {
    // SAFETY: every descriptor a measurement passes stays open until the measurement ends, longer
    // than any one call it is borrowed for.
    unsafe { BorrowedFd::borrow_raw(fd) }
}

fn fstat(fd: RawFd) -> Result<y2p_core::sys::Stat> {
    y2p_core::sys::fstat(borrowed(fd)).map_err(|e| Error::os(e, None))
}

fn fstatvfs(fd: RawFd) -> Result<y2p_core::sys::Space> {
    y2p_core::sys::fstatvfs(borrowed(fd)).map_err(|e| Error::os(e, None))
}

fn sync(fd: RawFd, data_only: bool) -> Result<()> {
    // SAFETY: a descriptor this process holds.
    let result = unsafe {
        if data_only {
            libc::fdatasync(fd)
        } else {
            libc::fsync(fd)
        }
    };
    check(result).map(drop).map_err(|e| Error::os(e, None))
}

/// What a measurement does to its scratch file and reads around each step: the kernel's calls
/// (`Kernel`), or a test's that brings the faults a fixture cannot cause (a failing write, aliased
/// addresses, refused cache advice, controller counters that move).
pub(super) trait Io {
    fn pwrite(&self, fd: BorrowedFd, data: &[u8], offset: u64) -> io::Result<usize> {
        y2p_core::sys::pwrite(fd, data, offset)
    }

    fn pread(&self, fd: BorrowedFd, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
        y2p_core::sys::pread(fd, buffer, offset)
    }

    /// `posix_fadvise(fd, 0, 0, POSIX_FADV_DONTNEED)`: 0 or the errno.
    fn drop_cache(&self, fd: RawFd) -> i32 {
        // SAFETY: an open descriptor; the whole file.
        unsafe { libc::posix_fadvise(fd, 0, 0, libc::POSIX_FADV_DONTNEED) }
    }

    /// The passive counters a measurement is compared by.
    fn counters(&self, root: &Root) -> Value {
        io_snapshot(root)
    }
}

/// The kernel's calls and counters.
pub(super) struct Kernel;

impl Io for Kernel {}

/// `bench.write_all`.
fn write_all(io: &dyn Io, fd: RawFd, data: &[u8], offset: usize) -> Result<()> {
    let mut written = 0;
    while written < data.len() {
        let n = io
            .pwrite(borrowed(fd), &data[written..], (offset + written) as u64)
            .map_err(|e| Error::os(e, None))?;
        if n == 0 {
            return Err(Error::raised(libc::EIO, "short_write"));
        }
        written += n;
    }
    Ok(())
}

/// `os.pread(fd, size, offset)`: one call.
fn pread(io: &dyn Io, fd: RawFd, size: usize, offset: usize) -> Result<Vec<u8>> {
    let mut buffer = vec![0u8; size];
    let n = io
        .pread(borrowed(fd), &mut buffer, offset as u64)
        .map_err(|e| Error::os(e, None))?;
    buffer.truncate(n);
    Ok(buffer)
}

/// The mount a scratch directory lives on: boot, mount, device and block instance.
#[derive(Clone, PartialEq)]
struct Identity(Value, Value, String, Option<String>);

/// `bench.Scratch`: never resolves workload names against a replaceable mountpoint path.
struct Scratch<'a> {
    root: &'a Root,
    volume: String,
    volume_fd: Option<File>,
    base_fd: Option<File>,
    fd: Option<File>,
    name: String,
    files: BTreeSet<String>,
    child_outputs: bool,
    initial: Identity,
    initial_uuid: Value,
}

impl<'a> Scratch<'a> {
    fn new(root: &'a Root, volume: &str, budget: u64) -> Result<Scratch<'a>> {
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

    fn ensure(&self) -> Result<()> {
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

    fn fd(&self) -> RawFd {
        self.fd.as_ref().expect("scratch").as_raw_fd()
    }

    fn alnum(name: &str) -> bool {
        !name.is_empty() && name.chars().all(char::is_alphanumeric)
    }

    fn open(&mut self, name: &str) -> Result<File> {
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

    fn rename(&mut self, source: &str, target: &str) -> Result<()> {
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

    fn unlink(&mut self, name: &str) -> Result<()> {
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
    fn close(&mut self) {
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
fn observe_storage(root: &Root) -> Result<Value> {
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

/// `bench.distribution`.
fn distribution(samples: &[u64], bytes: u64, duration: Option<u64>) -> Map<String, Value> {
    let mut result = Map::new();
    if samples.is_empty() {
        for (k, v) in [
            ("samples", json!(0)),
            ("p50_ms", Value::Null),
            ("p95_ms", Value::Null),
            ("p99_ms", Value::Null),
            ("max_ms", Value::Null),
            ("MB_per_second", Value::Null),
            ("operations_per_second", Value::Null),
        ] {
            result.insert(k.into(), v);
        }
        return result;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let elapsed = duration.unwrap_or_else(|| sorted.iter().sum());
    let percentile = |p: f64| {
        let index = ((sorted.len() as f64 * p).ceil() as i64 - 1).max(0) as usize;
        sorted[index] as f64 / 1e6
    };
    result.insert("samples".into(), json!(sorted.len()));
    result.insert("p50_ms".into(), json!(percentile(0.5)));
    result.insert("p95_ms".into(), json!(percentile(0.95)));
    result.insert("p99_ms".into(), json!(percentile(0.99)));
    result.insert(
        "max_ms".into(),
        json!(*sorted.last().expect("samples") as f64 / 1e6),
    );
    let rate = |numerator: f64| {
        if elapsed != 0 {
            json!(numerator / elapsed as f64)
        } else {
            Value::Null
        }
    };
    result.insert("MB_per_second".into(), rate((bytes * 1000) as f64));
    result.insert(
        "operations_per_second".into(),
        rate(sorted.len() as f64 * 1e9),
    );
    result.insert("bytes".into(), json!(bytes));
    result.insert("elapsed_ms".into(), json!(elapsed as f64 / 1e6));
    result
}

/// `bench.io_snapshot`: passive counters only; never resume MMC just to observe an idle gate.
fn io_snapshot(root: &Root) -> Value {
    let stat = root.read("/proc/stat").unwrap_or_default();
    let ticks = regex!(r"(?m)^cpu\s+([0-9 ]+)$").captures(&stat).map(|c| {
        split_ws(&c[1])
            .iter()
            .filter_map(|v| v.parse::<i64>().ok())
            .take(8)
            .collect::<Vec<_>>()
    });
    let mut irqs = Map::new();
    for line in splitlines(&root.read("/proc/interrupts").unwrap_or_default()) {
        let Some((key, fields)) = line.split_once(':') else {
            continue;
        };
        let mut values = Vec::new();
        for field in split_ws(fields) {
            if field.is_empty() || !field.chars().all(|c| c.is_ascii_digit()) {
                break;
            }
            values.push(field.parse::<i64>().unwrap_or(0));
        }
        if !values.is_empty() {
            irqs.insert(
                y2p_core::root::python_strip(key).to_owned(),
                json!(values.iter().sum::<i64>()),
            );
        }
    }
    let mut controllers = Map::new();
    for path in root.glob("/sys/bus/platform/devices/*.mmc/y2_storage") {
        controllers.insert(
            util::parent_name(&path),
            storage_mode(read(&path).as_deref()),
        );
    }
    json!({"cpu_ticks": ticks, "irqs": if irqs.is_empty() { Value::Null } else { Value::Object(irqs) },
           "controllers": controllers})
}

/// `bench.io_delta`: unavailable/reset counters are explicit, not false zero-error passes.
fn io_delta(before: &Value, after: &Value) -> Value {
    let mut busy = Value::Null;
    if let (Some(a), Some(b)) = (
        before["cpu_ticks"].as_array(),
        after["cpu_ticks"].as_array(),
    ) {
        let (a, b): (Vec<i64>, Vec<i64>) = (
            a.iter().filter_map(Value::as_i64).collect(),
            b.iter().filter_map(Value::as_i64).collect(),
        );
        if !a.is_empty()
            && !b.is_empty()
            && a.len() == b.len()
            && a.iter().zip(&b).all(|(x, y)| y >= x)
        {
            let delta: Vec<i64> = a.iter().zip(&b).map(|(x, y)| y - x).collect();
            let total: i64 = delta.iter().sum();
            if total != 0 && delta.len() >= 5 {
                busy = json!((100 * (total - delta[3] - delta[4])) as f64 / total as f64);
            }
        }
    }
    let irq = match (before["irqs"].as_object(), after["irqs"].as_object()) {
        (Some(a), Some(b)) => Value::Object(
            a.iter()
                .map(|(k, v)| {
                    let (v, now) = (v.as_i64().unwrap_or(0), b.get(k).and_then(Value::as_i64));
                    (
                        k.clone(),
                        now.filter(|n| *n >= v)
                            .map_or(Value::Null, |n| json!(n - v)),
                    )
                })
                .collect(),
        ),
        _ => Value::Null,
    };
    let counters = [
        "crc_events",
        "timeout_events",
        "controller_events",
        "tuning_events",
        "voltage_events",
        "verify_events",
        "fallbacks",
        "resets",
    ];
    let empty = Map::new();
    let names: BTreeSet<&String> = before["controllers"]
        .as_object()
        .unwrap_or(&empty)
        .keys()
        .chain(after["controllers"].as_object().unwrap_or(&empty).keys())
        .collect();
    let mut controllers = Map::new();
    for name in names {
        let pick = |side: &Value| match &side["controllers"][name.as_str()] {
            Value::Object(map) => map.clone(),
            _ => Map::new(),
        };
        let (a, b) = (pick(before), pick(after));
        let delta: Map<String, Value> = counters
            .iter()
            .map(|k| {
                let value = match (
                    util::is_int(a.get(*k)).filter(|_| a[*k].is_number()),
                    util::is_int(b.get(*k)).filter(|_| b[*k].is_number()),
                ) {
                    (Some(x), Some(y)) if y >= x => json!((y - x) as i64),
                    _ => Value::Null,
                };
                (k.to_string(), value)
            })
            .collect();
        controllers.insert(
            name.clone(),
            json!({"before": a, "after": b, "delta": delta}),
        );
    }
    json!({"cpu_busy_percent": busy, "irq_delta": irq, "controllers": controllers})
}

fn controller_fault(system: &Value) -> bool {
    system["controllers"].as_object().is_some_and(|c| {
        c.values().any(|c| {
            c["delta"]
                .as_object()
                .is_some_and(|d| d.values().any(|v| v.as_i64().is_some_and(|v| v > 0)))
        })
    })
}

/// What runs once after a measurement's steps (an fdatasync, a directory fsync).
type Finalize<'f> = &'f dyn Fn(&Scratch) -> Result<()>;

struct Bench<'a> {
    root: &'a Root,
    io: &'a dyn Io,
    deadline: f64,
    result: Map<String, Value>,
}

impl Bench<'_> {
    fn ensure(&self, scratch: &Scratch) -> Result<()> {
        if self.root.clock().monotonic() >= self.deadline {
            return Err(Error {
                class: "TimeoutError".into(),
                text: "benchmark_deadline".into(),
                errno: None,
            });
        }
        scratch.ensure()
    }

    fn list(&mut self, key: &str) -> &mut Vec<Value> {
        self.result
            .get_mut(key)
            .and_then(Value::as_array_mut)
            .expect("list")
    }

    /// `advise_read`: older Y2 kernels omit ADVISE_SYSCALLS (EINVAL on ARM). Preserve verified
    /// readback while explicitly refusing a cold or media-throughput claim. Genuine I/O faults fail.
    fn advise(&mut self, fd: RawFd, operation: &str) -> Result<()> {
        let error = self.io.drop_cache(fd);
        let (state, errno) = match error {
            0 => ("requested_advisory_only", Value::Null),
            libc::EINVAL | libc::ENOSYS | libc::EOPNOTSUPP => ("unavailable", json!(error)),
            other => return Err(Error::os(io::Error::from_raw_os_error(other), None)),
        };
        self.list("cache_advice")
            .push(json!({"before": operation, "state": state, "errno": errno}));
        Ok(())
    }

    fn measure(
        &mut self,
        scratch: &mut Scratch,
        name: &str,
        count: usize,
        bytes_per_op: u64,
        mut step: impl FnMut(&mut Scratch, usize) -> Result<()>,
        finalize: Option<Finalize>,
    ) -> Result<()> {
        let mut samples = Vec::with_capacity(count);
        let before_io = self.io.counters(self.root);
        let started = self.root.clock().monotonic_ns();
        for n in 0..count {
            self.ensure(scratch)?;
            let before = self.root.clock().monotonic_ns();
            step(scratch, n)?;
            samples.push(self.root.clock().monotonic_ns() - before);
        }
        if let Some(finalize) = finalize {
            finalize(scratch)?;
        }
        let elapsed = self.root.clock().monotonic_ns() - started;
        let mut measurement = Map::new();
        measurement.insert("operation".into(), json!(name));
        measurement.insert("result".into(), json!("OK"));
        measurement.extend(distribution(
            &samples,
            count as u64 * bytes_per_op,
            Some(elapsed),
        ));
        let system = io_delta(&before_io, &self.io.counters(self.root));
        let fault = controller_fault(&system);
        measurement.insert("system".into(), system);
        self.list("measurements").push(Value::Object(measurement));
        if fault {
            return Err(Error::raised(
                libc::EIO,
                "controller_fault_during_benchmark",
            ));
        }
        Ok(())
    }

    fn run(
        &mut self,
        scratch: &mut Scratch,
        size: usize,
        operations: usize,
        rng: &mut PyRandom,
        block: &[u8],
    ) -> Result<()> {
        let io = self.io;
        let data = scratch.open("data")?;
        let fd = data.as_raw_fd();
        let datasync = move |_: &Scratch| sync(fd, true);
        for bs in [4096usize, 16384, 65536, 1_048_576] {
            let count = size / bs;
            let chunk = &block[..bs];
            self.measure(
                scratch,
                &format!("sequential_write_{bs}"),
                count,
                bs as u64,
                |_, n| write_all(io, fd, chunk, n * bs),
                Some(&datasync),
            )?;
            self.advise(fd, &format!("sequential_read_{bs}"))?;
            self.measure(
                scratch,
                &format!("sequential_read_{bs}"),
                count,
                bs as u64,
                |_, n| {
                    if pread(io, fd, bs, n * bs)? != chunk {
                        Err(Error::raised(libc::EIO, "readback_mismatch"))
                    } else {
                        Ok(())
                    }
                },
                None,
            )?;
            let offsets: Vec<usize> = (0..count.min(operations))
                .map(|_| rng.randrange(count as u64) as usize * bs)
                .collect();
            self.measure(
                scratch,
                &format!("random_write_{bs}"),
                offsets.len(),
                bs as u64,
                |_, n| write_all(io, fd, chunk, offsets[n]),
                Some(&datasync),
            )?;
            self.advise(fd, &format!("random_read_{bs}"))?;
            self.measure(
                scratch,
                &format!("random_read_{bs}"),
                offsets.len(),
                bs as u64,
                |_, n| {
                    if pread(io, fd, bs, offsets[n])? != chunk {
                        Err(Error::raised(libc::EIO, "readback_mismatch"))
                    } else {
                        Ok(())
                    }
                },
                None,
            )?;
        }
        for (name, data_only) in [("fsync", false), ("fdatasync", true)] {
            self.measure(
                scratch,
                name,
                operations.min(64),
                4096,
                |_, _| {
                    write_all(io, fd, &block[..4096], 0)?;
                    sync(fd, data_only)
                },
                None,
            )?;
        }
        drop(data);
        let small = &block[..4096];
        let directory_sync = |s: &Scratch| sync(s.fd(), false);
        self.measure(
            scratch,
            "small_file_write_fsync",
            operations,
            4096,
            |s, n| {
                let file = s.open(&format!("small{n}"))?;
                write_all(io, file.as_raw_fd(), small, 0)?;
                sync(file.as_raw_fd(), false)
            },
            Some(&directory_sync),
        )?;
        for n in 0..operations {
            scratch.unlink(&format!("small{n}"))?;
        }
        self.measure(
            scratch,
            "create",
            operations,
            0,
            |s, n| s.open(&format!("f{n}")).map(drop),
            None,
        )?;
        self.measure(
            scratch,
            "stat",
            operations,
            0,
            |s, n| {
                let path = cname(&format!("f{n}"));
                y2p_core::sys::fstatat(borrowed(s.fd()), &path, libc::AT_SYMLINK_NOFOLLOW)
                    .map(drop)
                    .map_err(|e| Error::os(e, Some(&format!("f{n}"))))
            },
            None,
        )?;
        self.measure(
            scratch,
            "rename",
            operations,
            0,
            |s, n| s.rename(&format!("f{n}"), &format!("g{n}")),
            None,
        )?;
        self.measure(
            scratch,
            "delete",
            operations,
            0,
            |s, n| s.unlink(&format!("g{n}")),
            None,
        )?;
        let renamed = self.measure(
            scratch,
            "file_fsync_rename_directory_fsync",
            operations.min(64),
            4096,
            |s, _| {
                let file = s.open("renameA")?;
                write_all(io, file.as_raw_fd(), small, 0)?;
                sync(file.as_raw_fd(), false)?;
                drop(file);
                s.rename("renameA", "renameB")?;
                sync(s.fd(), false)?;
                s.unlink("renameB")?;
                sync(s.fd(), false)
            },
            None,
        );
        match renamed {
            Err(error)
                if matches!(error.errno, Some(libc::EINVAL | libc::EOPNOTSUPP))
                    && error.class != "ValueError" =>
            {
                self.list("measurements").push(json!({"operation": "file_fsync_rename_directory_fsync",
                    "result": "UNAVAILABLE", "reason": "filesystem_does_not_support_directory_fsync"}));
            }
            other => other?,
        }
        let before_io = self.io.counters(self.root);
        let integrity = self.regions(scratch)?;
        self.result
            .insert("integrity".into(), Value::Array(integrity));
        let system = io_delta(&before_io, &self.io.counters(self.root));
        let fault = controller_fault(&system);
        self.result.insert("integrity_system".into(), system);
        if fault {
            return Err(Error::raised(
                libc::EIO,
                "controller_fault_during_benchmark",
            ));
        }
        self.ensure(scratch)
    }

    /// `sha256_regions`: offset-specific patterns catch address aliasing as well as changed bytes.
    fn regions(&mut self, scratch: &mut Scratch) -> Result<Vec<Value>> {
        let file = scratch.open("integrity")?;
        let fd = file.as_raw_fd();
        let mut result = Vec::new();
        for (offset, size) in [
            (0usize, 1usize),
            (511, 513),
            (4093, 4099),
            (65543, 65537),
            (1_049_087, 1_048_583),
        ] {
            self.ensure(scratch)?;
            let payload = PyRandom::new((6582 + offset + size) as u64).randbytes(size);
            write_all(self.io, fd, &payload, offset)?;
            sync(fd, false)?;
            result.push(json!({"offset": offset, "bytes": size, "expected_sha256": hex(&payload)}));
        }
        // Verify only after every write: aliased writes must not erase another region undetected.
        // Advisory eviction is file-scoped, never global.
        for region in &mut result {
            let (offset, size) = (
                region["offset"].as_u64().expect("offset") as usize,
                region["bytes"].as_u64().expect("bytes") as usize,
            );
            self.advise(fd, &format!("sha256_{offset}_{size}"))?;
            self.ensure(scratch)?;
            let actual = hex(&pread(self.io, fd, size, offset)?);
            if actual != region["expected_sha256"] {
                return Err(Error::raised(libc::EIO, "sha256_readback_mismatch"));
            }
            region["readback_sha256"] = json!(actual);
            region["result"] = json!("OK");
        }
        Ok(result)
    }
}

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `y2-platform bench-storage`.
pub fn storage_benchmark(
    root: &Root,
    volume: &str,
    size_mib: i64,
    operations: i64,
    seconds: i64,
) -> Outcome {
    storage_benchmark_with(root, &Kernel, volume, size_mib, operations, seconds)
}

/// `storage_benchmark` with its scratch I/O and counters from `io`.
fn storage_benchmark_with(
    root: &Root,
    io: &dyn Io,
    volume: &str,
    size_mib: i64,
    operations: i64,
    seconds: i64,
) -> Outcome {
    if !(1..=128).contains(&size_mib)
        || !(1..=1024).contains(&operations)
        || !(1..=1800).contains(&seconds)
    {
        return Err(Failure::value("benchmark_bounds_exceeded"));
    }
    let size = size_mib as usize * 1024 * 1024;
    let operations = operations as usize;
    let deadline = root.clock().monotonic() + seconds as f64;
    let mut parameters = Map::new();
    parameters.insert("volume".into(), json!(volume));
    parameters.insert("size_mib".into(), json!(size_mib));
    parameters.insert("operations".into(), json!(operations));
    parameters.insert("deadline_seconds".into(), json!(seconds));
    parameters.insert("io".into(), json!("buffered"));
    parameters.insert("cold_read".into(), json!("not_guaranteed"));
    parameters.insert("seed".into(), json!(6582));
    let mut record = util::record(root, "storage", parameters);
    record.insert(
        "units".into(),
        json!({"latency": "ms", "throughput": "MB/s (decimal)", "rate": "operations/s"}),
    );
    let mut result = Map::new();
    result.insert("schema".into(), json!("org.y2linux.benchmark/v1"));
    result.insert("measurements".into(), json!([]));
    result.insert("volume".into(), Value::Null);
    result.insert("cache_advice".into(), json!([]));
    result.insert("integrity".into(), json!([]));
    let mut rng = PyRandom::new(6582);
    // A repeatable nonzero 1-MiB pattern prepared outside timed I/O.
    let block = rng.randbytes(1024 * 1024);
    let mut bench = Bench {
        root,
        io,
        deadline,
        result,
    };
    let outcome = (|| -> Result<()> {
        let mut scratch = Scratch::new(
            root,
            volume,
            (size + 3 * 1024 * 1024 + operations * 4096) as u64,
        )?;
        let initial = &scratch.initial;
        let mut described = Map::new();
        described.insert("path".into(), json!(volume));
        described.insert(
            "generation".into(),
            json!([initial.0, initial.1, initial.2, initial.3]),
        );
        described.insert("uuid".into(), scratch.initial_uuid.clone());
        described.insert("observation".into(), observe_storage(root)?);
        bench
            .result
            .insert("volume".into(), Value::Object(described));
        let ran = bench.run(&mut scratch, size, operations, &mut rng, &block);
        scratch.close();
        ran
    })();
    let mut result = bench.result;
    match outcome {
        Ok(()) => {
            record.insert("result".into(), json!("OK"));
        }
        Err(error) => {
            record.insert("result".into(), json!("FAILED"));
            record.insert(
                "failure".into(),
                json!({"type": error.class, "reason": error.text, "errno": error.errno}),
            );
        }
    }
    let failed = record["result"] == "FAILED";
    result.insert("record".into(), Value::Object(record));
    Reply::sorted(&result, u8::from(failed))
}

/// Python's `json.loads` error text for output that is not JSON (the messages a truncated or empty
/// answer gives; others are worded from serde's position).
fn json_error(text: &str, error: &serde_json::Error) -> String {
    let column = error.column();
    let line = error.line();
    let offset: usize = text
        .split('\n')
        .take(line.saturating_sub(1))
        .map(|l| l.len() + 1)
        .sum::<usize>()
        + column.saturating_sub(1);
    if error.classify() == serde_json::error::Category::Syntax
        && error.to_string().starts_with("trailing characters")
    {
        format!("Extra data: line {line} column {column} (char {offset})")
    } else if text.trim().is_empty() {
        "Expecting value: line 1 column 1 (char 0)".into()
    } else {
        format!("Expecting value: line {line} column {column} (char {offset})")
    }
}

/// `y2-platform bench-library`.
pub fn library_benchmark(root: &Root, volume: &str, tracks: i64, scan: bool) -> Outcome {
    let mut parameters = Map::new();
    parameters.insert("volume".into(), json!(volume));
    parameters.insert("tracks".into(), json!(tracks));
    parameters.insert("scan".into(), json!(scan));
    let mut record = util::record(root, "reborn-library", parameters);
    record.insert(
        "units".into(),
        json!({"latency": "ms", "memory": "KiB", "size": "bytes"}),
    );
    let mut measurements = Value::Null;
    let outcome = (|| -> Result<Value> {
        let mut scratch = Scratch::new(root, volume, (tracks as u64) * 8192 + 32 * 1024 * 1024)?;
        let vfs = fstatvfs(scratch.fd())?;
        if scan && vfs.files != 0 && (vfs.favail as i64) < tracks + 2048 {
            return Err(Error::value("insufficient_scratch_inode_reserve"));
        }
        scratch.child_outputs = true;
        let fd = scratch.fd().to_string();
        let count = tracks.to_string();
        let mut argv = vec![
            "/usr/bin/reborn-bench",
            "--scratch-fd",
            &fd,
            "--tracks",
            &count,
        ];
        if scan {
            argv.push("--scan");
        }
        let answer = run_passing_fd(root, &argv, 1800.0, 131_072, scratch.fd());
        scratch.ensure()?;
        if !answer.ok {
            return Err(Error {
                class: "RuntimeError".into(),
                text: answer.reason.unwrap_or_else(|| "None".into()),
                errno: None,
            });
        }
        let output = answer.output.unwrap_or_default();
        serde_json::from_str::<Value>(&output).map_err(|e| Error::value(&json_error(&output, &e)))
    })();
    match outcome {
        Ok(value) => {
            measurements = value;
            record.insert("result".into(), json!("OK"));
        }
        Err(error) => {
            record.insert("result".into(), json!("FAILED"));
            record.insert("failure".into(), json!(error.text));
        }
    }
    let failed = record["result"] == "FAILED";
    let result = json!({"schema": "org.y2linux.benchmark/v1", "record": record, "measurements": measurements});
    Reply::sorted(&result, u8::from(failed))
}

#[cfg(test)]
#[path = "bench_tests.rs"]
mod bench_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distributions_follow_bench() {
        let d = distribution(&[3_000_000, 1_000_000, 2_000_000], 300, Some(6_000_000));
        assert_eq!(d["p50_ms"], 2.0);
        assert_eq!(d["p95_ms"], 3.0);
        assert_eq!(d["MB_per_second"], 0.05);
        assert_eq!(d["operations_per_second"], 500.0);
        assert_eq!(distribution(&[], 0, None).len(), 7);
    }

    #[test]
    fn json_errors_read_as_python() {
        let check =
            |text: &str| json_error(text, &serde_json::from_str::<Value>(text).unwrap_err());
        assert_eq!(check(""), "Expecting value: line 1 column 1 (char 0)");
        assert_eq!(check("{\"a\":1} x"), "Extra data: line 1 column 9 (char 8)");
    }
}
