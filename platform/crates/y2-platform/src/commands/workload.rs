//! A workload's lease: the shared activity lease (sleep takes `/run/y2/activity.lock` exclusively, so
//! a held lease keeps the player awake; unlike optional frequency hints this is a mandatory operation
//! exclusion, and flock dies with the process) and the optional `/dev/y2-workload` QoS hint
//! (`<class> 3000`: valid for 3 s), written at once and renewed every second on its own thread, also
//! while a blocking transfer runs for minutes. A missing or failing hint never fails the work.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::At;
use crate::reply::{private_directory, Failure};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    sync::mpsc,
    thread,
    time::Duration,
};
use y2p_core::Root;

/// Held while the work runs; dropping it ends the renewal and releases the activity lease.
pub struct Lease {
    _activity: File,
    stop: Option<mpsc::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Lease {
    /// Take the lease for workload `class` (`Maintenance`, `NetworkTransfer`); refused as
    /// `sleep_operation_in_progress` while sleep holds the activity lock.
    pub fn take(root: &Root, class: &str) -> Result<Lease, Failure> {
        let runtime = root.path("/run/y2");
        private_directory(&runtime)?;
        let path = runtime.join("activity.lock");
        let activity = y2p_core::lock::try_shared(&path)
            .at(&path)?
            .ok_or_else(|| Failure::value("sleep_operation_in_progress"))?;
        let mut lease = Lease {
            _activity: activity,
            stop: None,
            worker: None,
        };
        let payload = format!("{class} 3000\n").into_bytes();
        let Ok(mut hint) = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_CLOEXEC)
            .open(root.path("/dev/y2-workload"))
        else {
            return Ok(lease);
        };
        if hint.write(&payload).ok() != Some(payload.len()) {
            return Ok(lease);
        }
        let (stop, stopped) = mpsc::channel::<()>();
        // Without a thread for the renewal the hint lapses after its 3 s; the work goes on.
        let worker = thread::Builder::new().spawn(move || loop {
            if hint.write(&payload).ok() != Some(payload.len()) {
                return;
            }
            if !matches!(
                stopped.recv_timeout(Duration::from_secs(1)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                return;
            }
        });
        if let Ok(worker) = worker {
            lease.stop = Some(stop);
            lease.worker = Some(worker);
        }
        Ok(lease)
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
