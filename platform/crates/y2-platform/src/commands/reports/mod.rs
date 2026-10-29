//! The reports group: `report` (opt-in error reports, the scrubber, the queue and its upload),
//! `reset` and `export-state` (the owner's maintenance), `export-diagnostics` (the tester export).
// SPDX-License-Identifier: GPL-2.0-only
mod build;
mod checks;
mod clock;
mod diagnostics;
mod fields;
mod maintenance;
pub(crate) mod media;
pub(crate) use y2p_core::pyjson;
mod pyops;
mod pyre;
mod scrub;
mod secrets;
pub(crate) mod state;
mod tarball;
#[cfg(test)]
mod test_player;
mod upload;
mod volume;

use super::Command;
use crate::reply::{Failure, Outcome, Reply};
use pyjson::Py;
use y2p_core::json::ObjectExt;
use y2p_core::Root;

const TOPICS: [&str; 7] = [
    "sound",
    "display",
    "battery",
    "bluetooth",
    "wifi",
    "crash",
    "other",
];
const SCOPES: [&str; 6] = [
    "settings",
    "network",
    "bluetooth-bonds",
    "library",
    "caches",
    "full-user",
];

/// The canonical form's options after the positional (argparse.rs: given ones only, full names, each
/// once): `--name value` pairs and bare `--flag`s; `None` for anything else.
fn options<'a>(
    rest: &[&'a str],
    valued: &[(&str, Option<&[&str]>)],
    flags: &[&str],
) -> Option<Vec<(String, Option<&'a str>)>> {
    let mut found: Vec<(String, Option<&'a str>)> = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        let name = rest[i];
        if found.iter().any(|(n, _)| n == name) {
            return None;
        }
        if flags.contains(&name) {
            found.push((name.to_owned(), None));
            i += 1;
            continue;
        }
        let (_, choices) = valued.iter().find(|(n, _)| *n == name)?;
        let value = *rest.get(i + 1)?;
        if choices.is_some_and(|c| !c.contains(&value)) {
            return None;
        }
        found.push((name.to_owned(), Some(value)));
        i += 2;
    }
    Some(found)
}

fn get<'a>(found: &[(String, Option<&'a str>)], name: &str) -> Option<&'a str> {
    found.iter().find(|(n, _)| n == name).and_then(|(_, v)| *v)
}

fn has(found: &[(String, Option<&str>)], name: &str) -> bool {
    found.iter().any(|(n, _)| n == name)
}

/// The Rust implementation of these exact arguments, if this group has one.
pub fn route(args: &[&str]) -> Option<Command> {
    match args {
        ["report", action, rest @ ..] => {
            const ACTIONS: [&str; 9] = [
                "status", "enable", "disable", "new-id", "send", "save", "check", "upload", "work",
            ];
            if !ACTIONS.contains(action) {
                return None;
            }
            let found = options(
                rest,
                &[
                    ("--to", Some(&["internal", "sd"][..])),
                    ("--topic", Some(&TOPICS[..])),
                ],
                &[],
            )?;
            let action = action.to_string();
            let to = get(&found, "--to").map(str::to_owned);
            let topic = get(&found, "--topic").map(str::to_owned);
            Some(Box::new(move |root| {
                report(root, &action, to.as_deref(), topic.as_deref())
            }))
        }
        ["reset", action, rest @ ..] => {
            if !["plan", "execute", "resume", "purge"].contains(action) {
                return None;
            }
            let found = options(
                rest,
                &[
                    ("--scope", Some(&SCOPES[..])),
                    ("--confirm", None),
                    ("--id", None),
                ],
                &["--erase-user-music"],
            )?;
            let action = action.to_string();
            let scope = get(&found, "--scope").map(str::to_owned);
            let confirm = get(&found, "--confirm").map(str::to_owned);
            let id = get(&found, "--id").map(str::to_owned);
            let erase = has(&found, "--erase-user-music");
            Some(Box::new(move |root| {
                maintenance::reset(
                    root,
                    &action,
                    scope.as_deref(),
                    confirm.as_deref(),
                    id.as_deref(),
                    erase,
                )
            }))
        }
        ["export-state", rest @ ..] => {
            let found = options(rest, &[], &["--include-database", "--include-network"])?;
            let database = has(&found, "--include-database");
            let network = has(&found, "--include-network");
            Some(Box::new(move |root| {
                maintenance::export_state(root, database, network)
            }))
        }
        ["export-diagnostics"] => Some(Box::new(diagnostics::export)),
        _ => None,
    }
}

/// `report.lower_priority`: building a report keeps every core busy for ≈10 s: never at the cost of
/// playback or the screen (the children inherit this).
fn lower_priority() {
    // SAFETY: nice(2) only changes this process's priority.
    unsafe {
        libc::nice(10);
    }
}

