//! `/usr/sbin/y2-platform`, the strangler dispatcher (docs/planning/PLATFORM-RUST-PORT-PLAN.md §3.1).
//! A subcommand ported to Rust runs here; every other one replaces this process with the Python
//! platform, exactly as the shell wrapper it replaces did:
//! `exec /usr/bin/python3 -I /usr/lib/y2-platform/main.py "$@"` with `PYTHONPATH=/usr/lib/y2-platform`
//! and `PYTHONDONTWRITEBYTECODE=1`. Same process (callers signal and wait for this PID: Reborn sends
//! SIGTERM to `update stage`), same argv and environment for Python, same stdin, stdout and stderr.
// SPDX-License-Identifier: GPL-2.0-only
use std::{
    env,
    ffi::OsString,
    io::Write,
    os::unix::process::CommandExt,
    process::{Command, ExitCode},
};

const PYTHON: &str = "/usr/bin/python3";
const MAIN: &str = "/usr/lib/y2-platform/main.py";
const LIBRARY: &str = "/usr/lib/y2-platform";

/// Subcommands implemented in Rust. Phase 1: none.
fn ported(_args: &[OsString]) -> Option<fn(&[OsString]) -> ExitCode> {
    None
}

fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    match ported(&args) {
        Some(command) => command(&args),
        None => python(&args),
    }
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
