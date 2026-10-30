//! Product sleep: `sleep request`, `sleep prepare`, the detached sleep helper (`__product-sleep`),
//! the suspend receipt (`suspend-last.json`, also `y2-suspend-record`), the sleep diagnostics and
//! power journal rows a receipt writes, and the small programmes `y2-power-audit`, `y2-status`,
//! `y2-health`, `y2-power-command`.
//!
//! The programmes that are not `y2-platform` subcommands are hidden routes (`__product-sleep`,
//! `__suspend-record`, `__power-audit`, `__power-command`; the parser refuses every one of them) and
//! names this binary answers to by its argv[0] (`program`). A hidden route prints nothing on stdout
//! and ends the process with the programme's own exit code; a failure is one line on stderr.
//!
//! A record on the sleep path with a shape the code cannot use (`E::Invalid`, `E::TypeError`) is a
//! refusal like any other failure: `sleep request|prepare|status` print `{"failure": …, "state":
//! "Failed"}` and exit 1, the helper records it in the receipt and still restores, and the hidden
//! programmes exit 1.
// SPDX-License-Identifier: GPL-2.0-only

pub mod admission;
pub mod audit;
pub mod diagnostics;
pub mod helper;
pub mod journal;
pub mod receipt;
pub mod request;
pub mod scripts;
#[cfg(test)]
mod test_support;

use super::Command;
use crate::reply::{Failure, Outcome, Reply};
use serde::{ser::SerializeMap, Serialize, Serializer};
use serde_json::Value;
use std::{
    ffi::OsString,
    io::{self, Write},
    path::Path,
    process,
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
};
pub use y2p_core::paths::{name, parent_name};
use y2p_core::{json::ObjectExt, py, suspend_text::Regex, Root};

/// The Rust implementation of these exact (canonical) arguments, if this group has one.
pub fn route(args: &[&str]) -> Option<Command> {
    Some(match args {
        ["sleep", "request"] => Box::new(request::request),
        ["sleep", "prepare"] => Box::new(request::prepare),
        _ => return None,
    })
}

/// The group's programmes that are not y2-platform subcommands, as hidden first arguments: main.rs
/// runs these before its parser (which refuses every `__` name). Each reads its own arguments and
/// ends the process.
#[allow(dead_code)] // main.rs runs it before the parser
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
/// - `y2-suspend-record`: one receipt record for y2-suspend (`__suspend-record`).
/// - `y2-power-audit`: the endpoint power audit (`__power-audit`).
/// - `poweroff`, `reboot` (the normal shutdown through the platform coordinator) and
///   `y2-power-command` (refused with exit 2).
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

/// The helper's SIGTERM/SIGINT, checked between steps (a handler only sets a flag): the error once
/// after the signal, nothing while the restore runs (it is checked after the restore).
pub fn interrupted() -> Result<(), E> {
    if MASKED.load(Ordering::SeqCst) {
        return Ok(());
    }
    if INTERRUPTED.swap(false, Ordering::SeqCst) {
        return Err(E::RuntimeError("sleep_helper_interrupted".into()));
    }
    Ok(())
}

/// A failure on the suspend path: its kind decides who handles it (a refusal, a recorded failure, a
/// dropped journal row), its text is the reason a receipt or a reply shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum E {
    /// A reason not to sleep now (exit 2, `refused` in the receipt).
    Refused(String),
    /// A value the code refuses (a JSON decode error from Reborn keeps its class: its text is the
    /// receipt's reason).
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
    /// A value of the wrong type in a record (a list where a count belongs, two values that do not
    /// compare): the journals and the progress rows count and drop it, as they do OS errors.
    TypeError(String),
    /// A record whose shape the sleep path cannot read (a product that is no object, a missing
    /// field): no journal drops it; it ends the operation as a refusal.
    Invalid(String),
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

    /// `boot.private_directory(path)`.
    pub fn private_directory(path: &Path) -> Result<(), E> {
        y2p_core::record::private_directory(path).map_err(|e| E::os(&e, Some(path)))
    }

    /// `common.atomic_json(path, value, durable=durable)`: a private (0600) record.
    pub fn atomic_json<T: Serialize + ?Sized>(
        path: &Path,
        value: &T,
        durable: bool,
    ) -> Result<(), E> {
        y2p_core::record::atomic_json(path, value, durable, 0o600)
            .map_err(|e| E::os(&e, Some(path)))
    }

    /// `str(error)`.
    pub fn text(&self) -> &str {
        match self {
            E::Refused(t) | E::RuntimeError(t) | E::TypeError(t) | E::Invalid(t) => t,
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
            E::Invalid(_) => "Invalid",
        }
    }
}

