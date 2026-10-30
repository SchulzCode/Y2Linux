//! What wakes the ssh daemon: SIGTERM/SIGINT (which also end it) and SIGCHLD through a wakeup pipe,
//! and the kernel's link and IPv4 address events on a netlink socket.
// SPDX-License-Identifier: GPL-2.0-only
use std::{
    io,
    os::fd::{AsRawFd, OwnedFd, RawFd},
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
    /// One netlink datagram at a time, kept for the daemon's lifetime.
    buffer: Vec<u8>,
}

/// The netlink message types and attributes the events are told apart by (linux/rtnetlink.h,
/// linux/if_link.h, linux/if_addr.h).
const RTM_NEWLINK: u16 = 16;
const RTM_DELLINK: u16 = 17;
const RTM_NEWADDR: u16 = 20;
const RTM_DELADDR: u16 = 21;
const IFLA_IFNAME: u16 = 3;
const IFA_LABEL: u16 = 3;
/// struct nlmsghdr, struct ifinfomsg, struct ifaddrmsg, struct rtattr.
const NLMSG_HEADER: usize = 16;
const IFINFOMSG: usize = 16;
const IFADDRMSG: usize = 8;
const RTA_HEADER: usize = 4;

fn align(length: usize) -> usize {
    (length + 3) & !3
}

