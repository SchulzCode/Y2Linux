//! The power socket's requests: one line from one root client (a `shutdown` client or Reborn's
//! acknowledgement), one line back.
// SPDX-License-Identifier: GPL-2.0-only
use super::super::{coordinator::Coordinator, unix};
use serde_json::Value;
use std::{os::unix::net::UnixStream, time::Duration};
use y2p_core::{
    json::{dumps, Object, ObjectExt, Separators},
    py::PyValue,
};

const REQUEST_LIMIT: usize = 2048;

/// One request on the power socket.
enum Request {
    /// Reborn's answer to a pending intent.
    Ack { id: Value, outcome: Value },
    /// A transition (`shutdown` or Reborn); the reason defaults to `user`.
    Transition { action: Value, reason: Value },
}

impl Request {
    /// The request's fields, or the error text of the field that is missing (`'id'`, as the socket
    /// has always answered it).
    fn of(request: Value) -> Result<Request, String> {
        let Value::Object(mut request) = request else {
            return Err(format!(
                "'{}' object has no attribute 'get'",
                request.type_name()
            ));
        };
        if request.value("operation").is_str("ack") {
            let id = request.shift_remove("id").ok_or("'id'")?;
            let outcome = request.shift_remove("outcome").ok_or("'outcome'")?;
            return Ok(Request::Ack { id, outcome });
        }
        Ok(Request::Transition {
            action: request.value("action"),
            reason: request
                .shift_remove("reason")
                .unwrap_or_else(|| "user".into()),
        })
    }
}

/// One request line from one client; the answer is one line, `{"ok": true, "result": intent}` or
/// `{"ok": false, "error": str(error)}`.
pub(super) fn serve_client(mut client: UnixStream, coordinator: &mut Coordinator) {
    let result = handle(&mut client, coordinator);
    let answer = match result {
        Ok(intent) => Object::new().with("ok", true).with("result", intent),
        Err(error) => Object::new().with("ok", false).with("error", error),
    };
    if let Ok(text) = dumps(&answer, Separators::Spaced) {
        let _ = unix::send_all(&mut client, (text + "\n").as_bytes());
    }
}

/// The request's result, or its error text (the socket's answers keep the texts they had, e.g.
/// `'list' object has no attribute 'get'`: callers log them).
fn handle(client: &mut UnixStream, coordinator: &mut Coordinator) -> Result<Object, String> {
    unix::timeouts(client, Duration::from_millis(200)).map_err(|e| e.message())?;
    let (pid, uid) = unix::peer(client).map_err(|e| e.message())?;
    if uid != 0 {
        return Err("owner_privilege_required".into());
    }
    let mut data = Vec::new();
    while !data.contains(&b'\n') {
        let mut part = [0u8; REQUEST_LIMIT + 1];
        let n = unix::recv(client, &mut part).map_err(|e| e.message())?;
        if n == 0 || data.len() + n > REQUEST_LIMIT {
            return Err("invalid_power_request".into());
        }
        data.extend_from_slice(&part[..n]);
    }
    // Accepted difference: text that is not JSON is refused with serde's message, not json's.
    let request = serde_json::from_slice::<Value>(&data).map_err(|e| e.to_string())?;
    match Request::of(request)? {
        Request::Ack { id, outcome } => coordinator.ack(&id, pid, &outcome),
        Request::Transition { action, reason } => coordinator.request(&action, &reason, 10),
    }
    .map_err(|e| e.message())
}
