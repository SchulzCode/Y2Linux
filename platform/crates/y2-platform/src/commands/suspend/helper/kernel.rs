//! What the sleep helper asks of the kernel (`Pm`) and the player's own (`Kernel`): the bounded
//! `wakeup_count` read, each PM control write as one unbuffered `write(2)` (`wakeup_count`, backstop
//! `30`, `mem`, backstop `0`, the stage marks) and the pauses between steps.
// SPDX-License-Identifier: GPL-2.0-only
use super::{check, install, E};
use std::{
    fs::{File, OpenOptions},
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::OpenOptionsExt,
    },
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use y2p_core::Root;

// glibc's interval timers (not in the libc crate for every target).
unsafe extern "C" {
    fn getitimer(which: libc::c_int, value: *mut libc::itimerval) -> libc::c_int;
    fn setitimer(
        which: libc::c_int,
        value: *const libc::itimerval,
        old: *mut libc::itimerval,
    ) -> libc::c_int;
}

static ALARM: AtomicBool = AtomicBool::new(false);

extern "C" fn alarm(_: libc::c_int) {
    ALARM.store(true, Ordering::SeqCst);
}

/// What the helper asks of the kernel, passed in so that a test can stand in for it: the bounded
/// `wakeup_count` read, the PM control writes (`wakeup_count`, the backstop, `mem`, the stage
/// marks) and a pause between steps.
pub trait Pm {
    /// `/sys/power/wakeup_count`: `None` when it cannot be read; refused while a wake source keeps
    /// the read from finishing.
    fn wakeup_count(&mut self, root: &Root) -> Result<Option<String>, E>;
    /// One PM control write of `value` to the device path `name` (`control_write`).
    fn control(&mut self, root: &Root, name: &str, value: &str) -> Result<(), E>;
    /// Wait `seconds`; a SIGTERM/SIGINT ends the wait.
    fn pause(&mut self, seconds: f64) -> Result<(), E>;
}

/// The player's kernel: the read interrupted by ITIMER_REAL, plain `write(2)`, `nanosleep`.
pub struct Kernel;

/// One `write(2)` of `data` on the open file: the bytes written.
pub fn write(file: &File, data: &[u8]) -> io::Result<usize> {
    // SAFETY: one write of `data` to the open descriptor.
    let written = unsafe { libc::write(file.as_raw_fd(), data.as_ptr().cast(), data.len()) };
    if written < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(written as usize)
}

impl Pm for Kernel {
    fn wakeup_count(&mut self, root: &Root) -> Result<Option<String>, E> {
        wakeup_count(root)
    }

    fn control(&mut self, root: &Root, name: &str, value: &str) -> Result<(), E> {
        control_write(root, name, value, &mut write)
    }

    fn pause(&mut self, seconds: f64) -> Result<(), E> {
        let until = std::time::Instant::now() + Duration::from_secs_f64(seconds);
        loop {
            let left = until.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return check();
            }
            let request = libc::timespec {
                tv_sec: left.as_secs() as libc::time_t,
                tv_nsec: left.subsec_nanos() as libc::c_long,
            };
            // SAFETY: a valid timespec; the remainder is not needed (the deadline is recomputed).
            if unsafe { libc::nanosleep(&request, std::ptr::null_mut()) } != 0 {
                check()?;
            }
        }
    }
}

/// Issue one sysfs command; a buffered close can replay a rejected write.
///
/// Python's TextIOWrapper/BufferedWriter retry a failed flush during close. For PM that can
/// suspend after EBUSY, yet still report the original refusal to the caller. This is one
/// `write(2)` on an unbuffered descriptor. A short write is also a failure (EIO), never a reason to
/// issue the remaining command again.
pub fn control_write(
    root: &Root,
    name: &str,
    value: &str,
    store: &mut dyn FnMut(&File, &[u8]) -> io::Result<usize>,
) -> Result<(), E> {
    let path = root.path(name);
    // open(..., 'wb', buffering=0): O_WRONLY|O_CREAT|O_TRUNC|O_CLOEXEC, 0666.
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o666)
        .custom_flags(libc::O_CLOEXEC)
        .open(&path)
        .map_err(|e| E::os(&e, Some(&path)))?;
    let data = value.as_bytes();
    loop {
        let written = match store(&file, data) {
            Ok(written) => written,
            Err(error) if error.raw_os_error() == Some(libc::EINTR) => {
                // An interrupted write is retried unless the interruption was a SIGTERM/SIGINT.
                check()?;
                continue;
            }
            Err(error) => return Err(E::os(&error, None)),
        };
        if written != data.len() {
            return Err(E::Os {
                class: "OSError",
                text: format!("[Errno 5] short PM control write: {name}"),
                errno: Some(libc::EIO),
            });
        }
        return Ok(());
    }
}

