//! Group `suspend` of the port (docs/planning/PLATFORM-RUST-PORT-PLAN.md, "Remaining port groups"):
//! `sleep request`, `sleep prepare`, the product sleep helper (`product-sleep.py`), the suspend receipt
//! (`suspend-record.py`, `y2-suspend-record`), the sleep diagnostics and power journal rows a receipt
//! writes, and the small programmes `y2-power-audit`, `y2-status`, `y2-health`, `y2-power-command`.
//!
//! The programmes that are not `y2-platform` subcommands are hidden routes (`__product-sleep`,
//! `__suspend-record`, `__power-audit`, `__power-command`; argparse refuses every one of them) and
//! names this binary answers to by its argv[0] (`program`). A hidden route prints nothing on stdout
//! and ends the process with the programme's own exit code; one that cannot do an input runs the
//! Python programme itself (`python_script`), never main.py.
// SPDX-License-Identifier: GPL-2.0-only
pub mod admission;
pub mod append;
pub mod audit;
pub mod diagnostics;
pub mod helper;
pub mod journal;
pub mod pyjson;
pub mod receipt;
pub mod request;
pub mod scripts;

use super::Command;
use crate::reply::{Failure, Outcome, Reply};
use serde_json::Value;
use std::{
    env,
    ffi::OsString,
    io::{self, Write},
    os::unix::process::CommandExt,
    path::Path,
    process,
    sync::atomic::{AtomicBool, Ordering},
};
use y2p_core::{py, Root};

/// The Rust implementation of these exact (canonical) arguments, if this group has one.
pub fn route(args: &[&str]) -> Option<Command> {
    Some(match args {
        ["sleep", "request"] => Box::new(request::request),
        ["sleep", "prepare"] => Box::new(request::prepare),
        _ => return None,
    })
}

/// The group's programmes that are not y2-platform subcommands, as hidden first arguments: main.rs
/// runs these before its argparse parser (which refuses every `__` name). Each handles every
/// argument form itself (its own Python programme for what it does not read) and ends the process.
#[allow(dead_code)] // main.rs runs it before argparse (see the report / plan §5a)
pub fn programme(args: &[&str]) -> Option<Command> {
    let rest: Vec<String> = args.iter().skip(1).map(|a| a.to_string()).collect();
    Some(match args.first().copied()? {
        "__product-sleep" => Box::new(move |root| helper::main(root, &rest)),
        "__suspend-record" => Box::new(move |root| scripts::suspend_record(root, &rest)),
        "__power-command" => Box::new(move |root| scripts::power_command(root, &rest)),
        "__power-audit" => Box::new(move |root| audit::main(root, &rest)),
        _ => return None,
    })
}

/// The names this binary also answers to (the basename of argv[0], installed as links to
/// `/usr/sbin/y2-platform`): the y2-platform arguments that name's programme is. `None` for
/// `y2-platform` itself and every other name. main.rs replaces its arguments with these before
/// anything else.
///
/// - `y2-status`, `y2-health`: `exec /usr/sbin/y2-platform status|health "$@"`.
/// - `y2-suspend-record`: `exec python3 -I /usr/lib/y2-platform/suspend-record.py "$@"`.
/// - `y2-power-audit`: the Python programme of the same name.
/// - `poweroff`, `reboot` (`tools/platform/y2-power-command`, installed under both names) and
///   `y2-power-command` (which that script refuses with exit 2).
#[allow(dead_code)] // main.rs's argv[0] dispatch (phase 6 installs the names)
pub fn program(name: &str, args: &[OsString]) -> Option<Vec<OsString>> {
    let with = |first: &[&str]| {
        first
            .iter()
            .map(OsString::from)
            .chain(args.iter().cloned())
            .collect()
    };
    Some(match name {
        "y2-status" => with(&["status"]),
        "y2-health" => with(&["health"]),
        "y2-suspend-record" => with(&["__suspend-record"]),
        "y2-power-audit" => with(&["__power-audit"]),
        "poweroff" | "reboot" | "y2-power-command" => with(&["__power-command", name]),
        _ => return None,
    })
}

/// Set by the sleep helper's SIGTERM/SIGINT handler (`RuntimeError('sleep_helper_interrupted')`).
pub static INTERRUPTED: AtomicBool = AtomicBool::new(false);
/// The helper's restore is running: a signal is raised once it has finished.
pub static MASKED: AtomicBool = AtomicBool::new(false);

