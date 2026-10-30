//! A daemon's SIGTERM, SIGINT and SIGUSR1 as a readable self-pipe (`service.SignalWake`), so its loop
//! waits on them and on its other descriptors in one place: SIGTERM and SIGINT end the loop (the
//! module's `RUNNING` turns false), SIGUSR1 only wakes it. `signal_wake!()` gives the module that
//! invokes it its own `RUNNING`, handler and `SignalWake`: each daemon's state stays its own, also in
//! the tests, which run several daemons' loops in one process.
// SPDX-License-Identifier: GPL-2.0-only

macro_rules! signal_wake {
    () => {
        pub(super) static RUNNING: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(true);
        static WAKE_FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

        pub(super) extern "C" fn on_signal(signal: libc::c_int) {
            use std::sync::atomic::Ordering;
            if signal != libc::SIGUSR1 {
                RUNNING.store(false, Ordering::SeqCst);
            }
            let fd = WAKE_FD.load(Ordering::SeqCst);
            if fd >= 0 {
                let byte = signal as u8;
                // SAFETY: write(2) is async-signal-safe; a full pipe drops the byte
                // (warn_on_full_buffer=False).
                unsafe { libc::write(fd, (&byte as *const u8).cast(), 1) };
            }
        }

        /// The self-pipe and the handlers it replaced, which come back on drop.
        pub(super) struct SignalWake {
            read: std::os::fd::OwnedFd,
            _write: std::os::fd::OwnedFd,
            previous: Vec<(libc::c_int, y2p_core::sys::Disposition)>,
        }

        impl SignalWake {
            pub(super) fn new() -> Result<Self, $crate::reply::Failure> {
                use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
                let mut fds = [0; 2];
                // SAFETY: pipe2 fills two descriptors this struct then owns.
                if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) } != 0
                {
                    return Err(std::io::Error::last_os_error().into());
                }
                // SAFETY: both descriptors were just created and are owned once.
                let (read, write) =
                    unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
                WAKE_FD.store(write.as_raw_fd(), std::sync::atomic::Ordering::SeqCst);
                let mut previous = Vec::new();
                for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGUSR1] {
                    // SAFETY: a plain handler that only touches atomics and write(2); the old
                    // action is kept.
                    let old = unsafe {
                        y2p_core::sys::set_signal(
                            signal,
                            y2p_core::sys::Handler::Call(on_signal),
                            libc::SA_RESTART,
                        )
                    };
                    if let Ok(old) = old {
                        previous.push((signal, old));
                    }
                }
                Ok(Self {
                    read,
                    _write: write,
                    previous,
                })
            }

            /// The pipe's reading end, for the loop's poll.
            pub(super) fn fd(&self) -> std::os::fd::RawFd {
                std::os::fd::AsRawFd::as_raw_fd(&self.read)
            }

            /// Empty the pipe (each signal left one byte).
            pub(super) fn drain(&self) {
                let mut buffer = [0u8; 4096];
                // SAFETY: reading into a local buffer from our own non-blocking pipe.
                unsafe { libc::read(self.fd(), buffer.as_mut_ptr().cast(), buffer.len()) };
            }
        }

        impl Drop for SignalWake {
            fn drop(&mut self) {
                for (signal, old) in &self.previous {
                    old.restore(*signal);
                }
                WAKE_FD.store(-1, std::sync::atomic::Ordering::SeqCst);
            }
        }
    };
}

pub(crate) use signal_wake;
