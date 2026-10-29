//! The activity lease and QoS hint a long transfer holds.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::{private_directory, Failure};
use std::{
    io::Write,
    os::unix::fs::OpenOptionsExt,
    sync::{mpsc, Arc},
    thread,
    time::Duration,
};
use y2p_core::Root;

/// `@workload(class_name)`: a shared activity lease for the whole call (sleep takes it exclusively;
/// unlike optional frequency hints this is a mandatory operation exclusion, and flock dies with the
/// process), plus the optional `/dev/y2-workload` QoS hint renewed every second on its own thread,
/// even while a blocking transfer runs for minutes. A missing or failing hint never fails the work.
pub(super) fn workload<T>(
    root: &Root,
    class: &str,
    work: impl FnOnce() -> Result<T, Failure>,
) -> Result<T, Failure> {
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    let path = runtime.join("activity.lock");
    let _lease = y2p_core::lock::try_shared(&path)
        .map_err(|e| Failure::os(e, &path))?
        .ok_or_else(|| Failure::value("sleep_operation_in_progress"))?;
    let payload = format!("{class} 3000\n").into_bytes();
    let mut renewal = None;
    let mut hint = None;
    if let Ok(file) = std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(root.path("/dev/y2-workload"))
    {
        let file = Arc::new(file);
        if matches!((&*file).write(&payload), Ok(n) if n == payload.len()) {
            let (stop, stopped) = mpsc::channel::<()>();
            let (writer, payload) = (Arc::clone(&file), payload.clone());
            let worker = thread::spawn(move || loop {
                if !matches!(stopped.try_recv(), Err(mpsc::TryRecvError::Empty)) {
                    return;
                }
                if !matches!((&*writer).write(&payload), Ok(n) if n == payload.len()) {
                    return;
                }
                if !matches!(
                    stopped.recv_timeout(Duration::from_secs(1)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    return;
                }
            });
            renewal = Some((stop, worker));
        }
        hint = Some(file);
    }
    let result = work();
    if let Some((stop, worker)) = renewal {
        drop(stop);
        let _ = worker.join();
    }
    drop(hint); // close/process death releases only this lease
    result
}