/// Where Python would run the helper's signal handler: raise (once) after a SIGTERM/SIGINT.
pub fn interrupted() -> Result<(), E> {
    if MASKED.load(Ordering::SeqCst) {
        return Ok(());
    }
    if INTERRUPTED.swap(false, Ordering::SeqCst) {
        return Err(E::RuntimeError("sleep_helper_interrupted".into()));
    }
    Ok(())
}

/// A Python exception on the suspend path: what `except` clauses tell apart and `str(error)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum E {
    /// `product_sleep.Refused` (a `ValueError`).
    Refused(String),
    /// A `ValueError` (or a subclass: `JSONDecodeError`, `UnicodeDecodeError`).
    Value {
        class: &'static str,
        text: String,
    },
    /// An `OSError` (or a subclass).
    Os {
        class: &'static str,
        text: String,
        errno: Option<i32>,
    },
    RuntimeError(String),
    TypeError(String),
    /// Any other exception (`AttributeError`, `KeyError`, ...): caught by no clause on this path.
    Crash(String),
}

impl E {
    pub fn value(text: impl Into<String>) -> E {
        E::Value {
            class: "ValueError",
            text: text.into(),
        }
    }

    pub fn refused(text: impl Into<String>) -> E {
        E::Refused(text.into())
    }

    /// An `OSError` from a call given `path` (its text names the path), or without one (sockets).
    pub fn os(error: &io::Error, path: Option<&Path>) -> E {
        if error.kind() == io::ErrorKind::InvalidData && error.raw_os_error().is_none() {
            // y2p_core's private_directory refusal and the journal's own ValueErrors.
            return E::value(error.to_string());
        }
        let shown = path.map(|p| p.to_string_lossy().into_owned());
        let (text, class) = py::os_error(error, shown.as_deref());
        E::Os {
            class,
            text,
            errno: error.raw_os_error(),
        }
    }

    /// `str(error)`.
    pub fn text(&self) -> &str {
        match self {
            E::Refused(t) | E::RuntimeError(t) | E::TypeError(t) | E::Crash(t) => t,
            E::Value { text, .. } | E::Os { text, .. } => text,
        }
    }

    /// `isinstance(error, ValueError)`.
    pub fn is_value(&self) -> bool {
        matches!(self, E::Refused(_) | E::Value { .. })
    }

    /// `isinstance(error, OSError)`.
    pub fn is_os(&self) -> bool {
        matches!(self, E::Os { .. })
    }

    /// `except (OSError, ValueError, TypeError)`: the clauses of the journals and the progress rows.
    pub fn is_recorded_failure(&self) -> bool {
        self.is_os() || self.is_value() || matches!(self, E::TypeError(_))
    }

    /// `type(error).__name__`.
    pub fn class(&self) -> &'static str {
        match self {
            E::Refused(_) => "Refused",
            E::Value { class, .. } | E::Os { class, .. } => class,
            E::RuntimeError(_) => "RuntimeError",
            E::TypeError(_) => "TypeError",
            E::Crash(_) => "AttributeError",
        }
    }
}

/// main.py's handling of an exception that left a subcommand: an `OSError`, `ValueError` or
/// `RuntimeError` is printed as `org.y2linux.error/v1` (exit 1); any other ends in a traceback
/// (nothing on stdout, exit 1).
pub fn cli(result: Result<Reply, E>) -> Outcome {
    match result {
        Ok(reply) => Ok(reply),
        Err(E::Value {
            class: "ValueError",
            text,
        }) => Err(Failure::Value(text)),
        Err(error @ (E::TypeError(_) | E::Crash(_))) => traceback(&error, 1),
        Err(error) => Err(Failure::Os {
            text: error.text().to_owned(),
            class: error.class(),
        }),
    }
}

/// An uncaught exception: Python's traceback (its last line) on stderr, nothing on stdout.
pub fn traceback(error: &E, code: i32) -> ! {
    let _ = writeln!(
        io::stderr(),
        "Traceback (most recent call last):\n{}: {}",
        error.class(),
        error.text()
    );
    exit(code)
}

