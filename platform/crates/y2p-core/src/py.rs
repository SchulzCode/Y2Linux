//! Python semantics the ported code needs on parsed JSON: `x is True`, `isinstance(x, (int, float))`
//! (a bool is an int there), `dict.get` on a value that may not be a dict, and Python's `str()` of an
//! `OSError`.
// SPDX-License-Identifier: GPL-2.0-only
use serde_json::Value;
use std::{ffi::CStr, io};

/// `value is True`.
pub fn is_true(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Bool(true)))
}

/// `value is False`.
pub fn is_false(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Bool(false)))
}

/// Python truthiness of a parsed JSON value.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `value == n` for an int: Python also finds `1.0 == 1` and `True == 1`.
pub fn eq_int(value: Option<&Value>, n: i64) -> bool {
    number(value) == Some(n as f64)
}

/// `isinstance(value, (int, float))` as a number (True is 1).
pub fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        _ => None,
    }
}

/// What a `for` loop over a parsed JSON value yields: a list's items, a dict's keys (sorted, the order
/// of the records, which are written sorted), a str's characters (as str values); `None` where Python
/// raises TypeError (a number, bool or None is not iterable).
pub fn iterate(value: &Value) -> Option<Vec<Value>> {
    match value {
        Value::Array(items) => Some(items.clone()),
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            Some(keys.into_iter().map(|k| Value::from(k.as_str())).collect())
        }
        Value::String(text) => Some(text.chars().map(|c| Value::from(c.to_string())).collect()),
        _ => None,
    }
}

/// `type(value) is int` (not a bool, not a float).
pub fn exact_int(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(n) if n.is_i64() || n.is_u64() => n.as_i64(),
        _ => None,
    }
}

/// Python's `str(OSError)` text and class name for an errno and the path the call was given.
pub fn os_error(error: &io::Error, path: Option<&str>) -> (String, &'static str) {
    let Some(errno) = error.raw_os_error() else {
        return (error.to_string(), "OSError");
    };
    let message = strerror(error);
    let text = match path {
        Some(path) => format!("[Errno {errno}] {message}: {}", repr(path)),
        None => format!("[Errno {errno}] {message}"),
    };
    (text, os_error_class(errno))
}

/// `error.strerror`: the message without its errno prefix (`No space left on device`), or the error's
/// text when it carries no errno.
pub fn strerror(error: &io::Error) -> String {
    match error.raw_os_error() {
        // SAFETY: strerror returns a pointer to a NUL-terminated static or thread-local message.
        Some(errno) => unsafe { CStr::from_ptr(libc::strerror(errno)) }
            .to_string_lossy()
            .into_owned(),
        None => error.to_string(),
    }
}

/// The builtin `OSError` subclass Python raises for an errno (PEP 3151).
pub fn os_error_class(errno: i32) -> &'static str {
    match errno {
        libc::ENOENT => "FileNotFoundError",
        libc::EEXIST => "FileExistsError",
        libc::EACCES | libc::EPERM => "PermissionError",
        libc::EISDIR => "IsADirectoryError",
        libc::ENOTDIR => "NotADirectoryError",
        libc::EAGAIN | libc::EALREADY | libc::EINPROGRESS => "BlockingIOError",
        libc::ECHILD => "ChildProcessError",
        libc::EPIPE | libc::ESHUTDOWN => "BrokenPipeError",
        libc::ECONNABORTED => "ConnectionAbortedError",
        libc::ECONNREFUSED => "ConnectionRefusedError",
        libc::ECONNRESET => "ConnectionResetError",
        libc::EINTR => "InterruptedError",
        libc::ESRCH => "ProcessLookupError",
        libc::ETIMEDOUT => "TimeoutError",
        _ => "OSError",
    }
}

/// Python `repr()` of a str: single quotes unless the text holds one and no double quote.
pub fn repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            // An escaped byte of a non-UTF-8 argument: a lone surrogate, which Python shows escaped.
            c if crate::os_text::surrogate(c).is_some() => out.push_str(&format!(
                "\\u{:04x}",
                crate::os_text::surrogate(c).unwrap_or_default()
            )),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// `str(uuid.uuid4())` from the kernel's random pool (blocking until it is ready, as `os.urandom`).
