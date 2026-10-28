//! `y2-platform sleep request` and `sleep prepare` (`sleep.request`, `sleep.prepare`): the product
//! sleep contract; kernel qualification remains an explicit owner path.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    admission::{admission, battery_and_cable},
    cli,
    receipt::{self, application_rpc, record, Stage},
    E,
};
use crate::reply::{Failure, Outcome, Reply};
use serde_json::{Map, Value};
use std::{
    fs::{File, OpenOptions},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt, unix::process::CommandExt},
    process::{Command, Stdio},
};
use y2p_core::{py::is_true, Root};

/// Python would crash in the power journal row (`battery.get` on a `battery` that is no dict) or read
/// the stored receipt differently: hand the command to Python before anything changed.
fn unusual(root: &Root, stage: &str) -> bool {
    let battery = root
        .json_object("/run/y2/power.json")
        .get("battery")
        .is_some_and(|b| !b.is_object());
    battery || !receipt::plain(&root.json_object(receipt::RECEIPT), stage)
}

/// `sleep.prepare`: Reborn must acknowledge the screen is off and it is ready to sleep.
pub fn prepare_owned(root: &Root) -> Result<Map<String, Value>, E> {
    let ack = application_rpc(root, "sleep_prepare", false).and_then(|ack| {
        if !is_true(ack.get("sleep_ready")) || !is_true(ack.get("screen_off")) {
            return Err(E::value("reborn_sleep_preparation_failed"));
        }
        Ok(ack)
    });
    if let Err(error) = ack {
        if error.is_os() || error.is_value() {
            record(
                root,
                Stage::new("refused_reborn_preparation", Some(1)).reason(error.text()),
            )?;
        }
        return Err(error);
    }
    let prepared = Stage {
        reborn_ack: Some(true),
        ..Stage::new("reborn_prepared", Some(0))
    };
    let value = record(root, prepared)?;
    Ok(value
        .get("product")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default())
}

/// `y2-platform sleep prepare` (y2-suspend's owner path): takes no lock itself.
pub fn prepare(root: &Root) -> Outcome {
    if unusual(root, "reborn_prepared") {
        return Err(Failure::Fallback);
    }
    cli(prepare_owned(root).and_then(|p| Reply::sorted(&p, 0).map_err(|_| E::value("json"))))
}

/// `y2-platform sleep request`: acknowledge an owned request; the detached helper owns the blocking
/// PM. Installed 04 lacks the built-in context-ownership ABI and stays refused.
pub fn request(root: &Root) -> Outcome {
    if unusual(root, "requested") {
        return Err(Failure::Fallback);
    }
    let directory = root.path("/run/y2");
    y2p_core::record::private_directory(&directory)
        .map_err(|e| E::os(&e, Some(&directory)))
        .map_err(|e| cli(Err(e)).err().unwrap_or(Failure::Fallback))?;
    let path = directory.join("suspend.lock");
    // os.open(O_RDWR | O_CREAT | O_NOFOLLOW, 0o600): close-on-exec, made inheritable for the helper.
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&path)
        .map_err(|e| cli(Err(E::os(&e, Some(&path)))).err().unwrap_or(Failure::Fallback))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            let current = super::super::sleep::product(
                &root.json_object(receipt::RECEIPT),
                root.read("/proc/sys/kernel/random/boot_id").as_deref(),
            )?;
            let mut busy = current;
            busy.insert("state".into(), "refused".into());
            busy.insert("reason".into(), "sleep_operation_in_progress".into());
            return Reply::sorted(&busy, 0);
        }
        Err(std::fs::TryLockError::Error(error)) => return cli(Err(E::os(&error, None))),
    }
    cli(admit(root, &lock).and_then(|p| Reply::sorted(&p, 0).map_err(|_| E::value("json"))))
}

fn admit(root: &Root, lock: &File) -> Result<Map<String, Value>, E> {
    let attempt = (|| {
        admission(root)?;
        battery_and_cable(root)?;
        let requested = Stage {
            mode: Some("product"),
            ..Stage::new("requested", Some(0))
        };
        let value = record(root, requested)?;
        launch(root, lock)?;
        Ok(value)
    })();
    let value = match attempt {
        Ok(value) => value,
        Err(error @ (E::Refused(_) | E::Os { .. })) => {
            let reason = match &error {
                E::Refused(reason) => reason.clone(),
                _ => "sleep_helper_unavailable".into(),
            };
            record(root, Stage::new(&format!("refused_{reason}"), Some(2)))?
        }
        Err(error) => return Err(error),
    };
    Ok(value
        .get("product")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default())
}

/// Start the product sleep helper, detached, with this request's lock. The inherited open file
/// description retains the request's flock across this process's exit, so another request cannot
/// pass between acknowledgement and entry.
fn launch(root: &Root, lock: &File) -> Result<(), E> {
    if !root.is_real() {
        return Err(E::refused("fixture_launcher_unavailable"));
    }
    let fd = lock.as_raw_fd();
    // SAFETY: sysconf has no preconditions.
    let open_max = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) }.clamp(256, 65_536) as i32;
    let mut command = Command::new("/usr/sbin/y2-platform");
    command
        .args(["__product-sleep", &fd.to_string()])
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: only async-signal-safe calls (setsid, fcntl, syscall) between fork and exec.
    unsafe {
        command.pre_exec(move || {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            // Python's close_fds=True with pass_fds=(fd,): nothing else is inherited.
            let ranged = libc::syscall(
                libc::SYS_close_range,
                3u32,
                u32::MAX,
                libc::CLOSE_RANGE_CLOEXEC,
            ) == 0;
            if !ranged {
                for other in 3..open_max {
                    libc::fcntl(other, libc::F_SETFD, libc::FD_CLOEXEC);
                }
            }
            if libc::fcntl(fd, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    command
        .spawn()
        .map(drop)
        .map_err(|e| E::os(&e, Some(std::path::Path::new("/usr/sbin/y2-platform"))))
}
