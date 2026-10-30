//! Reborn's control socket (`/run/reborn/control.sock`): one bounded request and its
//! acknowledgement, never just process liveness.
// SPDX-License-Identifier: GPL-2.0-only
use super::super::{interrupted, pyjson, E};
use serde_json::{json, Map, Value};
use std::os::fd::{AsFd, AsRawFd};
use y2p_core::py::PyValue;
use y2p_core::{
    json::{dumps, Separators},
    py::is_true,
    Root,
};

const RESPONSE_LIMIT: usize = 131_072;
const RPC_TIMEOUT_MS: i32 = 5000;

fn timed_out() -> E {
    E::Os {
        class: "TimeoutError",
        text: "timed out".into(),
        errno: None,
    }
}

/// Wait for one event on `fd` within the time left before `deadline` (an interrupted poll goes on
/// unless the helper was asked to stop).
fn wait(fd: i32, events: i16, deadline: std::time::Instant) -> Result<(), E> {
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let mut poll = [libc::pollfd {
            fd,
            events,
            revents: 0,
        }];
        match y2p_core::sys::poll(&mut poll, left, y2p_core::sys::Rounding::Down) {
            Ok(0) => return Err(timed_out()),
            Ok(_) => return Ok(()),
            Err(error) if error.raw_os_error() == Some(libc::EINTR) => interrupted()?,
            Err(error) => return Err(E::from(error)),
        }
    }
}

fn last_error() -> std::io::Error {
    std::io::Error::last_os_error()
}

/// `application_rpc(ctx, operation, **fields)`: a bounded explicit application acknowledgement over
/// Reborn's control socket, never just process liveness. `dark` adds `"dark": true` (F47).
pub fn application_rpc(root: &Root, operation: &str, dark: bool) -> Result<Map<String, Value>, E> {
    let op = dumps(&operation, Separators::Spaced).map_err(|e| E::value(e.to_string()))?;
    let request = format!(
        "{{\"version\": 1, \"id\": 6582, \"command\": {{\"op\": {op}{}}}}}\n",
        if dark { ", \"dark\": true" } else { "" }
    );
    let path = root.path("/run/reborn/control.sock");
    let socket = y2p_core::sys::unix_stream(true).map_err(E::from)?;
    let fd = socket.as_raw_fd();
    let address = y2p_core::sys::unix_address(&path).ok_or_else(|| E::Os {
        class: "OSError",
        text: "AF_UNIX path too long".into(),
        errno: None,
    })?;
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_millis(RPC_TIMEOUT_MS as u64);
    if let Err(error) = y2p_core::sys::connect_unix(socket.as_fd(), &address) {
        match error.raw_os_error() {
            Some(libc::EINPROGRESS) | Some(libc::EINTR) => {
                interrupted()?;
                wait(fd, libc::POLLOUT, deadline)?;
                y2p_core::sys::socket_error(socket.as_fd())?;
            }
            _ => return Err(E::from(error)),
        }
    }
    interrupted()?;
    // sendall: one deadline for the whole request.
    let mut data = request.as_bytes();
    let send_deadline =
        std::time::Instant::now() + std::time::Duration::from_millis(RPC_TIMEOUT_MS as u64);
    while !data.is_empty() {
        wait(fd, libc::POLLOUT, send_deadline)?;
        // SAFETY: `data` is a valid buffer of its length.
        let sent = unsafe { libc::send(fd, data.as_ptr().cast(), data.len(), libc::MSG_NOSIGNAL) };
        if sent < 0 {
            let error = last_error();
            match error.raw_os_error() {
                Some(libc::EINTR) => interrupted()?,
                Some(libc::EAGAIN) => {}
                _ => return Err(E::from(error)),
            }
            continue;
        }
        data = &data[sent as usize..];
    }
    let mut response: Vec<u8> = Vec::new();
    let mut block = [0u8; 4096];
    while !response.contains(&b'\n') && response.len() <= RESPONSE_LIMIT {
        // Each recv has its own 5-s timeout.
        let until =
            std::time::Instant::now() + std::time::Duration::from_millis(RPC_TIMEOUT_MS as u64);
        wait(fd, libc::POLLIN, until)?;
        // SAFETY: `block` is writable for its length.
        let got = unsafe { libc::recv(fd, block.as_mut_ptr().cast(), block.len(), 0) };
        if got < 0 {
            let error = last_error();
            match error.raw_os_error() {
                Some(libc::EINTR) => {
                    interrupted()?;
                    continue;
                }
                Some(libc::EAGAIN) => continue,
                _ => return Err(E::from(error)),
            }
        }
        if got == 0 {
            break;
        }
        response.extend_from_slice(&block[..got as usize]);
    }
    drop(socket);
    interrupted()?;
    if response.len() > RESPONSE_LIMIT {
        return Err(E::value("reborn_sleep_response_too_large"));
    }
    let line = response.split(|b| *b == b'\n').next().unwrap_or_default();
    let answer = pyjson::loads(line).map_err(|e| E::Value {
        class: e.class,
        text: e.text,
    })?;
    let Value::Object(answer) = answer else {
        return Err(E::value("reborn_sleep_acknowledgement_failed"));
    };
    if !answer.get("id").is_some_and(|id| id.py_eq(&json!(6582))) {
        return Err(E::value("reborn_sleep_acknowledgement_failed"));
    }
    if !is_true(answer.get("ok")) {
        return Err(E::value(match answer.get("error") {
            Some(Value::String(error)) if error.chars().count() <= 256 => error.clone(),
            _ => "reborn_sleep_acknowledgement_failed".into(),
        }));
    }
    match answer.get("result") {
        Some(Value::Object(result)) => Ok(result.clone()),
        _ => Err(E::value("reborn_sleep_acknowledgement_invalid")),
    }
}
