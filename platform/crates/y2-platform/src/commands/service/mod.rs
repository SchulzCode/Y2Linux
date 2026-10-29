//! Group `service`: `update`, `firmware` and `service-daemon`.
// SPDX-License-Identifier: GPL-2.0-only
pub(crate) mod daemon;
mod download;
mod image;
mod process;
pub mod radiofw;
mod storage;
pub mod update;
mod usb;

use super::Command;
use serde_json::{Map, Value};
use y2p_core::Root;

const UPDATE_ACTIONS: [&str; 10] = [
    "status",
    "check",
    "stage",
    "apply",
    "rollback",
    "cancel",
    "health-ack",
    "auto-check",
    "probe-recovery",
    "recovery-status",
];

/// The `time` command's record (clock and TLS readiness), as its fields.
fn time_status(root: &Root) -> Map<String, Value> {
    super::time::record(root)
}

/// `update <action>` with `--url URL` and `--package DIR` as the parser's canonical form gives them.
fn update_route(args: &[&str]) -> Option<Command> {
    let (action, rest) = args.split_first()?;
    if !UPDATE_ACTIONS.contains(action) {
        return None;
    }
    let (mut url, mut package) = (None, None);
    let mut rest = rest;
    while let [option, value, tail @ ..] = rest {
        match *option {
            "--url" if url.is_none() => url = Some(value.to_string()),
            "--package" if package.is_none() => package = Some(value.to_string()),
            _ => return None,
        }
        rest = tail;
    }
    if !rest.is_empty() {
        return None;
    }
    let action = action.to_string();
    Some(Box::new(move |root| {
        update::run(root, &action, url.as_deref(), package.as_deref())
    }))
}

/// The Rust implementation of these exact arguments, if this group has one.
pub fn route(args: &[&str]) -> Option<Command> {
    match args {
        ["service-daemon"] => Some(Box::new(daemon::serve)),
        ["update", rest @ ..] => update_route(rest),
        ["firmware", action, path @ ..]
            if ["status", "scan", "install"].contains(action) && path.len() <= 1 =>
        {
            let (action, path) = (action.to_string(), path.first().map(|p| p.to_string()));
            Some(Box::new(move |root| {
                radiofw::run(root, &action, path.as_deref())
            }))
        }
        _ => None,
    }
}
