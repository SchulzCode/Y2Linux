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
pub use serde_json::Value;
pub use y2p_core::digest::sha256_hex;
use y2p_core::json::ObjectExt;
pub use y2p_core::json::{object, ValueExt};
use y2p_core::Root;

const SCOPES: [&str; 6] = [
    "settings",
    "network",
    "bluetooth-bonds",
    "library",
    "caches",
    "full-user",
];

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
            let found = crate::cli::given("report", rest)?;
            let action = action.to_string();
            let to = found.value("--to").map(str::to_owned);
            let topic = found.value("--topic").map(str::to_owned);
            Some(Box::new(move |root| {
                report(root, &action, to.as_deref(), topic.as_deref())
            }))
        }
        ["reset", action, rest @ ..] => {
            if !["plan", "execute", "resume", "purge"].contains(action) {
                return None;
            }
            let found = crate::cli::given("reset", rest)?;
            let action = action.to_string();
            let scope = found.value("--scope").map(str::to_owned);
            let confirm = found.value("--confirm").map(str::to_owned);
            let id = found.value("--id").map(str::to_owned);
            let erase = found.flag("--erase-user-music");
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
            let found = crate::cli::given("export-state", rest)?;
            let database = found.flag("--include-database");
            let network = found.flag("--include-network");
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

/// Refusals of a record of an unexpected shape: Reborn's health answer, power.json's battery,
/// Reborn's session.json (reports.json and a queued report's report.json read leniently).
pub const SESSION_UNREADABLE: &str = "session_unreadable";
const UNREADABLE: [&str; 1] = [SESSION_UNREADABLE];

/// `report <action>`: every failure prints `report_failed` (exit 1) with a reason.
fn report(root: &Root, action: &str, to: Option<&str>, topic: Option<&str>) -> Outcome {
    if matches!(action, "send" | "save" | "check" | "work") {
        lower_priority();
    }
    let result = match action {
        "status" => state::status(root),
        "enable" | "disable" => state::set_enabled(root, action == "enable"),
        "new-id" => state::new_install_id(root),
        "send" => upload::send(root, topic).map(|queued| serde_json::json!(queued)),
        "save" => match to {
            None => Err(Failure::value("save_needs_to")),
            Some(to) => {
                let progress = build::Progress::new(root, Some(to));
                let result = media::save_to(root, to, topic, &progress);
                progress.close();
                result
            }
        },
        "upload" => upload::upload(root, None, None).map(|run| serde_json::json!(run)),
        "work" => checks::run_checks(root).map(|reports| {
            serde_json::json!({"reports": reports, "upload": upload::upload_now(root, None, None)})
        }),
        _ => checks::run_checks(root).map(|reports| serde_json::json!({ "reports": reports })),
    };
    match result {
        Ok(value) => Ok(Reply::text(
            serde_json::to_string(&value).unwrap_or_default(),
        )),
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

/// A JSON record of at most 128 KiB, read with Python's json rules (integers of any size, NaN,
/// duplicate keys): the records were written by Python and are compared with what was read before.
pub fn json_at(root: &Root, name: &str) -> Option<Value> {
    loads_text(&root.read(name)?)
}

/// A field that holds something: not null, not false, not an empty text, list or object (a number
/// always does).
pub fn filled(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => false,
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
        _ => true,
    }
}

/// A JSON document, or none.
pub fn loads_text(text: &str) -> Option<Value> {
    serde_json::from_str(text).ok()
}

/// A JSON document of bytes, or none.
pub fn loads_bytes(raw: &[u8]) -> Option<Value> {
    serde_json::from_slice(raw).ok()
}

/// A JSON record that is an object, `{}` for anything else.
pub fn json_dict(root: &Root, name: &str) -> Value {
    match json_at(root, name) {
        Some(value @ Value::Object(_)) => value,
        _ => object(),
    }
}

/// An OSError raised by a call on `path` (the class follows errno).
pub fn os(error: std::io::Error, path: &std::path::Path) -> Failure {
    Failure::os(error, path)
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
        // A repeated option counts once with its last value (cli.rs).
        for args in [
            &["report", "save", "--to", "sd", "--to", "sd"][..],
            &["export-state", "--include-database", "--include-database"],
        ] {
            assert!(
                matches!(crate::cli::parse(args), crate::cli::Outcome::Parsed(_)),
                "{args:?}"
            );
        }
        for args in [
            &["report"][..],
            &["report", "nope"],
            &["report", "save", "--to", "usb"],
            &["report", "save", "--to"],
            &["report", "status", "--help"],
            &["reset", "plan", "--scope", "everything"],
            &["export-diagnostics", "x"],
        ] {
            // Refused by cli::parse (exit 2) or by the route: never run.
            let runs = match crate::cli::parse(args) {
                crate::cli::Outcome::Parsed(canonical) => {
                    route(&canonical.iter().map(String::as_str).collect::<Vec<_>>()).is_some()
                }
                _ => false,
            };
            assert!(!runs, "{args:?}");
        }
    }
}
