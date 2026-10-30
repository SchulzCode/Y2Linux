//! The storage commands: `media` (the SD card), `internal-media` (Y2Media) and `space` (Y2DATA's
//! budget), each printing its record as sorted JSON with its own exit-code rule.
//! `/usr/sbin/y2-media` is the overlay's shell wrapper (`exec y2-platform media "$@"`).
//!
//! `route` receives the canonical form of the parsed command line (cli.rs): the positional action,
//! then the options given with their full names, `--opt value` as two tokens, flags bare and ints in
//! decimal; help and argument errors never reach it.
// SPDX-License-Identifier: GPL-2.0-only
pub(crate) mod internal;
pub(crate) mod media;
pub(crate) mod observe;
pub(crate) mod records;
mod space;
pub(crate) use space::policy as space_policy;
pub(crate) mod sys;

use super::Command;
use crate::reply::{Outcome, Reply};
use std::collections::BTreeMap;

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
                // Only the clamp to 1..WRITE_HOLD_MAX reads it, so an int beyond 64 bits clamps
                // as its saturated value does.
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
                .map(|s| s.to_string());
            let (automatic, shown, migrate) = (
                parsed.flag("--automatic"),
                parsed.flag("--confirm-shown"),
                !parsed.flag("--no-migrate"),
            );
            Some(Box::new(move |root| {
                let marker = |marker: internal::Marker, failed: fn(&str) -> bool| {
                    Reply::sorted(&marker, u8::from(failed(marker.state)))
                };
                match *action {
                    "status" => Reply::sorted(&internal::status(root), 0),
                    "provision" => marker(
                        internal::provision(root, confirm.as_deref(), migrate)?,
                        |state| state != "Ready",
                    ),
                    "setup" => {
                        let serial = if shown {
                            internal::shown_serial(root)
                        } else {
                            confirm.clone()
                        };
                        let note = internal::setup(root, automatic, serial.as_deref())?;
                        let failed = matches!(
                            note.state,
                            internal::SetupState::Failed | internal::SetupState::Unavailable
                        );
                        Reply::sorted(&note, u8::from(failed))
                    }
                    "mount" => marker(internal::mount(root)?, |state| state != "Ready"),
                    _ => marker(internal::unmount(root)?, |state| state == "Failed"),
                }
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

/// The exit code of a `media` action. `write` fails whenever it sets `failure` (not_mounted
/// included), so Reborn's window call sees a refusal as one; `status` reports the card in any state
/// (a Degraded card mounted read-only is an answer, not a failure). A lifecycle record without
/// `state` (an unchanged media-lifecycle.json that has none) is no answer: exit 1.
fn media_exit(action: &str, state: Option<&str>) -> u8 {
    let failed = match state {
        None => true,
        Some(state) => {
            matches!(state, "Failed" | "Degraded")
                || (["mount", "format", "grow", "repair"].contains(&action) && state != "Ready")
        }
    };
    u8::from(failed)
}

fn media_command(root: &y2p_core::Root, request: &MediaRequest) -> Outcome {
    let action = request.action;
    let mut result = match action {
        "status" => return Reply::sorted(&media::status(root)?, 0),
        "write" => {
            let mut answer =
                media::write_window(root, &request.holder, request.seconds, request.release)?;
            if answer.state != "Ready" {
                answer.failure = Some(
                    answer
                        .reason
                        .filter(|r| !r.is_empty())
                        .unwrap_or("write_window_failed"),
                );
            }
            return Reply::sorted(&answer, u8::from(answer.failure.is_some()));
        }
        "format" | "grow" => media::change_card(root, action, request.confirm_cid.as_deref())?,
        "repair" => media::repair(root)?,
        "verify" => media::verify(root)?,
        "reconcile" => media::reconcile(root)?,
        _ => media::operation(root, action)?,
    };
    let state = result.state.clone().filter(|s| !s.is_empty());
    if action == "repair" && state.as_deref() != Some("Ready") {
        // Reborn reads only `failure` from a refused command (client.rs matches it).
        let errors_left = state.as_deref() == Some("Degraded")
            && result
                .read_only()
                .is_some_and(|r| r != records::ReadOnly::ReadWrite);
        result.failure = Some(if errors_left {
            "errors_left".into()
        } else {
            result
                .reason
                .clone()
                .filter(|r| !r.is_empty())
                .unwrap_or_else(|| "repair_failed".into())
        });
    }
    Reply::sorted(&result, media_exit(action, result.state.as_deref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_actions_keep_their_rules() {
        assert_eq!(media_exit("reconcile", Some("Degraded")), 1);
        assert_eq!(media_exit("reconcile", Some("Unavailable")), 0);
        assert_eq!(media_exit("mount", Some("Unavailable")), 1);
        assert_eq!(media_exit("verify", None), 1);
    }
}
