//! `/usr/sbin/y2-platform`, the strangler dispatcher (docs/planning/PLATFORM-RUST-PORT-PLAN.md §3.1).
//! The arguments are parsed first, as `main.py`'s argparse parser does (argparse.rs): `--help` prints
//! that parser's help (exit 0), an argument error its usage and message on stderr (exit 2). A parsed
//! command goes to its Rust implementation in its canonical spelling (`argparse::Parsed::canonical`);
//! every other one replaces this process with the Python platform, with the arguments as given, exactly
//! as the shell wrapper it replaces did:
//! `exec /usr/bin/python3 -I /usr/lib/y2-platform/main.py "$@"` with `PYTHONPATH=/usr/lib/y2-platform`
//! and `PYTHONDONTWRITEBYTECODE=1`. Same process (callers signal and wait for this PID: Reborn sends
//! SIGTERM to `update stage`), same argv and environment for Python, same stdin, stdout and stderr.
// SPDX-License-Identifier: GPL-2.0-only
mod argparse;
mod commands;
mod reply;

use reply::Failure;
use std::{
    env,
    ffi::OsString,
    io::Write,
    os::unix::process::CommandExt,
    process::{Command, ExitCode},
};
use y2p_core::json::{dumps, Separators};

/// Whether this build may hand a command line to the Python platform (cargo feature `python-fallback`,
/// on by default until phase 6).
pub const PYTHON_FALLBACK: bool = cfg!(feature = "python-fallback");
const PYTHON: &str = "/usr/bin/python3";
const MAIN: &str = "/usr/lib/y2-platform/main.py";
const LIBRARY: &str = "/usr/lib/y2-platform";

fn main() -> ExitCode {
    let mut args: Vec<OsString> = env::args_os().skip(1).collect();
    // The programmes this binary also is, by the name it was started under (links to
    // /usr/sbin/y2-platform: y2-status, y2-health, y2-suspend-record, y2-power-audit, poweroff, reboot).
    let name = env::args_os()
        .next()
        .map(std::path::PathBuf::from)
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
    if let Some(mapped) = name
        .as_deref()
        .and_then(|n| commands::suspend::program(n, &args))
    {
        args = mapped;
    }
    // Not UTF-8 (Python would see surrogate escapes): the Python platform parses it.
    let Some(text) = args
        .iter()
        .map(|a| a.to_str())
        .collect::<Option<Vec<&str>>>()
    else {
        return python(&args);
    };
    // Internal programmes (the sleep helper, suspend records, the power audit and power command): not
    // main.py subcommands, so no argparse; each runs and exits itself.
    if let Some(programme) = commands::suspend::programme(&text) {
        let root = y2p_core::Root::from_env();
        return match programme(&root) {
            Ok(reply) => print(
                &mut std::io::stdout().lock(),
                &format!("{}\n", reply.stdout),
                reply.code,
            ),
            Err(error) => print(
                &mut std::io::stderr().lock(),
                &format!("{}: {}\n", text[0], error.message()),
                1,
            ),
        };
    }
    let parsed = match argparse::parse(&text) {
        argparse::Outcome::Help(help) => return print(&mut std::io::stdout().lock(), &help, 0),
        argparse::Outcome::Error(error) => return print(&mut std::io::stderr().lock(), &error, 2),
        argparse::Outcome::Parsed(parsed) => parsed,
    };
    if env::var_os("Y2_ROOT").is_some() && env::var_os("Y2_ARGPARSE_ECHO").is_some() {
        // tests/test_platform_argparse.py: the parse itself, compared with Python's namespace.
        let echo = serde_json::json!({"namespace": parsed.typed(), "canonical": parsed.canonical});
        return print(&mut std::io::stdout().lock(), &format!("{echo}\n"), 0);
    }
    let canonical: Vec<&str> = parsed.canonical.iter().map(String::as_str).collect();
    let Some(command) = commands::route(&canonical) else {
        return python(&args);
    };
    let root = y2p_core::Root::from_env();
    let (stdout, code) = match command(&root) {
        Ok(reply) => (reply.stdout, reply.code),
        Err(Failure::Fallback) => return python(&args),
        // An exception main.py does not catch: its traceback on stderr, nothing on stdout, exit 1.
        Err(error @ Failure::Crash { .. }) => {
            return print(&mut std::io::stderr().lock(), &error.traceback(), 1)
        }
        // main.py: an OSError, ValueError or RuntimeError no command caught.
        Err(error) => (
            dumps(&error.record(), Separators::Spaced).expect("record"),
            1,
        ),
    };
    let mut out = std::io::stdout().lock();
    // print(): a closed pipe after the work is done changes nothing (Python reports it on stderr).
    let _ = out
        .write_all(stdout.as_bytes())
        .and_then(|()| out.write_all(b"\n"))
        .and_then(|()| out.flush());
    ExitCode::from(code)
}

/// argparse's `_print_message` then `exit(code)`: a failed write is ignored.
fn print(out: &mut dyn Write, text: &str, code: u8) -> ExitCode {
    let _ = out.write_all(text.as_bytes()).and_then(|()| out.flush());
    ExitCode::from(code)
}

/// The Python interpreter and entry. A host test (`Y2_ROOT` set, tests/platform_differential.py) may
/// name its own, so the fallback runs the Python platform on the same fixture root.
fn fallback() -> (OsString, OsString) {
    let test = env::var_os("Y2_ROOT").is_some();
    let pick = |name, default: &str| {
        env::var_os(name)
            .filter(|_| test)
            .unwrap_or_else(|| default.into())
    };
    (
        pick("Y2_PLATFORM_PYTHON", PYTHON),
        pick("Y2_PLATFORM_MAIN", MAIN),
    )
}

