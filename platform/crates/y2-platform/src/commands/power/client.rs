//! `y2-platform shutdown poweroff|reboot [--reason user|service|update]`: ask the power
//! daemon for the transition and print the intent it answers with, in the daemon's key order. It
//! returns at once; the daemon executes the intent after Reborn's acknowledgement or the 10 s grace.
// SPDX-License-Identifier: GPL-2.0-only
use super::{coordinator::Intent, daemon::SOCKET, unix};
use crate::reply::{Failure, Outcome, Reply};
use serde_json::Value;
use std::time::Duration;
use y2p_core::{
    json::dumps,
    json::{Object, ObjectExt},
    Root,
};

const TIMEOUT: Duration = Duration::from_secs(1);
const REQUEST_LIMIT: usize = 2048;
const RESPONSE_LIMIT: usize = 8192;

/// `rpc(ctx, request)`: the daemon's `result` (the intent), or its `error` as a ValueError.
pub fn rpc(root: &Root, request: &Object) -> Result<Intent, Failure> {
    let data = dumps(request).map_err(Failure::value)? + "\n";
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
    let result = serde_json::from_slice::<Value>(&answer).map_err(Failure::value)?;
    // An answer that is no object, or an `ok` one without its result, is no answer.
    let Value::Object(mut result) = result else {
        return Err(Failure::value("invalid_power_response"));
    };
    if result.get("ok") != Some(&Value::Bool(true)) {
        let error = match result.get("error") {
            Some(Value::String(error)) => error.clone(),
            Some(other) => other.to_string(),
            None => "power_request_failed".to_owned(),
        };
        return Err(Failure::Value(error));
    }
    result
        .shift_remove("result")
        .and_then(|intent| serde_json::from_value(intent).ok())
        .ok_or_else(|| Failure::value("invalid_power_response"))
}

pub fn shutdown(root: &Root, action: &str, reason: &str) -> Outcome {
    let request = Object::new().with("action", action).with("reason", reason);
    Reply::unsorted(&rpc(root, &request)?, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Fixture;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;

    /// A daemon that answers one request with `answer` (a line), as `rpc` sees it.
    fn answered(answer: &'static str) -> Result<Intent, Failure> {
        let fixture = Fixture::new();
        fixture.mkdir("/run/y2");
        let listener = UnixListener::bind(fixture.path(SOCKET)).unwrap();
        let daemon = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            (&stream).write_all(answer.as_bytes()).unwrap();
            line
        });
        let request = Object::new()
            .with("action", "poweroff")
            .with("reason", "user");
        let result = rpc(&fixture.root, &request);
        assert_eq!(
            daemon.join().unwrap(),
            "{\"action\":\"poweroff\",\"reason\":\"user\"}\n"
        );
        result
    }

    #[test]
    fn the_daemons_intent_is_returned() {
        let intent = answered(
            "{\"ok\": true, \"result\": {\"schema\": 1, \"id\": \"y\", \"boot_id\": \"b\", \
             \"state\": \"ShutdownPending\", \"action\": \"poweroff\", \"reason\": \"user\", \
             \"requested_monotonic_ns\": 1, \"deadline_monotonic_ns\": 2, \"client\": null, \
             \"acknowledgement\": null, \"failure\": null}}\n",
        )
        .unwrap();
        assert_eq!((intent.id.as_str(), intent.deadline_monotonic_ns), ("y", 2));
        // Not an intent: no answer.
        let other = answered("{\"ok\": true, \"result\": {\"state\": \"x\", \"id\": \"y\"}}\n");
        assert_eq!(other.unwrap_err().message(), "invalid_power_response");
    }

    #[test]
    fn odd_answers_are_refusals_not_crashes() {
        for answer in ["[1]\n", "{\"ok\": true}\n", "5\n"] {
            let failure = answered(answer).unwrap_err();
            assert_eq!(failure.message(), "invalid_power_response", "{answer}");
            assert!(failure.caught(), "a JSON failure record, exit 1");
        }
        let refused = answered("{\"ok\": false, \"error\": \"invalid_grace\"}\n").unwrap_err();
        assert_eq!(refused.message(), "invalid_grace");
        let silent = answered("{\"ok\": 0}\n").unwrap_err();
        assert_eq!(silent.message(), "power_request_failed");
    }
}