/// End a programme that prints nothing (the hidden routes).
pub fn exit(code: i32) -> ! {
    let _ = io::stdout().flush();
    process::exit(code)
}

/// The fixture's Python, or the player's (as main.rs's fallback picks them).
fn interpreter() -> (OsString, bool) {
    let test = env::var_os("Y2_ROOT").is_some();
    let python = env::var_os("Y2_PLATFORM_PYTHON")
        .filter(|_| test)
        .unwrap_or_else(|| "/usr/bin/python3".into());
    (python, test)
}

/// Replace this process with the Python programme `script` (`/usr/lib/y2-platform/<script>`) and
/// `args`. Below a fixture root the harness's launcher runs it as the hidden route `route`
/// (tests/platform_cases/suspend.py).
pub fn python_script(script: &str, route: &str, args: &[String]) -> ! {
    let (python, test) = interpreter();
    let mut command = process::Command::new(&python);
    command.arg("-I");
    if test {
        let main = env::var_os("Y2_PLATFORM_MAIN")
            .unwrap_or_else(|| "/usr/lib/y2-platform/main.py".into());
        command.arg(main).arg(route);
    } else if script == "y2-power-audit" {
        // Not -I in its own #! line: `#!/usr/bin/env python3`.
        command = process::Command::new("/usr/bin/python3");
        command.arg("/usr/lib/y2-platform/y2-power-audit");
    } else {
        command.arg(format!("/usr/lib/y2-platform/{script}"));
    }
    let error = command.args(args).exec();
    let _ = writeln!(
        io::stderr(),
        "y2-platform: {}: {error}",
        python.to_string_lossy()
    );
    exit(if error.kind() == io::ErrorKind::PermissionDenied {
        126
    } else {
        127
    })
}

/// `exec /usr/sbin/y2-platform <args>` from a shell wrapper: the Rust command when one is ported,
/// otherwise the Python platform (main.rs's fallback).
pub fn platform(root: &Root, args: &[&str]) -> ! {
    if let Some(command) = super::route(args) {
        match command(root) {
            Ok(reply) => {
                let mut out = io::stdout().lock();
                let _ = out
                    .write_all(reply.stdout.as_bytes())
                    .and_then(|()| out.write_all(b"\n"))
                    .and_then(|()| out.flush());
                exit(i32::from(reply.code))
            }
            Err(Failure::Fallback) => {}
            Err(error) => {
                let text =
                    y2p_core::json::dumps(&error.record(), y2p_core::json::Separators::Spaced)
                        .expect("record");
                println!("{text}");
                exit(1)
            }
        }
    }
    let (python, test) = interpreter();
    let main = env::var_os("Y2_PLATFORM_MAIN")
        .filter(|_| test)
        .unwrap_or_else(|| "/usr/lib/y2-platform/main.py".into());
    let error = process::Command::new(&python)
        .arg("-I")
        .arg(main)
        .args(args)
        .env("PYTHONPATH", "/usr/lib/y2-platform")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .exec();
    let _ = writeln!(
        io::stderr(),
        "y2-platform: {}: {error}",
        python.to_string_lossy()
    );
    exit(127)
}

// ---- Python value rules on parsed records ----------------------------------------------------------

/// `a == b` between parsed JSON values as Python compares them (`1 == 1.0 == True`).
pub fn py_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(_) | Value::Bool(_), Value::Number(_) | Value::Bool(_)) => {
            match (exact(a), exact(b)) {
                (Some(x), Some(y)) => x == y,
                _ => py::number(Some(a)) == py::number(Some(b)),
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| py_eq(a, b))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| py_eq(v, w)))
        }
        _ => a == b,
    }
}

fn exact(value: &Value) -> Option<i128> {
    match value {
        Value::Bool(b) => Some(i128::from(*b)),
        Value::Number(n) => n
            .as_i64()
            .map(i128::from)
            .or_else(|| n.as_u64().map(i128::from)),
        _ => None,
    }
}

/// `value.get(key)` compared with a text.
pub fn is_text(value: Option<&Value>, text: &str) -> bool {
    value.and_then(Value::as_str) == Some(text)
}

