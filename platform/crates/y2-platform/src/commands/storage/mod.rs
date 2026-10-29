//! Group `storage` of the port (docs/planning/PLATFORM-RUST-PORT-PLAN.md, "Remaining port groups"):
//! `media` (every action), `internal-media` (every action) and `space`, with cli.main's printing and
//! exit-code rules. `/usr/sbin/y2-media` stays the overlay shell wrapper (`exec y2-platform media "$@"`).
//!
//! `route` receives the canonical form of the parsed command line (the dispatcher's argparse.rs): the
//! positional action, then the options given with their full names, `--opt value` as two tokens, flags
//! bare and ints as `str(int())`; argparse's help and errors never reach it.
// SPDX-License-Identifier: GPL-2.0-only
pub(crate) mod internal;
pub(crate) mod media;
pub(crate) mod observe;
mod space;
pub(crate) mod sys;

use super::Command;
use crate::reply::{Outcome, Reply};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use sys::{get, Object};
use y2p_core::py::truthy;

/// The parsed tokens of one subcommand: positionals, options with their value, flags.
struct Parsed<'a> {
    positionals: Vec<&'a str>,
    options: BTreeMap<&'static str, &'a str>,
    flags: Vec<&'static str>,
}

fn parse<'a>(
    args: &[&'a str],
    flags: &[&'static str],
    options: &[&'static str],
) -> Option<Parsed<'a>> {
    let mut parsed = Parsed {
        positionals: Vec::new(),
        options: BTreeMap::new(),
        flags: Vec::new(),
    };
    let mut tokens = args.iter();
    while let Some(&token) = tokens.next() {
        if let Some(flag) = flags.iter().find(|f| **f == token) {
            parsed.flags.push(flag);
        } else if let Some(option) = options.iter().find(|o| **o == token) {
            parsed.options.insert(option, tokens.next()?);
        } else {
            parsed.positionals.push(token);
        }
    }
    Some(parsed)
}

impl Parsed<'_> {
    fn flag(&self, name: &str) -> bool {
        self.flags.contains(&name)
    }
}

const MEDIA_ACTIONS: [&str; 10] = [
    "mount",
    "unmount",
    "check",
    "status",
    "reconcile",
    "verify",
    "format",
    "grow",
    "repair",
    "write",
];
const INTERNAL_ACTIONS: [&str; 5] = ["status", "mount", "unmount", "provision", "setup"];

/// The Rust implementation of these exact arguments, if this group has one.
pub fn route(args: &[&str]) -> Option<Command> {
    let (command, rest) = args.split_first()?;
    match *command {
        "media" => {
            let parsed = parse(
                rest,
                &["--release", "--json"],
                &["--holder", "--seconds", "--confirm-cid"],
            )?;
            let [action] = parsed.positionals[..] else {
                return None;
            };
            let action = MEDIA_ACTIONS.iter().find(|a| **a == action)?;
            let holder = parsed.options.get("--holder").copied().unwrap_or("reborn");
            if !["reborn", "report", "radiofw", "usb"].contains(&holder) {
                return None;
            }
            let seconds = match parsed.options.get("--seconds") {
                // `str(int())` from argparse; only `max(1, min(seconds, WRITE_HOLD_MAX))` reads it,
                // so an int beyond 64 bits clamps as its saturated value does.
                Some(text) => text.parse::<i64>().unwrap_or(if text.starts_with('-') {
                    i64::MIN
                } else {
                    i64::MAX
                }),
                None => 60,
            };
            let request = MediaRequest {
                action,
                holder: holder.to_owned(),
                seconds,
                release: parsed.flag("--release"),
                confirm_cid: parsed.options.get("--confirm-cid").map(|c| c.to_string()),
            };
            Some(Box::new(move |root| media_command(root, &request)))
        }
        "internal-media" => {
            let parsed = parse(
                rest,
                &["--automatic", "--confirm-shown", "--no-migrate"],
                &["--confirm-serial"],
            )?;
            let [action] = parsed.positionals[..] else {
                return None;
            };
            let action = INTERNAL_ACTIONS.iter().find(|a| **a == action)?;
            let confirm = parsed
                .options
                .get("--confirm-serial")
                .map_or(Value::Null, |s| json!(s));
            let (automatic, shown, migrate) = (
                parsed.flag("--automatic"),
                parsed.flag("--confirm-shown"),
                !parsed.flag("--no-migrate"),
            );
            Some(Box::new(move |root| {
                let result = match *action {
                    "status" => internal::status(root),
                    "provision" => internal::provision(root, &confirm, migrate)?,
                    "setup" => {
                        let serial = if shown {
                            internal::shown_serial(root)
                        } else {
                            confirm.clone()
                        };
                        internal::setup(root, automatic, &serial)?
                    }
                    "mount" => internal::mount(root)?,
                    _ => internal::unmount(root)?,
                };
                let state = get(&result, "state").as_str();
                let failed = match *action {
                    "mount" | "provision" => state != Some("Ready"),
                    "setup" => matches!(state, Some("Failed" | "Unavailable")),
                    "unmount" => state == Some("Failed"),
                    _ => false,
                };
                Reply::sorted(&result, u8::from(failed))
            }))
        }
        "space" => {
            let parsed = parse(rest, &["--cleanup"], &[])?;
            if !parsed.positionals.is_empty() {
                return None;
            }
            let cleanup = parsed.flag("--cleanup");
            Some(Box::new(move |root| {
                Reply::sorted(&space::policy(root, cleanup)?, 0)
            }))
        }
        _ => None,
    }
}