fn u16_at(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_ne_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn u32_at(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_ne_bytes(
        data.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

/// The NUL-terminated name in attribute `kind` of a message body after its fixed header.
fn name_attribute(body: &[u8], fixed: usize, kind: u16) -> Option<&[u8]> {
    let mut attributes = body.get(fixed..)?;
    while attributes.len() >= RTA_HEADER {
        let length = usize::from(u16_at(attributes, 0)?);
        if length < RTA_HEADER || length > attributes.len() {
            return None;
        }
        if u16_at(attributes, 2)? == kind {
            let value = &attributes[RTA_HEADER..length];
            return Some(value.split(|b| *b == 0).next().unwrap_or(value));
        }
        attributes = attributes.get(align(length)..).unwrap_or_default();
    }
    None
}

/// Whether a netlink datagram may concern usb0. Only a link or IPv4 address message that names
/// another interface does not (wlan0's link and address changes need no new `ip` request); usb0
/// (or an alias of it), a message without a name and anything not understood do.
fn concerns_usb0(mut data: &[u8]) -> bool {
    while !data.is_empty() {
        let (Some(length), Some(kind)) = (u32_at(data, 0), u16_at(data, 4)) else {
            return true;
        };
        let length = length as usize;
        if length < NLMSG_HEADER || length > data.len() {
            return true;
        }
        let body = &data[NLMSG_HEADER..length];
        let name = match kind {
            RTM_NEWLINK | RTM_DELLINK => name_attribute(body, IFINFOMSG, IFLA_IFNAME),
            RTM_NEWADDR | RTM_DELADDR => name_attribute(body, IFADDRMSG, IFA_LABEL),
            _ => return true,
        };
        match name {
            Some(name) if name != b"usb0" && !name.starts_with(b"usb0:") => {}
            _ => return true,
        }
        data = data.get(align(length)..).unwrap_or_default();
    }
    false
}

impl AddressEvents {
    pub(super) fn new() -> AddressEvents {
        // RTMGRP_LINK | RTMGRP_IPV4_IFADDR.
        let sock = y2p_core::sys::netlink_route(1 | 0x10, true).ok();
        AddressEvents::on(sock)
    }

    /// The events read from `sock` (none: every tick reads the address).
    pub(super) fn on(sock: Option<OwnedFd>) -> AddressEvents {
        AddressEvents {
            sock,
            buffer: Vec::new(),
        }
    }

    /// Wait on the wakeup pipe and the socket for up to `timeout` s; drain both; whether the
    /// kernel reported a link or address change that may concern usb0.
    pub(super) fn wait(&mut self, timeout: f64, wake: RawFd) -> bool {
        let mut fds =
            [wake, self.sock.as_ref().map_or(-1, AsRawFd::as_raw_fd)].map(|fd| libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            });
        let count = if self.sock.is_some() { 2 } else { 1 };
        let deadline = Instant::now() + Duration::from_secs_f64(timeout);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            // The full time left, never rounded down to whole milliseconds:
            // the next tick's `now >= refresh_at` relies on it.
            match y2p_core::sys::poll(&mut fds[..count], left, y2p_core::sys::Rounding::Exact) {
                // An interrupted wait is retried with the time left (the handler has written
                // to the pipe, so it returns at once).
                Err(error) if error.raw_os_error() == Some(libc::EINTR) => continue,
                _ => break,
            }
        }
        if fds[0].revents != 0 {
            let mut buffer = [0u8; 4096];
            // SAFETY: a writable buffer; the pipe is non-blocking.
            while unsafe { libc::read(wake, buffer.as_mut_ptr().cast(), buffer.len()) } > 0 {}
        }
        let mut changed = false;
        if count == 2 && fds[1].revents != 0 {
            // Drain a bounded burst. Remaining events stay readable next time.
            self.buffer.resize(65_536, 0);
            for _ in 0..32 {
                let Some(sock) = &self.sock else { break };
                // SAFETY: a writable buffer; the socket is non-blocking.
                let read = unsafe {
                    libc::recv(
                        sock.as_raw_fd(),
                        self.buffer.as_mut_ptr().cast(),
                        self.buffer.len(),
                        0,
                    )
                };
                if read >= 0 {
                    changed |= concerns_usb0(&self.buffer[..read as usize]);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::FromRawFd;

    /// A netlink message of `kind` whose fixed header is `fixed` bytes, with one name attribute.
    fn message(kind: u16, fixed: usize, name: &str) -> Vec<u8> {
        let attribute = RTA_HEADER + name.len() + 1;
        let length = NLMSG_HEADER + fixed + align(attribute);
        let mut data = Vec::new();
        data.extend((length as u32).to_ne_bytes());
        data.extend(kind.to_ne_bytes());
        data.extend([0u8; 10]); // flags, sequence, port id
        data.extend(vec![0u8; fixed]);
        data.extend((attribute as u16).to_ne_bytes());
        data.extend(3u16.to_ne_bytes());
        data.extend(name.as_bytes());
        data.resize(length, 0);
        data
    }

    #[test]
    fn only_events_naming_another_interface_are_ignored() {
        let address = |name| message(RTM_NEWADDR, IFADDRMSG, name);
        let link = |name| message(RTM_NEWLINK, IFINFOMSG, name);
        assert!(!concerns_usb0(&address("wlan0")));
        assert!(!concerns_usb0(&link("wlan0")));
        assert!(!concerns_usb0(&[link("lo"), address("wlan0")].concat()));
        assert!(concerns_usb0(&address("usb0")));
        assert!(concerns_usb0(&address("usb0:1")));
        assert!(concerns_usb0(&message(RTM_DELLINK, IFINFOMSG, "usb0")));
        assert!(concerns_usb0(&[address("wlan0"), link("usb0")].concat()));
        // Anything not understood counts: another type, a truncated message, stray bytes.
        assert!(concerns_usb0(&message(2, IFADDRMSG, "wlan0")));
        assert!(concerns_usb0(&address("wlan0")[..20]));
        assert!(concerns_usb0(b"address-change"));
        assert!(!concerns_usb0(b""));
    }

    #[test]
    fn another_interface_wakes_the_loop_without_a_change() {
        let mut fds = [-1; 2];
        // SAFETY: a two-element array for the new descriptors, owned right after.
        let (left, right) = unsafe {
            assert_eq!(
                libc::socketpair(
                    libc::AF_UNIX,
                    libc::SOCK_DGRAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                    0,
                    fds.as_mut_ptr()
                ),
                0
            );
            (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1]))
        };
        let send = |data: &[u8]| {
            // SAFETY: the slice is readable for its length.
            let sent = unsafe { libc::write(right.as_raw_fd(), data.as_ptr().cast(), data.len()) };
            assert_eq!(sent, data.len() as isize);
        };
        let mut events = AddressEvents::on(Some(left));
        let started = Instant::now();
        send(&message(RTM_NEWADDR, IFADDRMSG, "wlan0"));
        assert!(!events.wait(5.0, -1));
        send(&message(RTM_NEWLINK, IFINFOMSG, "usb0"));
        assert!(events.wait(5.0, -1));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(events.sock.is_some());
    }
}
