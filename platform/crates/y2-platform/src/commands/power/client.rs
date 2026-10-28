//! `y2-platform shutdown poweroff|reboot [--reason user|service|update]` (`power.rpc`): ask the power
//! daemon for the transition and print the intent it answers with, in the daemon's key order. It
//! returns at once; the daemon executes the intent after Reborn's acknowledgement or the 10 s grace.
// SPDX-License-Identifier: GPL-2.0-only
use super::{crash, daemon::SOCKET, unix};
use crate::reply::{Failure, Outcome, Reply};
use std::time::Duration;
use y2p_core::{
    json::{dumps, Separators},
    power_json::{Json, Object},
    Root,
};

const TIMEOUT: Duration = Duration::from_secs(1);
const REQUEST_LIMIT: usize = 2048;
const RESPONSE_LIMIT: usize = 8192;

/// `rpc(ctx, request)`: the daemon's `result`, or its `error` as a ValueError.
pub fn rpc(root: &Root, request: &Object) -> Result<Json, Failure> {
    let data = dumps(request, Separators::Spaced).map_err(Failure::value)? + "\n";
    if data.len() > REQUEST_LIMIT {
        return Err(Failure::value("request_too_large"));
    }
    let mut client = unix::connect(&root.path(SOCKET), TIMEOUT)?;
    unix::send_all(&mut client, data.as_bytes())?;
    let mut answer = Vec::new();
    while !answer.contains(&b'\n') {
        let mut part = [0u8; 4096];
        let n = unix::recv(&mut client, &mut part)?;
        if n == 0 || answer.len() + n > RESPONSE_LIMIT {
            return Err(Failure::value("invalid_power_response"));
        }
        answer.extend_from_slice(&part[..n]);
    }
    // Accepted difference: an answer that is not JSON names serde's error, not json's.
    let result = Json::parse(&answer).map_err(Failure::value)?;
    let Json::Object(result) = result else {
        crash(&format!(
            "AttributeError: '{}' object has no attribute 'get'",
            result.type_name()
        ));
    };
    if !result.value("ok").truthy() {
        let error = result
            .get("error")
            .map_or_else(|| "power_request_failed".to_owned(), Json::py_str);
        return Err(Failure::Value(error));
    }
    match result.get("result") {
        Some(value) => Ok(value.clone()),
        None => crash("KeyError: 'result'"),
    }
}

pub fn shutdown(root: &Root, action: &str, reason: &str) -> Outcome {
    let request = Object::new().with("action", action).with("reason", reason);
    Reply::unsorted(&rpc(root, &request)?, 0)
}
