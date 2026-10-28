//! The power socket as Python's `socket` module drives it: a listening AF_UNIX stream socket with
//! backlog 4, `accept()` with a timeout (poll, retried on EINTR until the deadline), a client whose
//! `connect()` does not wait for a full backlog (EAGAIN is `BlockingIOError`, as CPython's timeout
//! sockets), `SO_PEERCRED`, and Python's error texts (`timed out` for a socket timeout).
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::Failure;
use std::{
    io,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::{
            ffi::OsStrExt,
            net::{UnixListener, UnixStream},
        },
    },
    path::Path,
    time::{Duration, Instant},
};
use y2p_core::py;

/// An `OSError` from a socket call (Python names no file in these).
pub fn os(error: io::Error) -> Failure {
    let (text, class) = py::os_error(&error, None);
    Failure::Os { text, class }
}

/// `socket.timeout` (`TimeoutError: timed out`).
pub fn timed_out() -> Failure {
    Failure::Os {
        text: "timed out".into(),
        class: "TimeoutError",
    }
}

fn address(path: &Path) -> Result<(libc::sockaddr_un, libc::socklen_t), Failure> {
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: an all-zero sockaddr_un is valid.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.len() > address.sun_path.len() {
        return Err(Failure::os_message("AF_UNIX path too long"));
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (slot, byte) in address.sun_path.iter_mut().zip(bytes) {
        *slot = *byte as libc::c_char;
    }
    let length = std::mem::size_of::<libc::sa_family_t>() + bytes.len();
    Ok((address, length as libc::socklen_t))
}

fn socket(flags: libc::c_int) -> Result<OwnedFd, Failure> {
    // SAFETY: plain socket(2); the result is checked and owned below.
    let fd = unsafe {
        libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC | flags,
            0,
        )
    };
    if fd < 0 {
        return Err(os(io::Error::last_os_error()));
    }
    // SAFETY: a new descriptor this function owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// `bind(path)`, `chmod(path, 0o600)`, `listen(4)`; non-blocking for the timed accept.
pub fn listen(path: &Path) -> Result<UnixListener, Failure> {
    let fd = socket(libc::SOCK_NONBLOCK)?;
    let (address, length) = address(path)?;
    // SAFETY: `address` is a valid sockaddr_un of `length` bytes.
    if unsafe {
        libc::bind(
            fd.as_raw_fd(),
            (&address as *const libc::sockaddr_un).cast(),
            length,
        )
    } < 0
    {
        return Err(os(io::Error::last_os_error()));
    }
    std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600))
        .map_err(|e| Failure::os(e, path))?;
    // SAFETY: listen(2) on the bound socket.
    if unsafe { libc::listen(fd.as_raw_fd(), 4) } < 0 {
        return Err(os(io::Error::last_os_error()));
    }
    Ok(UnixListener::from(fd))
}

fn poll(fd: libc::c_int, events: libc::c_short, deadline: Instant) -> io::Result<bool> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(false);
        }
        // CPython rounds the timeout up to whole milliseconds.
        let ms = remaining
            .as_nanos()
            .div_ceil(1_000_000)
            .min(i32::MAX as u128) as libc::c_int;
        let mut entry = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        // SAFETY: one valid pollfd.
        match unsafe { libc::poll(&mut entry, 1, ms) } {
            0 => return Ok(false),
            n if n > 0 => return Ok(true),
            _ => {
                let error = io::Error::last_os_error();
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(error);
                }
            }
        }
    }
}

/// `server.settimeout(wait); server.accept()`: `None` when the time passed without a client.
pub fn accept(listener: &UnixListener, wait: Duration) -> Result<Option<UnixStream>, Failure> {
    let deadline = Instant::now() + wait;
    loop {
        if !poll(listener.as_raw_fd(), libc::POLLIN, deadline).map_err(os)? {
            return Ok(None);
        }
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).map_err(os)?;
                return Ok(Some(stream));
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            Err(e) => return Err(os(e)),
        }
    }
}

/// `SO_PEERCRED`: (pid, uid).
pub fn peer(stream: &UnixStream) -> Result<(i64, u32), Failure> {
    // SAFETY: an all-zero ucred is valid; getsockopt fills it.
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: the buffer and its length describe `credentials`.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if result < 0 {
        return Err(os(io::Error::last_os_error()));
    }
    Ok((i64::from(credentials.pid), credentials.uid))
}

/// `client.settimeout(timeout); client.connect(path)`.
pub fn connect(path: &Path, timeout: Duration) -> Result<UnixStream, Failure> {
    let fd = socket(libc::SOCK_NONBLOCK)?;
    let (address, length) = address(path)?;
    // SAFETY: `address` is a valid sockaddr_un of `length` bytes.
    let result = unsafe {
        libc::connect(
            fd.as_raw_fd(),
            (&address as *const libc::sockaddr_un).cast(),
            length,
        )
    };
    if result < 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINPROGRESS) {
            return Err(os(error));
        }
        if !poll(fd.as_raw_fd(), libc::POLLOUT, Instant::now() + timeout).map_err(os)? {
            return Err(timed_out());
        }
        let mut code: libc::c_int = 0;
        let mut size = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        // SAFETY: SO_ERROR into one int.
        unsafe {
            libc::getsockopt(
                fd.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                (&mut code as *mut libc::c_int).cast(),
                &mut size,
            )
        };
        if code != 0 {
            return Err(os(io::Error::from_raw_os_error(code)));
        }
    }
    let stream = UnixStream::from(fd);
    stream.set_nonblocking(false).map_err(os)?;
    timeouts(&stream, timeout)?;
    Ok(stream)
}

/// A socket timeout for each send and receive.
pub fn timeouts(stream: &UnixStream, timeout: Duration) -> Result<(), Failure> {
    stream.set_read_timeout(Some(timeout)).map_err(os)?;
    stream.set_write_timeout(Some(timeout)).map_err(os)
}

/// One `recv(size)`: a timeout is `timed out`.
pub fn recv(stream: &mut UnixStream, buffer: &mut [u8]) -> Result<usize, Failure> {
    use std::io::Read;
    loop {
        match stream.read(buffer) {
            Ok(n) => return Ok(n),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(timed_out())
            }
            Err(e) => return Err(os(e)),
        }
    }
}

/// `sendall(data)`.
pub fn send_all(stream: &mut UnixStream, data: &[u8]) -> Result<(), Failure> {
    use std::io::Write;
    stream.write_all(data).map_err(|e| {
        if matches!(
            e.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        ) {
            timed_out()
        } else {
            os(e)
        }
    })
}
