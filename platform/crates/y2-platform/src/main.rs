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

const PYTHON: &str = "/usr/bin/python3";
const MAIN: &str = "/usr/lib/y2-platform/main.py";
const LIBRARY: &str = "/usr/lib/y2-platform";

fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    // Not UTF-8 (Python would see surrogate escapes): the Python platform parses it.
    let Some(text) = args
        .iter()
        .map(|a| a.to_str())
        .collect::<Option<Vec<&str>>>()
    else {
        return python(&args);
    };
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