struct MediaRequest {
    action: &'static str,
    holder: String,
    seconds: i64,
    release: bool,
    confirm_cid: Option<String>,
}

/// The exit code of a `media` action. `status` reports the card in any state (a Degraded card
/// mounted read-only is an answer, not a failure); `write` fails whenever it sets `failure`
/// (not_mounted included), so Reborn's window call sees a refusal as one. A lifecycle record
/// without `state` exits 1 (Python's KeyError).
fn media_exit(action: &str, result: &Object) -> u8 {
    let failed = match result.get("state") {
        None => true,
        Some(_) if action == "status" => false,
        Some(_) if action == "write" => result.contains_key("failure"),
        Some(state) => {
            let state = state.as_str();
            matches!(state, Some("Failed" | "Degraded"))
                || (["mount", "format", "grow", "repair"].contains(&action)
                    && state != Some("Ready"))
        }
    };
    u8::from(failed)
}

fn media_command(root: &y2p_core::Root, request: &MediaRequest) -> Outcome {
    let action = request.action;
    let result: Object = match action {
        "format" | "grow" => media::change_card(root, action, request.confirm_cid.as_deref())?,
        "repair" => {
            let mut result = media::repair(root)?;
            if get(&result, "state").as_str() != Some("Ready") {
                // Reborn reads only `failure` from a refused command.
                let failure = if get(&result, "state").as_str() == Some("Degraded")
                    && truthy(get(&result, "read_only"))
                {
                    json!("errors_left")
                } else if truthy(get(&result, "reason")) {
                    get(&result, "reason").clone()
                } else {
                    json!("repair_failed")
                };
                result.insert("failure".into(), failure);
            }
            result
        }
        "write" => {
            let mut result =
                media::write_window(root, &request.holder, request.seconds, request.release)?;
            if get(&result, "state").as_str() != Some("Ready") {
                let failure = if truthy(get(&result, "reason")) {
                    get(&result, "reason").clone()
                } else {
                    json!("write_window_failed")
                };
                result.insert("failure".into(), failure);
            }
            result
        }
        "verify" => media::verify(root)?,
        "reconcile" => media::reconcile(root)?,
        _ => media::operation(root, action)?,
    };
    let code = media_exit(action, &result);

    Reply::sorted(&result, code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(value: serde_json::Value) -> Object {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn status_answers_for_every_card_state() {
        for state in ["Ready", "Unavailable", "Degraded", "Failed"] {
            assert_eq!(
                media_exit("status", &record(json!({"state": state}))),
                0,
                "{state}"
            );
        }
    }

    #[test]
    fn a_refused_write_window_exits_1() {
        let refused =
            json!({"state": "Unavailable", "reason": "not_mounted", "failure": "not_mounted"});
        assert_eq!(media_exit("write", &record(refused)), 1);
        let degraded =
            json!({"state": "Degraded", "reason": "card_has_errors", "failure": "card_has_errors"});
        assert_eq!(media_exit("write", &record(degraded)), 1);
        assert_eq!(
            media_exit(
                "write",
                &record(json!({"state": "Ready", "writable": true}))
            ),
            0
        );
    }

    #[test]
    fn lifecycle_actions_keep_their_rules() {
        assert_eq!(
            media_exit("reconcile", &record(json!({"state": "Degraded"}))),
            1
        );
        assert_eq!(
            media_exit("reconcile", &record(json!({"state": "Unavailable"}))),
            0
        );
        assert_eq!(
            media_exit("mount", &record(json!({"state": "Unavailable"}))),
            1
        );
        assert_eq!(media_exit("verify", &record(json!({}))), 1);
    }
}