fn python(args: &[OsString]) -> ExitCode {
    if !PYTHON_FALLBACK {
        // Built without the Python platform: every command line argparse takes has a route (the test
        // below), so this is a command no build of this binary implements.
        let _ = writeln!(
            std::io::stderr(),
            "y2-platform: {}: no Rust implementation and no Python platform",
            args.first()
                .map(|a| a.to_string_lossy())
                .unwrap_or_default()
        );
        return ExitCode::from(127);
    }
    let (interpreter, main) = fallback();
    let error = Command::new(&interpreter)
        .arg("-I")
        .arg(&main)
        .args(args)
        .env("PYTHONPATH", LIBRARY)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .exec();
    // As the shell's failed `exec`: a message on stderr, nothing on stdout, 127 (126 when not executable).
    let _ = writeln!(
        std::io::stderr(),
        "y2-platform: {}: {error}",
        interpreter.to_string_lossy()
    );
    ExitCode::from(if error.kind() == std::io::ErrorKind::PermissionDenied {
        126
    } else {
        127
    })
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    /// The service group's commands still being ported (another branch): until they land these
    /// command lines go to Python. Remove each as its route arrives.
    const KNOWN_UNROUTED: &[&str] = &["service-daemon"];

    /// One sample value argparse takes for an action (its first choice, a number, or a word).
    fn sample(action: &Value) -> String {
        if let Some(choice) = action["choices"].as_array().and_then(|c| c.first()) {
            return match choice {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
        }
        match action["type"].as_str() {
            Some("int") => "1".into(),
            Some("float") => "1.5".into(),
            _ => "x".into(),
        }
    }

    /// Every command line of argparse.json's parser, as far as routes tell them apart: each subcommand,
    /// each choice of its first positional with choices (the action), the other positionals with one
    /// sample (an optional positional also left out), alone and with each of its options once.
    fn command_lines() -> Vec<(String, Vec<String>)> {
        let spec: Value =
            serde_json::from_str(include_str!("argparse.json")).expect("argparse.json");
        let mut lines = Vec::new();
        for entry in spec["commands"].as_array().expect("commands") {
            let name = entry[0].as_str().expect("name").to_owned();
            let actions = entry[1]["actions"].as_array().expect("actions");
            let named = |a: &&Value| a["options"].as_array().is_some_and(|o| !o.is_empty());
            let positionals: Vec<&Value> = actions
                .iter()
                .filter(|a| !named(a) && a["kind"] != "help")
                .collect();
            let optionals: Vec<&Value> = actions
                .iter()
                .filter(|a| named(a) && a["kind"] != "help")
                .collect();
            let chooser = positionals.iter().position(|a| a["choices"].is_array());
            let choices: Vec<Option<String>> = match chooser {
                Some(i) => positionals[i]["choices"]
                    .as_array()
                    .expect("choices")
                    .iter()
                    .map(|c| c.as_str().map(str::to_owned))
                    .collect(),
                None => vec![None],
            };
            for choice in choices {
                let mut full = vec![name.clone()];
                let mut short = vec![name.clone()];
                for (i, positional) in positionals.iter().enumerate() {
                    let value = match (Some(i) == chooser, &choice) {
                        (true, Some(c)) => c.clone(),
                        _ => sample(positional),
                    };
                    full.push(value.clone());
                    if positional["nargs"] != "?" || Some(i) == chooser {
                        short.push(value);
                    }
                }
                let label = match &choice {
                    Some(c) => format!("{name} {c}"),
                    None => name.clone(),
                };
                for base in [&full, &short] {
                    lines.push((label.clone(), base.clone()));
                    for optional in &optionals {
                        let mut line = base.clone();
                        let option = optional["options"]
                            .as_array()
                            .and_then(|o| o.iter().rev().find_map(Value::as_str))
                            .expect("option")
                            .to_owned();
                        line.push(option);
                        if optional["kind"] != "store_true" {
                            line.push(sample(optional));
                        }
                        lines.push((label.clone(), line));
                    }
                }
            }
        }
        lines
    }

    #[test]
    fn every_command_line_argparse_takes_has_a_route() {
        let mut unrouted = std::collections::BTreeSet::new();
        let mut missing = Vec::new();
        let mut parsed = 0;
        for (label, line) in command_lines() {
            let args: Vec<&str> = line.iter().map(String::as_str).collect();
            let crate::argparse::Outcome::Parsed(result) = crate::argparse::parse(&args) else {
                continue; // a combination argparse refuses (a required option missing)
            };
            parsed += 1;
            let canonical: Vec<&str> = result.canonical.iter().map(String::as_str).collect();
            if crate::commands::route(&canonical).is_none() {
                if !KNOWN_UNROUTED.contains(&label.as_str()) {
                    missing.push(format!("{canonical:?}"));
                }
                unrouted.insert(label);
            }
        }
        assert!(parsed > 300, "only {parsed} command lines parsed");
        assert!(missing.is_empty(), "no route for {missing:#?}");
        if !super::PYTHON_FALLBACK {
            // Built without the Python platform: nothing may be left for it.
            assert!(
                unrouted.is_empty(),
                "no route (and no Python) for {unrouted:?}"
            );
        }
    }
}