/// A journal append failure (y2p_core::power_journal) as the error the suspend path counts.
/// An `OSError` from a call that names no file.
impl From<io::Error> for E {
    fn from(error: io::Error) -> E {
        E::os(&error, None)
    }
}

impl From<y2p_core::power_journal::AppendError> for E {
    fn from(error: y2p_core::power_journal::AppendError) -> E {
        use y2p_core::power_journal::AppendError;
        match error {
            AppendError::Value(text) => E::value(text),
            AppendError::Os { error, path } => E::os(&error, path.as_deref()),
            AppendError::OsMessage(text) => E::Os {
                class: "OSError",
                text: text.into(),
                errno: None,
            },
        }
    }
}

/// An error that left a sleep subcommand: an OS error, a value error or a runtime error is printed
/// as `org.y2linux.error/v1` (exit 1, as every subcommand's); a record the command cannot read is
/// its refusal record (exit 1).
pub fn cli(result: Result<Reply, E>) -> Outcome {
    match result {
        Ok(reply) => Ok(reply),
        Err(E::Value {
            class: "ValueError",
            text,
        }) => Err(Failure::Value(text)),
        Err(error @ (E::TypeError(_) | E::Invalid(_))) => refusal(error.text()),
        Err(error) => Err(Failure::Os {
            text: error.text().to_owned(),
            class: error.class(),
        }),
    }
}

/// A sleep subcommand's refusal record: `{"failure": text, "state": "Failed"}`, exit 1.
pub fn refusal(text: &str) -> Outcome {
    let record = y2p_core::json::Object::new()
        .with("failure", text)
        .with("state", "Failed");
    Reply::sorted(&record, 1)
}

/// End a hidden programme that failed: one line on stderr, nothing on stdout, exit 1.
pub fn fail(programme: &str, error: &E) -> ! {
    let _ = writeln!(io::stderr(), "{programme}: {}", error.text());
    exit(1)
}

/// End a programme that prints nothing (the hidden routes).
pub fn exit(code: i32) -> ! {
    let _ = io::stdout().flush();
    process::exit(code)
}

/// `exec /usr/sbin/y2-platform <args>` from a shell wrapper: the command's reply, its error record or
/// its refusal record, and its exit code.
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
            Err(error) => {
                let text = y2p_core::json::dumps(&error.record());
                if let Ok(text) = text {
                    println!("{text}");
                }
                exit(1)
            }
        }
    }
    unrouted(args)
}

/// A programme line no route takes (main.rs's route test rules this out).
fn unrouted(args: &[&str]) -> ! {
    let _ = writeln!(
        io::stderr(),
        "y2-platform: {}: no implementation",
        args.first().copied().unwrap_or_default()
    );
    exit(127)
}

// ---- comparisons and counters on parsed records (their JSON types may be anything) ----------------

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

/// `a > b` for the numbers (or texts) a record holds; any other pair does not compare.
pub fn py_gt(a: &Value, b: &Value) -> Result<bool, E> {
    match (exact(a), exact(b)) {
        (Some(x), Some(y)) => Ok(x > y),
        _ => match (py::python_number(Some(a)), py::python_number(Some(b))) {
            (Some(x), Some(y)) => Ok(x > y),
            _ => match (a, b) {
                (Value::String(x), Value::String(y)) => Ok(x > y),
                _ => Err(E::TypeError("'>' not supported between instances".into())),
            },
        },
    }
}

/// Pairs in their order as one JSON object (a later pair with the same key replaced the earlier
/// one when the list was built).
pub fn as_map<K: Serialize, V: Serialize, S: Serializer>(
    pairs: &[(K, V)],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(pairs.len()))?;
    for (key, value) in pairs {
        map.serialize_entry(key, value)?;
    }
    map.end()
}

/// A text that should hold a decimal integer and does not: the reason names it.
pub fn int_literal(text: &str) -> E {
    E::value(format!(
        "invalid literal for int() with base 10: {}",
        py::repr(text)
    ))
}

/// A constant pattern, compiled once. `None` only if the text were no pattern (the tests compile
/// each one): every caller then takes its safe answer instead of a panic, which on the sleep path
/// could leave Reborn quiesced.
pub fn pattern(cell: &'static OnceLock<Option<Regex>>, text: &str) -> Option<&'static Regex> {
    cell.get_or_init(|| Regex::new(text).ok()).as_ref()
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

pub use y2p_core::clock::process_time;

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
            let path = std::env::temp_dir().join(format!(
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
        assert!(py_gt(&json!(3), &json!(2)).unwrap());
        assert!(py_gt(&json!("b"), &json!("a")).unwrap());
        assert!(py_gt(&json!("b"), &json!(1)).is_err());
    }
}