/// The kernel entry: the backstop armed immediately before the blocking `mem` write and cleared
/// immediately after it (Python's `try: enter(ctx) finally: backstop 0`).
pub(super) fn enter(root: &Root, pm: &mut dyn Pm) -> Result<(), E> {
    // The already qualified one-shot RGU backstop pauses during actual SPM residence and bounds
    // stalls in entry/restoration. A reset remains a failed sleep, quarantined by durable intent.
    pm.control(root, "/sys/firmware/y2_pm/backstop_s", "30\n")?;
    let entered = kernel_sleep(root, pm);
    let cleared = pm.control(root, "/sys/firmware/y2_pm/backstop_s", "0\n");
    cleared?;
    entered
}

/// The blocking `mem` write: it returns once Linux has resumed (or refused).
pub(super) fn kernel_sleep(root: &Root, pm: &mut dyn Pm) -> Result<(), E> {
    let result = pm.control(root, "/sys/power/state", "mem\n");
    if result.is_ok() {
        check()?;
    }
    result
}

/// `wakeup_count(ctx, timeout=2)`: sysfs `wakeup_count_show` calls `pm_get_wakeup_count(...,
/// true)`, so even O_NONBLOCK cannot bound an active wake source. Interrupt that read (SIGALRM)
/// rather than leaving the product helper stuck before it reaches kernel PM.
fn wakeup_count(root: &Root) -> Result<Option<String>, E> {
    let mut running: libc::itimerval = unsafe { std::mem::zeroed() };
    // SAFETY: getitimer into a zeroed itimerval.
    unsafe { getitimer(libc::ITIMER_REAL, &mut running) };
    if running.it_value.tv_sec != 0 || running.it_value.tv_usec != 0 {
        return Err(E::refused("sleep_deadline_in_progress"));
    }
    ALARM.store(false, Ordering::SeqCst);
    let old = install(libc::SIGALRM, alarm);
    let timer = |seconds: libc::time_t| {
        let value = libc::itimerval {
            it_interval: libc::timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            it_value: libc::timeval {
                tv_sec: seconds,
                tv_usec: 0,
            },
        };
        // SAFETY: a valid itimerval.
        unsafe { setitimer(libc::ITIMER_REAL, &value, std::ptr::null_mut()) };
    };
    timer(2);
    let result = read_count(root);
    timer(0);
    if let Some(old) = old {
        old.restore(libc::SIGALRM);
    }
    result
}

/// `ctx.read('/sys/power/wakeup_count')` in-process, with the alarm's raise.
fn read_count(root: &Root) -> Result<Option<String>, E> {
    let raised = || -> Result<(), E> {
        if ALARM.swap(false, Ordering::SeqCst) {
            return Err(E::refused("wakeup_count_busy"));
        }
        check()
    };
    let path = root.path("/sys/power/wakeup_count");
    let Ok(file) = File::open(&path) else {
        raised()?;
        return Ok(None);
    };
    let limit = y2p_core::root::READ_LIMIT;
    let cap = (limit + 1) * 4;
    let mut bytes = Vec::new();
    let mut block = [0u8; 8192];
    // SAFETY: the descriptor stays owned by `file`; ManuallyDrop avoids a second close.
    let mut reader = std::mem::ManuallyDrop::new(unsafe { File::from_raw_fd(file.as_raw_fd()) });
    while bytes.len() < cap {
        match reader.read(&mut block) {
            Ok(0) => break,
            Ok(n) => bytes.extend_from_slice(&block[..n.min(cap - bytes.len())]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => raised()?,
            Err(_) => {
                raised()?;
                return Ok(None);
            }
        }
    }
    drop(file);
    raised()?;
    let text = String::from_utf8_lossy(&bytes);
    if text.chars().count() > limit {
        return Ok(None);
    }
    Ok(Some(y2p_core::root::python_strip(&text).to_owned()))
}
