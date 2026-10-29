//! Python semantics the observation code leans on: `str.splitlines`/`split`, `int()` of text,
//! `common.key_values`/`counters`, `re.findall` pairs, `==` between parsed JSON values (`1 == 1.0 ==
//! True`), `os.path.realpath`, uncapped `Path.glob`, and `ctx.record`.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::Failure;
use serde_json::{json, Map, Value};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use y2p_core::py::PyValue;
use y2p_core::{
    clock,
    observation_support::regex::Regex,
    root::{self, python_strip},
    Root,
};

/// `common.read(path, limit)` on a host path.
pub fn read(path: &Path) -> Option<String> {
    root::read(path, root::READ_LIMIT)
}

pub fn read_limited(path: &Path, limit: usize) -> Option<String> {
    root::read(path, limit)
}

/// `str.splitlines()`.
pub fn splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        let breaks = matches!(
            c,
            '\n' | '\r'
                | '\u{b}'
                | '\u{c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if !breaks {
            continue;
        }
        lines.push(&text[start..index]);
        let mut end = index + c.len_utf8();
        if c == '\r' {
            if let Some(&(next, '\n')) = chars.peek() {
                chars.next();
                end = next + 1;
            }
        }
        start = end;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// Python `str.isspace()` for one character.
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// `str.split()` without arguments.
pub fn split_ws(text: &str) -> Vec<&str> {
    text.split(is_space).filter(|s| !s.is_empty()).collect()
}

/// `int(text)` (`common.number`) of a str: any Unicode decimal digits, surrounding whitespace and
/// single underscores, as Python's `int()`. None where it raises (and, an accepted difference, beyond
/// 64 bits, where Python would carry on with a larger int).
pub fn int(text: &str) -> Option<i64> {
    crate::argparse::python::int(text)?.parse().ok()
}

/// `int(text)` where nothing catches its ValueError: the value, or main.py's error record
/// (`invalid literal for int() with base 10: '…'`).
pub fn int_or_raise(text: &str) -> Result<i64, Failure> {
    int(text).ok_or_else(|| Failure::int_literal(text))
}

/// The characters `str.isdigit()` takes besides the decimal digits (Numeric_Type=Digit:
/// superscripts, circled digits, …), as inclusive ranges (Python's Unicode 16 tables).
const DIGITS: [(u32, u32); 20] = [
    (0xb2, 0xb3),
    (0xb9, 0xb9),
    (0x1369, 0x1371),
    (0x19da, 0x19da),
    (0x2070, 0x2070),
    (0x2074, 0x2079),
    (0x2080, 0x2089),
    (0x2460, 0x2468),
    (0x2474, 0x247c),
    (0x2488, 0x2490),
    (0x24ea, 0x24ea),
    (0x24f5, 0x24fd),
    (0x24ff, 0x24ff),
    (0x2776, 0x277e),
    (0x2780, 0x2788),
    (0x278a, 0x2792),
    (0x10a40, 0x10a43),
    (0x10e60, 0x10e68),
    (0x11052, 0x1105a),
    (0x1f100, 0x1f10a),
];

/// `str.isdigit()`: not empty, every character a decimal digit or another Unicode digit. `int()`
/// reads only the decimal ones: an `isdigit()` text may still be no int.
pub fn isdigit(text: &str) -> bool {
    !text.is_empty()
        && text.chars().all(|c| {
            crate::argparse::python::decimal(c).is_some()
                || DIGITS.iter().any(|&(a, b)| (a..=b).contains(&(c as u32)))
        })
}

/// A dict key that is no str (`dict([[1, 2]])`), kept under this prefix: Python's
/// `json.dumps(sort_keys=True)` of a dict holding it raises TypeError (`unsortable`).
pub const NON_STR_KEY: &str = "\u{0}non-str-key:";

