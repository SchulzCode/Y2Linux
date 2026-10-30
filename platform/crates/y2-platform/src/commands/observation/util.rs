//! Text and value helpers the observation code shares, and the record every measurement carries.
//! Their line splits, integer reading and equality (`1 == 1.0 == True`) are Python's because the
//! records and outputs were defined by them: another split would change what the snapshot reports.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::Failure;
use regex::Regex;
use serde::de::{value::MapDeserializer, DeserializeOwned};
use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
pub use y2p_contract::ObservationRecord;
pub use y2p_core::paths::realpath;
pub use y2p_core::paths::{name, parent_name};
pub use y2p_core::text::{split as split_ws, splitlines};
use y2p_core::{
    root::{self, python_strip},
    Root,
};

/// A host path's text, trimmed (`Root::read` for a path that is no device path).
pub fn read(path: &Path) -> Option<String> {
    root::read(path, root::READ_LIMIT)
}

pub fn read_limited(path: &Path, limit: usize) -> Option<String> {
    root::read(path, limit)
}

/// A decimal integer as the kernel and the platform write it (surrounding whitespace allowed).
pub fn int(text: &str) -> Option<i64> {
    y2p_core::root::parse_int(text)
}

/// `int(text)` where nothing catches the error: the value, or the error record
/// (`invalid literal for int() with base 10: '…'`).
pub fn int_or_raise(text: &str) -> Result<i64, Failure> {
    int(text).ok_or_else(|| Failure::int_literal(text))
}

/// Not empty and only the digits 0-9.
pub fn isdigit(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// JSON of an optional str.
pub fn text(value: Option<String>) -> Value {
    value.map_or(Value::Null, Value::String)
}

/// `key<separator>value` lines as a map, trimmed; a later key wins.
pub fn key_values(text: Option<&str>, separator: &str) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    for line in splitlines(text.unwrap_or_default()) {
        if let Some((key, value)) = line.split_once(separator) {
            result.insert(python_strip(key).to_owned(), python_strip(value).to_owned());
        }
    }
    result
}

/// `name value…` lines as name → its first value (none where it is no integer).
pub fn counters(text: Option<&str>) -> Counted {
    key_values(text, " ")
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| {
            let first = split_ws(&v).first().copied();
            (k, first.and_then(int))
        })
        .collect()
}

/// Counters by name (`counters`).
pub type Counted = BTreeMap<String, Option<i64>>;

/// Every match's two groups.
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

fn as_i128(value: &Value) -> Option<i128> {
    match value {
        Value::Bool(b) => Some(i128::from(*b)),
        Value::Number(n) => n.as_i64().map(i128::from).or(n.as_u64().map(i128::from)),
        _ => None,
    }
}

/// The named fields of a record as `T` (each field a `Value`; a missing one is null, as `dict.get`
/// gives None), for the checks that read them; the record itself can still be shown whole. `T` takes
/// any object, so nothing about the record's other fields or types is refused here.
pub fn fields<T: DeserializeOwned + Default>(record: &Map<String, Value>) -> T {
    let entries = record.iter().map(|(key, value)| (key.as_str(), value));
    T::deserialize(MapDeserializer::<_, serde_json::Error>::new(entries)).unwrap_or_default()
}

/// `fields` of the JSON object at a device path (all null for a missing or unreadable file, or one
/// that is no object).
pub fn record_fields<T: DeserializeOwned + Default>(root: &Root, name: &str) -> T {
    fields(&root.json_object(name))
}

/// `value in sequence` for a list.
pub fn contains(list: &[Value], value: &Value) -> bool {
    list.contains(value)
}

/// `isinstance(value, int)`: an int or a bool, as an exact integer.
pub fn is_int(value: Option<&Value>) -> Option<i128> {
    value.and_then(as_i128)
}

/// The entries of `directory` matching a one-component `pattern`, sorted (not capped, unlike
/// `Root::glob`).
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

/// The record a measurement carries: when (wall clock, monotonic), which boot and build, the
/// workload and its parameters; the caller fills in `result` and `failure`.
pub fn record(root: &Root, workload: &str, parameters: Map<String, Value>) -> ObservationRecord {
    let versions = root.json_object("/etc/y2linux/versions.json");
    let version = |key: &str| versions.get(key).and_then(Value::as_str).map(str::to_owned);
    ObservationRecord {
        wall_timestamp: y2p_core::devices_support::utc_isoformat_now(),
        monotonic_ns: root.clock().monotonic_ns(),
        boot_id: root.boot_id(),
        y2linux_commit: version("build_git_commit"),
        reborn_commit: version("reborn_source_commit"),
        kernel: root.read("/proc/sys/kernel/osrelease"),
        rootfs_release: version("rootfs_version"),
        build_id: root.read("/etc/y2linux/build-id"),
        workload: workload.into(),
        parameters,
        units: Map::new(),
        result: Value::Null,
        failure: Value::Null,
        evidence_level: "IMPLEMENTED".into(),
        extra: Map::new(),
    }
}

/// Each file matching `pattern` by its name (or its directory's), with its text.
pub fn reads_by_name(root: &Root, pattern: &str, parent: bool) -> Readings {
    let mut map = Readings::new();
    for path in root.glob(pattern) {
        let key = if parent {
            parent_name(&path)
        } else {
            name(&path)
        };
        map.insert(key, read(&path));
    }
    map
}

/// A JSON object's entries (none for another value).
pub fn map(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// Kernel attribute texts by file (or parent directory) name; null where a file did not read.
pub type Readings = BTreeMap<String, Option<String>>;

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use serde_json::json;

    #[test]
    fn ints_and_digits() {
        assert_eq!(int(" 12\n"), Some(12));
        assert_eq!(int("1_000"), None);
        assert_eq!(int("\u{661}"), None);
        assert!(isdigit("0123") && !isdigit("\u{661}") && !isdigit("") && !isdigit("1 "));
        assert_eq!(
            int_or_raise("x").unwrap_err().message(),
            "invalid literal for int() with base 10: 'x'"
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
            Some(7)
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
}
