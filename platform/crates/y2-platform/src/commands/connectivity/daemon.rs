//! `y2-platform connectivity-daemon`: monitor the boot-started radio helpers
//! without shell polling or reconnect ownership. `usr/libexec/y2/connectivity` starts calibration,
//! wpa_supplicant, BlueALSA and y2-bt-reconnect and then `exec`s this daemon (so
//! `/run/y2/connectivity.pid` names it); it restarts a dead helper, attaches wpa_supplicant to wlan0
//! with the wpa-event action hook, re-adds a stale interface (F53) and stops every helper when it ends.
//! While healthy it starts no process: helper death is a pidfd event, link changes a netlink event.
//!
//! Below a fixture root the loop does not wait, sees no netlink events (a socket that never becomes
//! readable) and ends after `Y2_DAEMON_TICKS` iterations (tests/platform_cases/connectivity.py).
// SPDX-License-Identifier: GPL-2.0-only
mod supervisor;
mod wait;

use crate::reply::{private_directory, Failure, Outcome};
use std::{
    env, io,
    os::fd::{AsRawFd, OwnedFd, RawFd},
    sync::atomic::Ordering,
};
pub use supervisor::Supervisor;
pub use wait::select;
use wait::{os, pidfd_open, Events, SignalWake, RUNNING};
use y2p_core::Root;

/// One run of the loop until SIGTERM/SIGINT (or the fixture's tick count).
fn monitor(
    root: &Root,
    supervisor: &mut Supervisor,
    pidfds: &mut Vec<(&'static str, i32, OwnedFd)>,
    ticks: Option<u64>,
) -> Result<(), Failure> {
    let wake = SignalWake::new()?;
    let mut events = Events::open(root);
    let (mut changed, mut dead) = (true, true);
    let mut done = 0u64;
    while RUNNING.load(Ordering::SeqCst) {
        wake.drain();
        let now = root.clock().monotonic();
        supervisor.health(now, dead)?;
        supervisor.verify(now);
        if changed || !supervisor.attached || !events.present() {
            supervisor.interface(now);
        }
        // pidfds make helper death an event. Older hosts retain the existing 30 s health cadence;
        // healthy checks use native reads, no children.
        pidfds.retain(|(name, pid, _)| supervisor.pids.iter().any(|(n, p)| n == name && p == pid));
        for &(name, pid) in &supervisor.pids {
            if !pidfds.iter().any(|(n, _, _)| *n == name) {
                if let Some(fd) = pidfd_open(pid) {
                    pidfds.push((name, pid, fd));
                }
            }
        }
        let mut timeout = (supervisor.next_health - root.clock().monotonic()).max(0.0);
        if !supervisor.attached && root.path("/sys/class/net/wlan0").is_dir() {
            timeout = timeout.min((supervisor.retry_at - root.clock().monotonic()).max(0.05));
        }
        if ticks.is_some() {
            timeout = 0.0;
        }
        let readers: Vec<RawFd> = std::iter::once(wake.read.as_raw_fd())
            .chain(pidfds.iter().map(|(_, _, fd)| fd.as_raw_fd()))
            .chain(events.fd())
            .collect();
        let ready = select(&readers, timeout).map_err(os)?;
        changed = events.wait(0.0, wake.read.as_raw_fd())?;
        dead = pidfds
            .iter()
            .any(|(_, _, fd)| ready.contains(&fd.as_raw_fd()));
        if dead {
            // A failed restart must not leave an exited pidfd spinning.
            let mut index = 0;
            while index < pidfds.len() {
                if ready.contains(&pidfds[index].2.as_raw_fd()) {
                    let (name, _, _) = pidfds.remove(index);
                    supervisor.pids.retain(|(n, _)| *n != name);
                } else {
                    index += 1;
                }
            }
        }
        done += 1;
        if ticks.is_some_and(|n| done >= n) {
            break;
        }
    }
    drop(events);
    drop(wake);
    Ok(())
}

/// `y2-platform connectivity-daemon`: prints nothing and exits 0 when stopped; a second instance is
/// refused (BlockingIOError) before it touches any helper.
pub fn serve(root: &Root) -> Outcome {
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    let path = runtime.join("connectivity-monitor.lock");
    let lock = y2p_core::lock::try_exclusive(&path)
        .map_err(|e| Failure::os(e, &path))?
        .ok_or_else(|| os(io::Error::from_raw_os_error(libc::EWOULDBLOCK)))?;
    let ticks = if root.is_real() {
        None
    } else {
        env::var("Y2_DAEMON_TICKS")
            .ok()
            .and_then(|t| t.parse().ok())
    };
    let mut supervisor = Supervisor::new(root);
    let mut pidfds = Vec::new();
    let result = monitor(root, &mut supervisor, &mut pidfds, ticks);
    drop(pidfds);
    supervisor.cleanup();
    drop(lock);
    match result {
        // A daemon prints nothing (main.rs would print a newline for a reply).
        Ok(()) => std::process::exit(0),
        // A failed wait or a runtime file it cannot write: the error record, exit 1.
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn a_second_instance_is_refused() {
        let dir = std::env::temp_dir().join(format!("y2-conn-{}", std::process::id()));
        fs::create_dir_all(dir.join("run/y2")).unwrap();
        let held =
            y2p_core::lock::exclusive(&dir.join("run/y2/connectivity-monitor.lock")).unwrap();
        let error = serve(&Root::new(&dir)).err().expect("refused");
        assert_eq!(
            error.message(),
            "[Errno 11] Resource temporarily unavailable"
        );
        assert!(matches!(
            error,
            Failure::Os {
                class: "BlockingIOError",
                ..
            }
        ));
        drop(held);
        fs::remove_dir_all(dir).unwrap();
    }
}
