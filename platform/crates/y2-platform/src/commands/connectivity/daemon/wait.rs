//! What the connectivity daemon waits on: its signals (a self-pipe), the helpers' pidfds and the
//! kernel's link and address events.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::Failure;
use std::{
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
    sync::atomic::{AtomicBool, AtomicI32, Ordering},
};
use y2p_core::sys::{set_signal, Disposition, Handler};
use y2p_core::{clock, Root};

/// Readable descriptors among `fds` within `timeout` seconds (a hang-up or error reads as readable,
/// so its owner looks and closes it; EINTR waits again for the rest).
pub fn select(fds: &[RawFd], timeout: f64) -> io::Result<Vec<RawFd>> {
    let mut polled: Vec<libc::pollfd> = fds
        .iter()
        .map(|&fd| libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();
    let deadline = clock::monotonic() + timeout.max(0.0);
    loop {
        let left = (deadline - clock::monotonic()).max(0.0);
        let spec = libc::timespec {
            tv_sec: left.trunc() as libc::time_t,
            tv_nsec: (left.fract() * 1e9) as libc::c_long,
        };
        // SAFETY: `polled` is a valid array of `len` pollfd structures for the call.
        let n = unsafe {
            libc::ppoll(
                polled.as_mut_ptr(),
                polled.len() as libc::nfds_t,
                &spec,
                std::ptr::null(),
            )
        };
        if n >= 0 {
            break;
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINTR) {
            return Err(error);
        }
    }
    Ok(polled
        .iter()
        .filter(|p| p.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0)
        .map(|p| p.fd)
        .collect())
}

// ---- signals ------------------------------------------------------------------------------------

pub(super) static RUNNING: AtomicBool = AtomicBool::new(true);
static WAKE_FD: AtomicI32 = AtomicI32::new(-1);

extern "C" fn on_signal(signal: libc::c_int) {
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

/// `service.SignalWake`: SIGTERM and SIGINT end the loop, SIGUSR1 only wakes it; each writes to a
/// non-blocking pipe the loop selects on. The previous handlers come back on drop.
pub(super) struct SignalWake {
    pub(super) read: OwnedFd,
    write: OwnedFd,
    previous: Vec<(libc::c_int, Disposition)>,
}

impl SignalWake {
    pub(super) fn new() -> Result<Self, Failure> {
        let mut fds = [0; 2];
        // SAFETY: pipe2 fills two descriptors this struct then owns.
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) } != 0 {
            return Err(Failure::from(io::Error::last_os_error()));
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
            write,
            previous,
        })
    }

    pub(super) fn drain(&self) {
        let mut buffer = [0u8; 4096];
        // SAFETY: reading into a local buffer from our own non-blocking pipe.
        unsafe {
            libc::read(
                self.read.as_raw_fd(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
            )
        };
    }
}

impl Drop for SignalWake {
    fn drop(&mut self) {
        for (signal, old) in &self.previous {
            old.restore(*signal);
        }
        WAKE_FD.store(-1, Ordering::SeqCst);
        let _ = &self.write;
    }
}

// ---- netlink ------------------------------------------------------------------------------------

/// `ssh.AddressEvents`: passive link/IPv4-address events (RTMGRP_LINK | RTMGRP_IPV4_IFADDR), never
/// parsed. `Fixture` is a socket that never becomes readable.
pub(super) enum Events {
    Real(OwnedFd),
    Fixture,
    Closed,
}

impl Events {
    pub(super) fn open(root: &Root) -> Self {
        if !root.is_real() {
            return Events::Fixture;
        }
        // SAFETY: socket/bind with a zeroed sockaddr_nl; the descriptor is owned on success.
        unsafe {
            let fd = libc::socket(
                libc::AF_NETLINK,
                libc::SOCK_RAW | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                libc::NETLINK_ROUTE,
            );
            if fd < 0 {
                return Events::Closed;
            }
            let fd = OwnedFd::from_raw_fd(fd);
            let mut address: libc::sockaddr_nl = std::mem::zeroed();
            address.nl_family = libc::AF_NETLINK as libc::sa_family_t;
            address.nl_groups = 1 | 0x10;
            if libc::bind(
                fd.as_raw_fd(),
                (&address as *const libc::sockaddr_nl).cast(),
                std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
            ) != 0
            {
                return Events::Closed;
            }
            Events::Real(fd)
        }
    }

    pub(super) fn present(&self) -> bool {
        !matches!(self, Events::Closed)
    }

    pub(super) fn fd(&self) -> Option<RawFd> {
        match self {
            Events::Real(fd) => Some(fd.as_raw_fd()),
            _ => None,
        }
    }

    /// `AddressEvents.wait(timeout, wake_fd)`: drain the wake pipe and a bounded burst of events;
    /// whether a link or address changed.
    pub(super) fn wait(&mut self, timeout: f64, wake: RawFd) -> Result<bool, Failure> {
        let fds: Vec<RawFd> = std::iter::once(wake).chain(self.fd()).collect();
        let ready = select(&fds, timeout)?;
        if ready.contains(&wake) {
            let mut buffer = [0u8; 4096];
            // SAFETY: reads into a local buffer from our own non-blocking pipe until it is empty.
            while unsafe { libc::read(wake, buffer.as_mut_ptr().cast(), buffer.len()) } > 0 {}
        }
        let Some(fd) = self.fd().filter(|fd| ready.contains(fd)) else {
            return Ok(false);
        };
        // Drain a bounded burst. Remaining events stay readable next time. Nothing reads the
        // messages: each recv consumes one whole message, a longer one is cut (MSG_TRUNC).
        let mut buffer = [0u8; 4096];
        for _ in 0..32 {
            // SAFETY: receiving into a local buffer from our own non-blocking socket.
            let n = unsafe {
                libc::recv(
                    fd,
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                    libc::MSG_TRUNC,
                )
            };
            if n < 0 {
                if io::Error::last_os_error().kind() != io::ErrorKind::WouldBlock {
                    *self = Events::Closed; // refresh on the bounded fallback cadence
                }
                break;
            }
        }
        Ok(true)
    }
}

pub(super) fn pidfd_open(pid: i32) -> Option<OwnedFd> {
    // SAFETY: pidfd_open(2) returns a new descriptor this function then owns.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    (fd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(fd as RawFd) })
}