/// Refusals of a record of an unexpected shape (each was a traceback of the Python platform):
/// reports.json, a queued report's report.json, Reborn's health answer, power.json's battery,
/// Reborn's session.json.
pub const STATE_UNREADABLE: &str = "report_state_unreadable";
pub const QUEUED_UNREADABLE: &str = "queued_report_unreadable";
pub const HEALTH_UNREADABLE: &str = "reborn_health_unreadable";
pub const POWER_UNREADABLE: &str = "power_record_unreadable";
pub const SESSION_UNREADABLE: &str = "session_unreadable";
const UNREADABLE: [&str; 5] = [
    STATE_UNREADABLE,
    QUEUED_UNREADABLE,
    HEALTH_UNREADABLE,
    POWER_UNREADABLE,
    SESSION_UNREADABLE,
];

/// `report <action>`: every failure prints `report_failed` (exit 1) with a reason.
fn report(root: &Root, action: &str, to: Option<&str>, topic: Option<&str>) -> Outcome {
    if matches!(action, "send" | "save" | "check" | "work") {
        lower_priority();
    }
    let result = match action {
        "status" => state::status(root),
        "enable" | "disable" => state::set_enabled(root, action == "enable"),
        "new-id" => state::new_install_id(root),
        "send" => upload::send(root, topic),
        "save" => match to {
            None => Err(Failure::value("save_needs_to")),
            Some(to) => {
                let progress = build::Progress::new(root, Some(to));
                let result = media::save_to(root, to, topic, &progress);
                progress.close();
                result
            }
        },
        "upload" => upload::upload(root, None, None),
        "work" => checks::run_checks(root).map(|reports| {
            Py::dict()
                .with("reports", reports)
                .with("upload", upload::upload_now(root, None, None))
        }),
        _ => checks::run_checks(root).map(|reports| Py::dict().with("reports", reports)),
    };
    match result {
        Ok(value) => Ok(Reply::text(pyjson::dumps(&value, pyjson::Style::Spaced))),
        Err(error) => Reply::unsorted(
            &y2p_core::json::Object::new()
                .with("failure", "report_failed")
                .with("reason", reason(&error)),
            1,
        ),
    }
}

/// A failed report's `reason`: an unreadable record's refusal by its name, any other error by its
/// class (`type(error).__name__`, as the Python platform printed it).
fn reason(error: &Failure) -> &str {
    match error {
        Failure::Value(text) if UNREADABLE.contains(&text.as_str()) => text,
        Failure::Os { class, .. } | Failure::Crash { class, .. } => class,
        Failure::Value(_) => "ValueError",
    }
}

/// The SHA-256 of `data` in lowercase hex: report and panic signatures, file digests, the reset
/// plan's confirmation.
pub fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A JSON record of at most 128 KiB, read with Python's json rules (integers of any size, NaN,
/// duplicate keys): the records were written by Python and are compared with what was read before.
pub fn json_at(root: &Root, name: &str) -> Option<Py> {
    pyjson::loads(&root.read(name)?)
}

/// A JSON record that is an object, `{}` for anything else.
pub fn json_dict(root: &Root, name: &str) -> Py {
    match json_at(root, name) {
        Some(value @ Py::Dict(_)) => value,
        _ => Py::dict(),
    }
}

/// `value or {}` of a dict default.
pub fn or_dict(value: Py) -> Py {
    if value.truthy() {
        value
    } else {
        Py::dict()
    }
}

/// An OSError raised by a call on `path` (the class follows errno).
pub fn os(error: std::io::Error, path: &std::path::Path) -> Failure {
    Failure::os(error, path)
}

/// An OSError Python raises without a file name (`os.fstat`, `os.fsync`, a dir_fd call on a name).
pub fn os_plain(error: std::io::Error) -> Failure {
    let (text, class) = y2p_core::py::os_error(&error, None);
    Failure::Os { text, class }
}

/// An OSError of a `dir_fd` call: Python names the relative name.
pub fn os_name(error: std::io::Error, name: &str) -> Failure {
    let (text, class) = y2p_core::py::os_error(&error, Some(name));
    Failure::Os { text, class }
}

#[cfg(test)]
mod cli_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_well_formed_arguments_run_here() {
        for args in [
            &["report", "status"][..],
            &["report", "save", "--to", "sd", "--topic", "wifi"],
            &["report", "send", "--topic", "crash"],
            &["reset", "plan", "--scope", "settings"],
            &["reset", "execute", "--confirm", "-x"],
            &["reset", "execute", "--confirm", "abc", "--erase-user-music"],
            &["reset", "purge", "--id", "", "--confirm", "x"],
            &["export-state", "--include-network", "--include-database"],
            &["export-diagnostics"],
        ] {
            assert!(route(args).is_some(), "{args:?}");
        }
        for args in [
            &["report"][..],
            &["report", "nope"],
            &["report", "save", "--to", "usb"],
            &["report", "save", "--to"],
            &["report", "save", "--to", "sd", "--to", "sd"],
            &["report", "status", "--help"],
            &["reset", "plan", "--scope", "everything"],
            &["export-state", "--include-database", "--include-database"],
            &["export-diagnostics", "x"],
        ] {
            assert!(route(args).is_none(), "{args:?}");
        }
    }
}