pub fn uuid4() -> io::Result<String> {
    let mut bytes = [0u8; 16];
    let mut filled = 0;
    while filled < bytes.len() {
        // SAFETY: the pointer and length describe the unfilled tail of `bytes`.
        let read = unsafe {
            libc::getrandom(bytes[filled..].as_mut_ptr().cast(), bytes.len() - filled, 0)
        };
        if read < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(error);
        }
        filled += read as usize;
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

/// `len(os.getrandom(1, os.GRND_NONBLOCK)) == 1`: the kernel's random pool is initialised.
pub fn entropy_ready() -> bool {
    let mut byte = 0u8;
    // SAFETY: one writable byte.
    unsafe { libc::getrandom((&mut byte as *mut u8).cast(), 1, libc::GRND_NONBLOCK) == 1 }
}

/// Python's rules for a parsed JSON value (`json.loads` gives dict, list, str, int, float, bool,
/// None): truthiness, `type(x).__name__`, `==`, `str()` and `repr()`.
pub trait PyValue {
    /// Python truthiness.
    fn truthy(&self) -> bool;
    /// `type(value).__name__`.
    fn type_name(&self) -> &'static str;
    /// Whether the value can be a dict key or a set member (`TypeError: unhashable` otherwise).
    fn hashable(&self) -> bool;
    /// Python `a == b`: numbers and bools by value (`True == 1 == 1.0`, an int and a float exactly),
    /// containers element-wise, dicts by keys and values regardless of order.
    fn py_eq(&self, other: &Value) -> bool;
    /// `value == text` for a str.
    fn is_str(&self, text: &str) -> bool;
    /// `type(value) is int`.
    fn exact_int(&self) -> Option<i64>;
    /// Python `str(value)` (`'%s' % value`, `str(ValueError(value))`).
    fn py_str(&self) -> String;
    /// Python `repr(value)`: a dict in its key order.
    fn py_repr(&self) -> String;
}

impl PyValue for Value {
    fn truthy(&self) -> bool {
        truthy(self)
    }

    fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "NoneType",
            Value::Bool(_) => "bool",
            Value::Number(n) if n.is_f64() => "float",
            Value::Number(_) => "int",
            Value::String(_) => "str",
            Value::Array(_) => "list",
            Value::Object(_) => "dict",
        }
    }

    fn hashable(&self) -> bool {
        !matches!(self, Value::Array(_) | Value::Object(_))
    }

    fn py_eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.py_eq(y))
            }
            (Value::Object(a), Value::Object(b)) => {
                a.len() == b.len() && a.iter().all(|(k, v)| b.get(k).is_some_and(|w| v.py_eq(w)))
            }
            (a, b) => match (Numeric::of(a), Numeric::of(b)) {
                (Some(x), Some(y)) => x.same(&y),
                _ => false,
            },
        }
    }

    fn is_str(&self, text: &str) -> bool {
        self.as_str() == Some(text)
    }

    fn exact_int(&self) -> Option<i64> {
        exact_int(Some(self))
    }

    fn py_str(&self) -> String {
        match self {
            Value::String(s) => s.clone(),
            other => other.py_repr(),
        }
    }

    fn py_repr(&self) -> String {
        match self {
            Value::Null => "None".into(),
            Value::Bool(true) => "True".into(),
            Value::Bool(false) => "False".into(),
            Value::Number(n) => match n.as_f64() {
                Some(f) if n.is_f64() => crate::json::float_repr(f),
                _ => n.to_string(),
            },
            Value::String(s) => repr(s),
            Value::Array(a) => format!(
                "[{}]",
                a.iter().map(Value::py_repr).collect::<Vec<_>>().join(", ")
            ),
            Value::Object(o) => format!(
                "{{{}}}",
                o.iter()
                    .map(|(k, v)| format!("{}: {}", repr(k), v.py_repr()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

/// `a == b` of two optional values, a missing one as None.
pub fn eq(a: Option<&Value>, b: Option<&Value>) -> bool {
    a.unwrap_or(&Value::Null).py_eq(b.unwrap_or(&Value::Null))
}

enum Numeric {
    Int(i128),
    Float(f64),
}

impl Numeric {
    fn of(value: &Value) -> Option<Numeric> {
        match value {
            Value::Bool(b) => Some(Numeric::Int(i128::from(*b))),
            Value::Number(n) => Some(if let Some(i) = n.as_i64() {
                Numeric::Int(i128::from(i))
            } else if let Some(u) = n.as_u64() {
                Numeric::Int(i128::from(u))
            } else {
                Numeric::Float(n.as_f64().unwrap_or(f64::NAN))
            }),
            _ => None,
        }
    }

    fn same(&self, other: &Numeric) -> bool {
        match (self, other) {
            (Numeric::Int(a), Numeric::Int(b)) => a == b,
            (Numeric::Float(a), Numeric::Float(b)) => a == b,
            // Python compares an int with a float exactly, not through a conversion.
            (Numeric::Int(i), Numeric::Float(f)) | (Numeric::Float(f), Numeric::Int(i)) => {
                f.is_finite() && f.fract() == 0.0 && f.abs() < 1.8e38 && *f as i128 == *i
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn equality_and_spelling_follow_python() {
        let parse = |t: &str| serde_json::from_str::<Value>(t).unwrap();
        assert!(parse("1").py_eq(&parse("1.0")) && parse("true").py_eq(&parse("1")));
        assert!(!parse("1.5").py_eq(&parse("1")) && !parse("\"1\"").py_eq(&parse("1")));
        assert!(parse(r#"{"a": 1, "b": [2]}"#).py_eq(&parse(r#"{"b": [2.0], "a": true}"#)));
        // 2**53 + 1 is no float: Python compares exactly.
        assert!(!parse("9007199254740993").py_eq(&parse("9007199254740992.0")));
        assert!(eq(None, Some(&Value::Null)) && !eq(Some(&json!("1")), Some(&json!(1))));
        assert_eq!(
            parse(r#"[null, true, 1.0, "it's", {"k": "v", "a": 1}]"#).py_repr(),
            r#"[None, True, 1.0, "it's", {'k': 'v', 'a': 1}]"#
        );
        assert_eq!(parse("\"x\"").py_str(), "x");
        assert_eq!(parse("[]").type_name(), "list");
        assert_eq!(parse("2.0").type_name(), "float");
        assert!(Numeric::Int(3).same(&Numeric::Float(3.0)));
    }

    #[test]
    fn values_follow_python_rules() {
        assert!(is_true(Some(&json!(true))) && !is_true(Some(&json!(1))) && !is_true(None));
        assert_eq!(number(Some(&json!(true))), Some(1.0));
        assert_eq!(number(Some(&json!("1"))), None);
        assert_eq!(exact_int(Some(&json!(true))), None);
        assert_eq!(exact_int(Some(&json!(2.0))), None);
        assert_eq!(exact_int(Some(&json!(-3))), Some(-3));
    }

    #[test]
    fn os_errors_print_as_python_does() {
        // python3 -c "open('/nonexistent/x')": [Errno 2] No such file or directory: '/nonexistent/x'
        let error = io::Error::from_raw_os_error(libc::ENOENT);
        assert_eq!(
            os_error(&error, Some("/nonexistent/x")),
            (
                "[Errno 2] No such file or directory: '/nonexistent/x'".into(),
                "FileNotFoundError"
            )
        );
        assert_eq!(
            os_error(&io::Error::from_raw_os_error(libc::EAGAIN), None).0,
            "[Errno 11] Resource temporarily unavailable"
        );
        assert_eq!(repr("it's"), "\"it's\"");
        assert_eq!(repr("a'b\"c\n"), "'a\\'b\"c\\n'");
    }

    #[test]
    fn uuids_are_version_4() {
        let id = uuid4().unwrap();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert!(matches!(&id[19..20], "8" | "9" | "a" | "b"));
        assert_ne!(id, uuid4().unwrap());
    }
}
