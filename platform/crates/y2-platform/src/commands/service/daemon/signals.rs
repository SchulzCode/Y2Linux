//! SIGTERM, SIGINT and SIGUSR1 as a readable pipe, so that the loop waits on them and on the
//! supplicant's events in one place.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;

pub(super) static RUNNING: AtomicBool = AtomicBool::new(true);
pub(super) static WAKE_FD: AtomicI32 = AtomicI32::new(-1);

pub(super) extern "C" fn on_signal(signal: libc::c_int) {
    if signal != libc::SIGUSR1 {
        RUNNING.store(false, Ordering::SeqCst);
    }
    let fd = WAKE_FD.load(Ordering::SeqCst);
    if fd >= 0 {
        let byte = signal as u8;
        // SAFETY: write(2) is async-signal-safe; a full pipe drops the byte (warn_on_full_buffer=False).
        unsafe { libc::write(fd, (&byte as *const u8).cast(), 1) };
    }
}

pub(super) fn os(error: io::Error) -> Failure {
    let (text, class) = py::os_error(&error, None);
    Failure::Os { text, class }
}

/// Wake the same select as supplicant events; retain signals until drained.
/// SIGTERM and SIGINT end the loop, SIGUSR1 (usb.wake_service: re-read the media views now) only wakes
/// it. The previous handlers come back on drop.
pub(super) struct SignalWake {
    read: OwnedFd,
    _write: OwnedFd,
    previous: Vec<(libc::c_int, Disposition)>,
}

impl SignalWake {
    pub(super) fn new() -> Result<Self, Failure> {
        let mut fds = [0; 2];
        // SAFETY: pipe2 fills two descriptors this struct then owns.
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) } != 0 {
            return Err(os(io::Error::last_os_error()));
        }
        // SAFETY: both descriptors were just created and are owned once.
        let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        WAKE_FD.store(write.as_raw_fd(), Ordering::SeqCst);
        let mut previous = Vec::new();
        for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGUSR1] {
            // SAFETY: a plain handler that only touches atomics and write(2); the old action is kept.
            if let Ok(old) =
                unsafe { set_signal(signal, Handler::Call(on_signal), libc::SA_RESTART) }
            {
                previous.push((signal, old));
            }
        }
        Ok(Self {
            read,
            _write: write,
            previous,
        })
    }

    pub(super) fn fd(&self) -> RawFd {
        self.read.as_raw_fd()
    }

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
        WAKE_FD.store(-1, Ordering::SeqCst);
    }
}
