//! The power socket's requests: one line from one root client (a `shutdown` client or Reborn's
//! acknowledgement), one line back.
// SPDX-License-Identifier: GPL-2.0-only
use super::super::{
    coordinator::{Coordinator, Intent},
    unix,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{os::unix::net::UnixStream, time::Duration};
use y2p_core::{json::dumps, lenient};

const REQUEST_LIMIT: usize = 2048;

/// One request on the power socket.
enum Request {
    /// Reborn's answer to a pending intent (its id and outcome when they are texts).
    Ack {
        id: Option<String>,
        outcome: Option<String>,
    },
    /// A transition (`shutdown` or Reborn); the reason defaults to `user`.
    Transition {
        action: Option<String>,
        reason: Option<String>,
    },
}

/// A request line's fields: texts where it has texts (`Some(None)`: there, but no text).
#[derive(Deserialize, Default)]
#[serde(default)]
struct Line {
    #[serde(deserialize_with = "lenient::text")]
    operation: Option<String>,
    #[serde(deserialize_with = "text_there")]
    id: Option<Option<String>>,
    #[serde(deserialize_with = "text_there")]
    outcome: Option<Option<String>>,
    #[serde(deserialize_with = "lenient::text")]
    action: Option<String>,
    #[serde(deserialize_with = "text_there")]
    reason: Option<Option<String>>,
}

fn text_there<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error> {
    lenient::text(deserializer).map(Some)
}

impl Request {
    /// The request's fields, or why it is none: `invalid_request` for no object,
    /// `missing_<field>` for a field it lacks.
    fn of(request: Value) -> Result<Request, String> {
        let Value::Object(request) = request else {
            return Err("invalid_request".to_owned());
        };
        let line: Line = lenient::read(request);
        if line.operation.as_deref() == Some("ack") {
            let id = line.id.ok_or("missing_id")?;
            let outcome = line.outcome.ok_or("missing_outcome")?;
            return Ok(Request::Ack { id, outcome });
        }
        Ok(Request::Transition {
            action: line.action,
            reason: line.reason.unwrap_or_else(|| Some("user".into())),
        })
    }
}

/// One request line from one client; the answer is one line, `{"ok": true, "result": intent}` or
/// `{"ok": false, "error": str(error)}`.
pub(super) fn serve_client(mut client: UnixStream, coordinator: &mut Coordinator) {
    let answer = match handle(&mut client, coordinator) {
        Ok(intent) => Answer {
            ok: true,
            result: Some(intent),
            error: None,
        },
        Err(error) => Answer {
            ok: false,
            result: None,
            error: Some(error),
        },
    };
    if let Ok(text) = dumps(&answer) {
        let _ = unix::send_all(&mut client, (text + "\n").as_bytes());
    }
}

/// The socket's answer line.
#[derive(Serialize)]
struct Answer {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Intent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// The request's result, or its error text (the socket's answers keep the texts they had, e.g.
/// `'list' object has no attribute 'get'`: callers log them).
fn handle(client: &mut UnixStream, coordinator: &mut Coordinator) -> Result<Intent, String> {
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
        Request::Ack { id, outcome } => coordinator.ack(id.as_deref(), pid, outcome.as_deref()),
        Request::Transition { action, reason } => {
            coordinator.request(action.as_deref(), reason.as_deref(), 10)
        }
    }
    .map_err(|e| e.message())
}