/// `dict(value)` of a truthy parsed JSON value that is no dict: a list of pairs (two-item lists,
/// two-character strs, two-key dicts), or the ValueError/TypeError Python raises.
pub fn py_dict(value: &Value) -> Result<Map<String, Value>, Failure> {
    let length = |index: usize, n: usize| {
        Failure::value(format!(
            "dictionary update sequence element #{index} has length {n}; 2 is required"
        ))
    };
    let items = match value {
        Value::Array(items) => items,
        // A str's items are its characters (length 1).
        Value::String(_) => return Err(length(0, 1)),
        other => {
            return Err(Failure::type_error(format!(
                "'{}' object is not iterable",
                PyValue::type_name(other)
            )))
        }
    };
    let mut result = Map::new();
    for (index, item) in items.iter().enumerate() {
        let (key, value) = match item {
            Value::Array(pair) if pair.len() == 2 => (pair[0].clone(), pair[1].clone()),
            Value::Array(other) => return Err(length(index, other.len())),
            Value::String(text) => {
                let chars: Vec<char> = text.chars().collect();
                if chars.len() != 2 {
                    return Err(length(index, chars.len()));
                }
                (json!(chars[0].to_string()), json!(chars[1].to_string()))
            }
            // Its keys, sorted (py::iterate).
            Value::Object(map) if map.len() == 2 => {
                let mut keys = map.keys().collect::<Vec<_>>();
                keys.sort();
                (json!(keys[0]), json!(keys[1]))
            }
            Value::Object(map) => return Err(length(index, map.len())),
            _ => return Err(Failure::type_error("object is not iterable")),
        };
        let key = match key {
            Value::String(key) => key,
            key @ (Value::Array(_) | Value::Object(_)) => {
                let name = PyValue::type_name(&key);
                return Err(Failure::type_error(format!(
                    "cannot use '{name}' as a dict key (unhashable type: '{name}')"
                )));
            }
            other => format!("{NON_STR_KEY}{other}"),
        };
        result.insert(key, value);
    }
    Ok(result)
}

/// Whether `json.dumps(value, sort_keys=True)` raises: a dict in it holds a key that is no str.
pub fn unsortable(value: &Value) -> bool {
    match value {
        Value::Object(map) => map
            .iter()
            .any(|(k, v)| k.starts_with(NON_STR_KEY) || unsortable(v)),
        Value::Array(items) => items.iter().any(unsortable),
        _ => false,
    }
}

/// `json.dumps(value, sort_keys=True)`: the TypeError where it cannot sort a dict's keys.
pub fn sortable(value: &Value) -> Result<(), Failure> {
    if unsortable(value) {
        return Err(Failure::type_error(
            "'<' not supported between instances of 'int' and 'str'",
        ));
    }
    Ok(())
}

/// `value.get(...)` on a parsed JSON value that is no dict: Python's AttributeError.
pub fn no_get(value: &Value) -> Failure {
    Failure::attribute_error(format!(
        "'{}' object has no attribute 'get'",
        PyValue::type_name(value)
    ))
}

/// `common.number(value)` as JSON: an int or null.
pub fn number(text: Option<&str>) -> Value {
    text.and_then(int).map_or(Value::Null, Value::from)
}

/// JSON of an optional str.
pub fn text(value: Option<String>) -> Value {
    value.map_or(Value::Null, Value::String)
}

/// `common.key_values(text, separator)`.
pub fn key_values(text: Option<&str>, separator: &str) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    for line in splitlines(text.unwrap_or_default()) {
        if let Some((key, value)) = line.split_once(separator) {
            result.insert(python_strip(key).to_owned(), python_strip(value).to_owned());
        }
    }
    result
}

/// `common.counters(text)`.
pub fn counters(text: Option<&str>) -> Map<String, Value> {
    key_values(text, " ")
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| {
            let first = split_ws(&v).first().copied();
            (k, number(first))
        })
        .collect()
}

/// `re.findall(pattern, text)` for a pattern with two groups.
pub fn findall2(pattern: &Regex, text: &str) -> Vec<(String, String)> {
    pattern
        .captures_iter(text)
        .map(|c| {
            (
                c.get(1).map_or("", |m| m.as_str()).to_owned(),
                c.get(2).map_or("", |m| m.as_str()).to_owned(),
            )
        })
        .collect()
}

/// `dict(re.findall(pattern, text))` (a later key wins).
pub fn findall_map(pattern: &Regex, text: &str) -> BTreeMap<String, String> {
    findall2(pattern, text).into_iter().collect()
}

