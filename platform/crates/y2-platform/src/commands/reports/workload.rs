//! A workload's lease: the shared activity lease (sleep takes `/run/y2/activity.lock`
//! exclusively, so a held lease keeps the player awake) and the optional `/dev/y2-workload` QoS hint,
//! renewed every second on its own thread while the work runs.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::{private_directory, Failure};
use std::{
    fs::File,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::Duration,
};
use y2p_core::Root;

pub struct Lease {
    _lease: File,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Lease {
    pub fn take(root: &Root, class: &str) -> Result<Lease, Failure> {
        let runtime = root.path("/run/y2");
        private_directory(&runtime)?;
        let path = runtime.join("activity.lock");
        // Unlike optional frequency hints this is a mandatory operation exclusion. flock dies with
        // the process; a leftover file is inert.
        let lease = y2p_core::lock::try_shared(&path)
            .map_err(|e| super::os(e, &path))?
            .ok_or_else(|| Failure::value("sleep_operation_in_progress"))?;
        let stop = Arc::new(AtomicBool::new(false));
        let payload = format!("{class} 3000\n").into_bytes();
        let mut worker = None;
        // QoS is optional; never fail a transfer on a hint.
        if let Ok(mut hint) = std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_CLOEXEC)
            .open(root.path("/dev/y2-workload"))
        {
            // Acquire before work starts; renew on an independent worker even when a blocking
            // transfer subprocess runs for minutes.
            if hint.write(&payload).is_ok_and(|n| n == payload.len()) {
                let stop = Arc::clone(&stop);
                worker = Some(std::thread::spawn(move || {
                    while !stop.load(Ordering::SeqCst) {
                        if !hint.write(&payload).is_ok_and(|n| n == payload.len()) {
                            return;
                        }
                        for _ in 0..20 {
                            if stop.load(Ordering::SeqCst) {
                                return;
                            }
                            std::thread::sleep(Duration::from_millis(50));
                        }
                    }
                }));
            }
        }
        Ok(Lease {
            _lease: lease,
            stop,
            worker,
        })
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
