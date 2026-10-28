//! Python semantics the observation code leans on: `str.splitlines`/`split`, `int()` of text,
//! `common.key_values`/`counters`, `re.findall` pairs, `==` between parsed JSON values (`1 == 1.0 ==
//! True`), `os.path.realpath`, uncapped `Path.glob`, and `ctx.record`.
// SPDX-License-Identifier: GPL-2.0-only
use serde_json::{json, Map, Value};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use y2p_core::{
    clock,
    observation_support::regex::Regex,
    root::{self, python_int, python_strip},
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

/// `int(text)` (`common.number`) of a str.
pub fn int(text: &str) -> Option<i64> {
    python_int(text)
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

/// Python `a == b` for parsed JSON (bools and numbers compare numerically).
pub fn eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(_) | Value::Bool(_), Value::Number(_) | Value::Bool(_)) => {
            match (as_i128(a), as_i128(b)) {
                (Some(x), Some(y)) => x == y,
                _ => num(a) == num(b),
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| eq(a, b))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| eq(v, w)))
        }
        _ => a == b,
    }
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
    eq(map.get(key).unwrap_or(&Value::Null), other)
}

/// `value in sequence` for a list.
pub fn contains(list: &[Value], value: &Value) -> bool {
    list.iter().any(|item| eq(item, value))
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
    fn equality_is_pythons() {
        assert!(eq(&json!(1), &json!(1.0)) && eq(&json!(true), &json!(1)));
        assert!(!eq(&json!("1"), &json!(1)) && !eq(&json!(null), &json!(false)));
        assert!(eq(&json!({"a": [1, 2.0]}), &json!({"a": [true, 2]})));
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
