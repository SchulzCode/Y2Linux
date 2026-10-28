//! Group `reports` of the port (docs/planning/PLATFORM-RUST-PORT-PLAN.md, "Remaining port groups"):
//! `report` (report.py: opt-in error reports, the scrubber, the queue and its upload), `reset` and
//! `export-state` (maintenance.py), `export-diagnostics` (diagnostics.py, diagnostic_fields.py).
// SPDX-License-Identifier: GPL-2.0-only
mod build;
mod checks;
mod clock;
mod diagnostics;
mod fields;
mod maintenance;
mod media;
mod pyjson;
mod pyre;
mod scrub;
mod secrets;
mod state;
mod tarball;
mod upload;
mod volume;
mod workload;

use super::Command;
use crate::reply::{Failure, Outcome, Reply};
use pyjson::Py;
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

/// cli.py's `report` branch: OSError and ValueError print `report_failed` with the class name.
fn report(root: &Root, action: &str, to: Option<&str>, topic: Option<&str>) -> Outcome {
    // Inputs Python would crash on (a traceback, not an answer) go to Python before anything changes.
    state::prechecks(root, action)?;
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
        Err(Failure::Fallback) => Err(Failure::Fallback),
        Err(error) => Reply::unsorted(
            &y2p_core::json::Ordered::new()
                .with("failure", "report_failed")
                .with("reason", class(&error)),
            1,
        ),
    }
}

/// `type(error).__name__`.
fn class(error: &Failure) -> &'static str {
    match error {
        Failure::Os { class, .. } => class,
        _ => "ValueError",
    }
}

/// `ctx.json(name)`: Python's json of `ctx.read(name)` (128 KiB), exactly.
pub fn json_at(root: &Root, name: &str) -> Option<Py> {
    pyjson::loads(&root.read(name)?)
}

/// `ctx.json(name, {})`.
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

pub fn boot_id(root: &Root) -> Option<String> {
    root.read("/proc/sys/kernel/random/boot_id")
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
