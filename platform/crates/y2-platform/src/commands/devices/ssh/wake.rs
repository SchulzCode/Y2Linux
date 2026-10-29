//! What wakes the ssh daemon: SIGTERM/SIGINT (which also end it) and SIGCHLD through a wakeup pipe,
//! and the kernel's link and IPv4 address events on a netlink socket.
// SPDX-License-Identifier: GPL-2.0-only
use std::{
    io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
    sync::atomic::{AtomicBool, AtomicI32, Ordering},
    time::{Duration, Instant},
};
use y2p_core::sys::{set_signal, Disposition, Handler};

// ---- signals: handlers that only note the signal and write it to a wakeup pipe -------------------

pub(super) static RUNNING: AtomicBool = AtomicBool::new(true);
static WAKE: AtomicI32 = AtomicI32::new(-1);
const SIGNALS: [libc::c_int; 3] = [libc::SIGTERM, libc::SIGINT, libc::SIGCHLD];

extern "C" fn on_signal(signal: libc::c_int) {
    // SAFETY: only async-signal-safe calls; errno is restored for the interrupted code.
    unsafe {
        let errno = *libc::__errno_location();
        if signal == libc::SIGTERM || signal == libc::SIGINT {
            RUNNING.store(false, Ordering::SeqCst);
        }
        let fd = WAKE.load(Ordering::SeqCst);
        if fd >= 0 {
            let byte = signal as u8;
            libc::write(fd, (&byte as *const u8).cast(), 1);
        }
        *libc::__errno_location() = errno;
    }
}

pub(super) struct Signals {
    previous: Vec<(libc::c_int, Disposition)>,
}

impl Signals {
    /// SIGTERM/SIGINT stop the loop, SIGCHLD only wakes it (a no-op handler); each writes its number
    /// to the wakeup pipe.
    pub(super) fn install(wake: RawFd) -> Signals {
        WAKE.store(wake, Ordering::SeqCst);
        let mut previous = Vec::new();
        for signal in SIGNALS {
            // SAFETY: the handler only touches atomics and write(2); the old action is kept.
            if let Ok(old) =
                unsafe { set_signal(signal, Handler::Call(on_signal), libc::SA_ONSTACK) }
            {
                previous.push((signal, old));
            }
        }
        Signals { previous }
    }

    pub(super) fn restore(self) {
        WAKE.store(-1, Ordering::SeqCst);
        for (signal, old) in self.previous {
            old.restore(signal);
        }
    }
}

// ---- AddressEvents -------------------------------------------------------------------------------

/// Passive link/IPv4-address events; never configures the USB interface.
pub(super) struct AddressEvents {
    pub(super) sock: Option<OwnedFd>,
}

impl AddressEvents {
    pub(super) fn new() -> AddressEvents {
        // SAFETY: plain socket calls; the descriptor is owned right after it is created.
        unsafe {
            let fd = libc::socket(
                libc::AF_NETLINK,
                libc::SOCK_RAW | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                libc::NETLINK_ROUTE,
            );
            if fd < 0 {
                return AddressEvents { sock: None };
            }
            let sock = OwnedFd::from_raw_fd(fd);
            let mut address: libc::sockaddr_nl = std::mem::zeroed();
            address.nl_family = libc::AF_NETLINK as libc::sa_family_t;
            address.nl_groups = 1 | 0x10; // RTMGRP_LINK | RTMGRP_IPV4_IFADDR
            let bound = libc::bind(
                sock.as_raw_fd(),
                (&address as *const libc::sockaddr_nl).cast(),
                std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
            );
            AddressEvents {
                sock: (bound == 0).then_some(sock),
            }
        }
    }

    /// Wait on the wakeup pipe and the socket for up to `timeout` s; drain both; whether the
    /// kernel reported an address or link change.
    pub(super) fn wait(&mut self, timeout: f64, wake: RawFd) -> bool {
        let mut fds = vec![libc::pollfd {
            fd: wake,
            events: libc::POLLIN,
            revents: 0,
        }];
        if let Some(sock) = &self.sock {
            fds.push(libc::pollfd {
                fd: sock.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            });
        }
        let deadline = Instant::now() + Duration::from_secs_f64(timeout);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            // The full time left, never rounded down to whole milliseconds:
            // the next tick's `now >= refresh_at` relies on it.
            let timeout = libc::timespec {
                tv_sec: left.as_secs() as libc::time_t,
                tv_nsec: left.subsec_nanos() as libc::c_long,
            };
            // SAFETY: a valid array of pollfd for its length and a valid timespec.
            let ready = unsafe {
                libc::ppoll(
                    fds.as_mut_ptr(),
                    fds.len() as libc::nfds_t,
                    &timeout,
                    std::ptr::null(),
                )
            };
            // An interrupted wait is retried with the time left (the handler has written
            // to the pipe, so it returns at once).
            if ready < 0 && io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            break;
        }
        if fds[0].revents != 0 {
            let mut buffer = [0u8; 4096];
            // SAFETY: a writable buffer; the pipe is non-blocking.
            while unsafe { libc::read(wake, buffer.as_mut_ptr().cast(), buffer.len()) } > 0 {}
        }
        let changed = fds.get(1).is_some_and(|f| f.revents != 0);
        if changed {
            // Drain a bounded burst. Remaining events stay readable next time.
            let mut buffer = vec![0u8; 65_536];
            for _ in 0..32 {
                let Some(sock) = &self.sock else { break };
                // SAFETY: a writable buffer; the socket is non-blocking.
                let read = unsafe {
                    libc::recv(
                        sock.as_raw_fd(),
                        buffer.as_mut_ptr().cast(),
                        buffer.len(),
                        0,
                    )
                };
                if read >= 0 {
                    continue;
                }
                match io::Error::last_os_error().raw_os_error() {
                    Some(libc::EAGAIN) => break,
                    Some(libc::EINTR) => continue,
                    _ => {
                        self.sock = None; // refresh on the bounded fallback cadence
                        break;
                    }
                }
            }
        }
        changed
    }
}