/// `a > b` for the numbers a record holds; any other pair is Python's `TypeError`.
pub fn py_gt(a: &Value, b: &Value) -> Result<bool, E> {
    match (exact(a), exact(b)) {
        (Some(x), Some(y)) => Ok(x > y),
        _ => match (py::number(Some(a)), py::number(Some(b))) {
            (Some(x), Some(y)) => Ok(x > y),
            _ => match (a, b) {
                (Value::String(x), Value::String(y)) => Ok(x > y),
                _ => Err(E::TypeError("'>' not supported between instances".into())),
            },
        },
    }
}

/// `value.get(key, 0) + n` for a counter kept in a record (`None`: the key is missing).
pub fn py_add(current: Option<&Value>, n: f64, integer: bool) -> Result<Value, E> {
    let zero = Value::from(0);
    let current = current.unwrap_or(&zero);
    match (exact(current), integer) {
        (Some(x), true) => Ok(Value::from((x + n as i128) as i64)),
        _ => match py::number(Some(current)) {
            Some(x) => Ok(serde_json::Number::from_f64(x + n)
                .map(Value::Number)
                .unwrap_or(Value::Null)),
            None => Err(E::TypeError("unsupported operand type(s) for +".into())),
        },
    }
}

/// `time.time()`.
pub fn wall() -> f64 {
    super::time::wall()
}

/// `time.monotonic()`.
pub fn monotonic() -> f64 {
    clock(libc::CLOCK_MONOTONIC)
}

/// `time.clock_gettime(time.CLOCK_BOOTTIME)`.
pub fn boottime() -> f64 {
    clock(libc::CLOCK_BOOTTIME)
}

/// `time.process_time()`.
pub fn process_time() -> f64 {
    clock(libc::CLOCK_PROCESS_CPUTIME_ID)
}

fn clock(id: libc::clockid_t) -> f64 {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: a valid, writable timespec; the clock ids exist on Linux.
    unsafe { libc::clock_gettime(id, &mut now) };
    now.tv_sec as f64 + now.tv_nsec as f64 * 1e-9
}

/// The value as JSON: `None` is `null`.
pub fn text_value(text: Option<String>) -> Value {
    text.map_or(Value::Null, Value::String)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A self-cleaning directory (no tempfile crate in the vendored set).
    pub struct Dir(std::path::PathBuf);
    static NEXT: AtomicU32 = AtomicU32::new(0);
    impl Dir {
        pub fn new() -> Self {
            let path = env::temp_dir().join(format!(
                "y2-suspend-{}-{}",
                process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        pub fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn names_map_to_their_programmes() {
        let args = |list: &[&str]| list.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(
            program("y2-status", &args(&["wifi"])),
            Some(args(&["status", "wifi"]))
        );
        assert_eq!(program("y2-health", &[]), Some(args(&["health"])));
        assert_eq!(
            program("y2-suspend-record", &args(&["requested", "0"])),
            Some(args(&["__suspend-record", "requested", "0"]))
        );
        assert_eq!(
            program("reboot", &args(&["-f"])),
            Some(args(&["__power-command", "reboot", "-f"]))
        );
        assert_eq!(
            program("y2-power-audit", &args(&["--state", "x"])),
            Some(args(&["__power-audit", "--state", "x"]))
        );
        assert_eq!(program("y2-platform", &[]), None);
        assert_eq!(program("python3", &[]), None);
    }

    #[test]
    fn values_compare_as_python_does() {
        assert!(py_eq(&json!(1), &json!(1.0)) && py_eq(&json!(true), &json!(1)));
        assert!(!py_eq(&json!("1"), &json!(1)));
        assert!(py_eq(&json!({"a": [1, null]}), &json!({"a": [1.0, null]})));
        assert!(py_gt(&json!(3), &json!(2)).unwrap());
        assert!(py_gt(&json!("b"), &json!("a")).unwrap());
        assert!(py_gt(&json!("b"), &json!(1)).is_err());
        assert_eq!(py_add(None, 1.0, true).unwrap(), json!(1));
        assert!(py_add(Some(&json!(null)), 1.0, true).is_err());
        assert_eq!(py_add(Some(&json!(2)), 1.0, true).unwrap(), json!(3));
        assert_eq!(py_add(Some(&json!(true)), 1.0, true).unwrap(), json!(2));
        assert_eq!(py_add(Some(&json!(0.5)), 1.0, true).unwrap(), json!(1.5));
    }
}