/// A regex compiled once.
#[macro_export]
macro_rules! regex {
    ($pattern:expr) => {{
        static PATTERN: std::sync::LazyLock<y2p_core::observation_support::regex::Regex> =
            std::sync::LazyLock::new(|| {
                y2p_core::observation_support::regex::Regex::new($pattern).expect("pattern")
            });
        &*PATTERN
    }};
}

fn as_i128(value: &Value) -> Option<i128> {
    match value {
        Value::Bool(b) => Some(i128::from(*b)),
        Value::Number(n) => n.as_i64().map(i128::from).or(n.as_u64().map(i128::from)),
        _ => None,
    }
}

fn num(value: &Value) -> Option<f64> {
    y2p_core::py::number(Some(value))
}

/// `d.get(key) == other` where a missing key is None.
pub fn get_eq(map: &Map<String, Value>, key: &str, other: &Value) -> bool {
    map.get(key).unwrap_or(&Value::Null).py_eq(other)
}

/// `value in sequence` for a list.
pub fn contains(list: &[Value], value: &Value) -> bool {
    list.iter().any(|item| item.py_eq(value))
}

/// `isinstance(value, int)`: an int or a bool, as an exact integer.
pub fn is_int(value: Option<&Value>) -> Option<i128> {
    value.and_then(as_i128)
}

/// `isinstance(value, (int, float))` as a float.
pub fn is_number(value: Option<&Value>) -> Option<f64> {
    value.and_then(num)
}

/// `0 <= now - stamp < window` for a monotonic seconds stamp read from JSON.
pub fn fresh_seconds(stamp: Option<&Value>, window: f64) -> bool {
    is_number(stamp).is_some_and(|t| (0.0..window).contains(&(clock::monotonic() - t)))
}

/// Path.name.
pub fn name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The name of the parent directory (`path.parent.name`).
pub fn parent_name(path: &Path) -> String {
    path.parent().map(name).unwrap_or_default()
}

/// `os.path.realpath(path)` (non-strict `Path.resolve()`): symlinks followed where they exist, the
/// rest kept as written.
pub fn realpath(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut pending: Vec<OsString> = Vec::new();
    push_components(&mut pending, &absolute);
    let mut resolved = PathBuf::from("/");
    let mut links = 0;
    while let Some(part) = pending.pop() {
        match part.to_str() {
            Some(".") | Some("") => continue,
            Some("..") => {
                resolved.pop();
                continue;
            }
            _ => {}
        }
        let candidate = resolved.join(&part);
        match std::fs::symlink_metadata(&candidate) {
            Ok(meta) if meta.file_type().is_symlink() && links < 40 => {
                links += 1;
                let Ok(target) = std::fs::read_link(&candidate) else {
                    resolved = candidate;
                    continue;
                };
                if target.is_absolute() {
                    resolved = PathBuf::from("/");
                }
                push_components(&mut pending, &target);
            }
            _ => resolved = candidate,
        }
    }
    resolved
}

fn push_components(pending: &mut Vec<OsString>, path: &Path) {
    let parts: Vec<OsString> = path
        .components()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n.to_os_string()),
            Component::ParentDir => Some("..".into()),
            _ => None,
        })
        .collect();
    pending.extend(parts.into_iter().rev());
}

/// `sorted(directory.glob(pattern))` for a one-component pattern (not capped, unlike `ctx.glob`).
pub fn glob_dir(directory: &Path, pattern: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| root::fnmatch(pattern, n))
        })
        .map(|e| e.path())
        .collect();
    found.sort();
    found
}

/// `datetime.now(timezone.utc).isoformat()`.
pub fn wall_timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = now.as_secs() as i64;
    let micros = now.subsec_micros();
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let clock = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    );
    if micros == 0 {
        format!("{clock}+00:00")
    } else {
        format!("{clock}.{micros:06}+00:00")
    }
}

