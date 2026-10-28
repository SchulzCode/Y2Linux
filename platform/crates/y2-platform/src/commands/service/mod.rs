//! Group `service` of the port (docs/planning/PLATFORM-RUST-PORT-PLAN.md, "Remaining port groups"):
//! `update` (update.py), `firmware` (radiofw.py) and `service-daemon` (service.py).
// SPDX-License-Identifier: GPL-2.0-only
mod download;
mod python;
mod storage;
pub mod update;

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

/// `timekeeping.status(ctx)` as a dict (the ported `time` command's value).
fn time_status(root: &Root) -> Map<String, Value> {
    super::time::status(root)
        .ok()
        .and_then(|reply| serde_json::from_str(&reply.stdout).ok())
        .unwrap_or_default()
}

/// `time.time()`.
fn time_wall() -> f64 {
    super::time::wall()
}

/// A value argparse takes for an option: present and not itself an option.
fn option_value(value: &str) -> bool {
    !value.starts_with('-')
}

/// `update <action>` with `--url URL` and `--package DIR` each at most once, in any order, after the
/// action (other spellings are argparse's).
fn update_route(args: &[&str]) -> Option<Command> {
    let (action, rest) = args.split_first()?;
    if !UPDATE_ACTIONS.contains(action) {
        return None;
    }
    let (mut url, mut package) = (None, None);
    let mut rest = rest;
    while let [option, value, tail @ ..] = rest {
        if !option_value(value) {
            return None;
        }
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
        ["update", rest @ ..] => update_route(rest),
        _ => None,
    }
}