/// `ctx.record(workload, parameters)`.
pub fn record(root: &Root, workload: &str, parameters: Map<String, Value>) -> Map<String, Value> {
    let versions = root.json_object("/etc/y2linux/versions.json");
    let get = |key: &str| versions.get(key).cloned().unwrap_or(Value::Null);
    let mut value = Map::new();
    value.insert("wall_timestamp".into(), json!(wall_timestamp()));
    value.insert("monotonic_ns".into(), json!(clock::monotonic_ns()));
    value.insert(
        "boot_id".into(),
        text(root.read("/proc/sys/kernel/random/boot_id")),
    );
    value.insert("y2linux_commit".into(), get("build_git_commit"));
    value.insert("reborn_commit".into(), get("reborn_source_commit"));
    value.insert(
        "kernel".into(),
        text(root.read("/proc/sys/kernel/osrelease")),
    );
    value.insert("rootfs_release".into(), get("rootfs_version"));
    value.insert("build_id".into(), text(root.read("/etc/y2linux/build-id")));
    value.insert("workload".into(), json!(workload));
    value.insert("parameters".into(), Value::Object(parameters));
    value.insert("units".into(), json!({}));
    value.insert("result".into(), Value::Null);
    value.insert("failure".into(), Value::Null);
    value.insert("evidence_level".into(), json!("IMPLEMENTED"));
    value
}

/// `{p.name: read(p) for p in ctx.glob(pattern)}` (or the parent's name).
pub fn reads_by_name(root: &Root, pattern: &str, parent: bool) -> Value {
    let mut map = Map::new();
    for path in root.glob(pattern) {
        let key = if parent {
            parent_name(&path)
        } else {
            name(&path)
        };
        map.insert(key, text(read(&path)));
    }
    Value::Object(map)
}

/// The `boot_id` this boot reads (`ctx.read('/proc/sys/kernel/random/boot_id')`) as JSON.
pub fn boot_id(root: &Root) -> Value {
    text(root.read("/proc/sys/kernel/random/boot_id"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ints_digits_and_dicts_as_python() {
        assert_eq!(int(" \u{661}_2 "), Some(12));
        assert_eq!(int("\u{b2}"), None);
        assert!(isdigit("\u{b2}1") && isdigit("\u{661}") && !isdigit("") && !isdigit("1 "));
        assert_eq!(
            int_or_raise("x").unwrap_err().message(),
            "invalid literal for int() with base 10: 'x'"
        );
        let pairs = py_dict(&json!([["a", 1], "bc", {"x": 1, "y": 2}, [2, 3]])).unwrap();
        assert_eq!(pairs["a"], json!(1));
        assert_eq!(pairs["b"], json!("c"));
        assert_eq!(pairs["x"], json!("y"));
        assert!(unsortable(&json!({"p": pairs})));
        assert_eq!(
            py_dict(&json!(["abc"])).unwrap_err().message(),
            "dictionary update sequence element #0 has length 3; 2 is required"
        );
        assert!(matches!(
            py_dict(&json!(5)),
            Err(Failure::Crash {
                class: "TypeError",
                ..
            })
        ));
        assert_eq!(
            no_get(&json!([1])).message(),
            "'list' object has no attribute 'get'"
        );
    }

    #[test]
    fn lines_and_words_split_as_python() {
        assert_eq!(
            splitlines("a\r\nb\rc\n\nd\u{85}e\u{2028}f\u{b}g\n"),
            ["a", "b", "c", "", "d", "e", "f", "g"]
        );
        assert!(splitlines("").is_empty());
        assert_eq!(split_ws(" a\u{1c}b  c\t"), ["a", "b", "c"]);
        assert_eq!(
            key_values(Some("Name:\treborn\nVmRSS:  12 kB\nbad\n"), ":")["VmRSS"],
            "12 kB"
        );
        assert_eq!(
            counters(Some("ctxt 5\nintr 7 1 2\nempty\n"))["intr"],
            json!(7)
        );
    }

    #[test]
    fn realpath_follows_links_that_exist() {
        let dir = std::env::temp_dir().join(format!("y2-realpath-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("devices/x")).unwrap();
        std::os::unix::fs::symlink("../devices/x", dir.join("devices/link")).unwrap();
        let real = std::fs::canonicalize(&dir).unwrap();
        assert_eq!(
            realpath(&dir.join("devices/link/missing/../y")),
            real.join("devices/x/y")
        );
        assert_eq!(realpath(&dir.join("absent/z")), real.join("absent/z"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn timestamps_look_like_isoformat() {
        let stamp = wall_timestamp();
        assert!(
            stamp.ends_with("+00:00") && stamp.as_bytes()[10] == b'T',
            "{stamp}"
        );
    }
}
