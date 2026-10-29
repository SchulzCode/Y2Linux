//! Python behaviour the service group's code relies on: `os.path.realpath` (pathlib `resolve()`),
//! `observe.mountinfo` with every field, `str()` of a parsed JSON value, `json.loads(strict=False)`,
//! the key order of one parsed object, and `product_sleep.lock_holders`.
// SPDX-License-Identifier: GPL-2.0-only
use serde::de::{Deserializer, IgnoredAny, MapAccess, Visitor};
use serde_json::Value;
use std::{
    collections::HashMap,
    fmt,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};
use y2p_core::{
    json::{float_repr, sorted_entries},
    py, Root,
};

/// `os.path.realpath(path, strict=False)` (posixpath, Python 3.15): symlinks resolved where they
/// exist, the rest of the path taken as written with `..` removed lexically.
pub fn realpath(path: &Path) -> PathBuf {
    let text = path.to_string_lossy().into_owned();
    let mut rest: Vec<Option<String>> = text.split('/').rev().map(|s| Some(s.to_owned())).collect();
    let mut current = if text.starts_with('/') {
        "/".to_owned()
    } else {
        std::env::current_dir()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "/".into())
    };
    let mut seen: HashMap<String, Option<String>> = HashMap::new();
    let mut links = 0;
    while let Some(name) = rest.pop() {
        let Some(name) = name else {
            // The end of a resolved link's target: remember what it resolved to.
            if let Some(Some(link)) = rest.pop() {
                seen.insert(link, Some(current.clone()));
            }
            continue;
        };
        if name.is_empty() || name == "." {
            continue;
        }
        if name == ".." {
            current = match current.rfind('/') {
                Some(0) | None => "/".into(),
                Some(index) => current[..index].to_owned(),
            };
            continue;
        }
        let next = if current == "/" {
            format!("/{name}")
        } else {
            format!("{current}/{name}")
        };
        let Ok(meta) = std::fs::symlink_metadata(&next) else {
            current = next;
            continue;
        };
        if !meta.file_type().is_symlink() {
            current = next;
            continue;
        }
        match seen.get(&next) {
            Some(Some(resolved)) => {
                current = resolved.clone();
                continue;
            }
            Some(None) => {
                // A loop: not strict, so the path stays as it is.
                current = next;
                continue;
            }
            None => {}
        }
        links += 1;
        let Ok(target) = std::fs::read_link(&next) else {
            current = next;
            continue;
        };
        if links > 40 {
            current = next;
            continue;
        }
        let target = target.to_string_lossy().into_owned();
        seen.insert(next.clone(), None);
        if target.starts_with('/') {
            current = "/".into();
        }
        rest.push(Some(next));
        rest.push(None);
        rest.extend(target.split('/').rev().map(|s| Some(s.to_owned())));
    }
    PathBuf::from(current)
}

/// One line of /proc/self/mountinfo with the fields `observe.mountinfo` keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mount {
    pub mount_id: i64,
    pub device_id: String,
    pub path: String,
    pub options: Vec<String>,
    pub filesystem: String,
    pub source: String,
    pub super_options: Vec<String>,
}

fn unescape(value: &str) -> String {
    let (mut out, mut rest) = (String::new(), value);
    while let Some(index) = rest.find('\\') {
        out.push_str(&rest[..index]);
        let code = &rest[index + 1..];
        match ["040", "011", "012", "134"]
            .iter()
            .find(|c| code.starts_with(**c))
        {
            Some(c) => {
                out.push(char::from(u8::from_str_radix(c, 8).expect("octal")));
                rest = &code[3..];
            }
            None => {
                out.push('\\');
                rest = code;
            }
        }
    }
    out + rest
}

/// `observe.mountinfo`.
pub fn mountinfo(text: &str) -> Vec<Mount> {
    let mut result = Vec::new();
    for line in text.lines() {
        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };
        let a: Vec<&str> = left.split_whitespace().collect();
        let b: Vec<&str> = right.split_whitespace().collect();
        if a.len() < 6
            || b.len() < 3
            || a[0].is_empty()
            || !a[0].bytes().all(|c| c.is_ascii_digit())
        {
            continue;
        }
        let Ok(mount_id) = a[0].parse() else {
            continue;
        };
        result.push(Mount {
            mount_id,
            device_id: a[2].to_owned(),
            path: unescape(a[4]),
            options: a[5].split(',').map(str::to_owned).collect(),
            filesystem: b[0].to_owned(),
            source: unescape(b[1]),
            super_options: b[2].split(',').map(str::to_owned).collect(),
        });
    }
    result
}

/// `str(value)` of a parsed JSON value.
pub fn str_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => repr_of(other),
    }
}

/// `repr(value)` of a parsed JSON value.
pub fn repr_of(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) if n.is_f64() => float_repr(n.as_f64().unwrap_or(0.0)),
        Value::Number(n) => n.to_string(),
        Value::String(text) => py::repr(text),
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(repr_of).collect::<Vec<_>>().join(", ")
        ),
        // Sorted keys: the records it spells are written sorted.
        Value::Object(map) => format!(
            "{{{}}}",
            sorted_entries(map)
                .into_iter()
                .map(|(k, v)| format!("{}: {}", py::repr(k), repr_of(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Python `a == b` for parsed JSON values: numbers by value (`1 == 1.0`, `True == 1`), containers
/// element by element.
pub fn py_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_i64(), y.as_i64()) {
            (Some(x), Some(y)) => x == y,
            _ => match (x.as_u64(), y.as_u64()) {
                (Some(x), Some(y)) => x == y,
                _ => x.as_f64() == y.as_f64(),
            },
        },
        (Value::Bool(_), Value::Number(_)) | (Value::Number(_), Value::Bool(_)) => {
            py::number(Some(a)) == py::number(Some(b))
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

/// `text[:limit]` by characters.
pub fn prefix(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// `value in (a, b, ...)` for strings: only an equal string matches.
pub fn is_one_of(value: Option<&Value>, choices: &[&str]) -> bool {
    matches!(value, Some(Value::String(s)) if choices.contains(&s.as_str()))
}

/// `type(value) in (int, float)`: a number, not a bool.
pub fn int_or_float(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

/// `json.loads(text, strict=False)`: raw control characters inside strings are accepted.
pub fn loads_lenient(text: &str) -> Option<Value> {
    let mut escaped = String::with_capacity(text.len());
    let mut in_string = false;
    let mut backslash = false;
    for c in text.chars() {
        if in_string {
            if backslash {
                backslash = false;
            } else if c == '\\' {
                backslash = true;
            } else if c == '"' {
                in_string = false;
            } else if (c as u32) < 0x20 {
                escaped.push_str(&format!("\\u{:04x}", c as u32));
                continue;
            }
        } else if c == '"' {
            in_string = true;
        }
        escaped.push(c);
    }
    serde_json::from_str(&escaped).ok()
}

/// The keys of the object `field` of a JSON document in the order they are written (a Python dict's
/// iteration order; a `serde_json::Map` sorts them). Duplicates keep their first position.
pub fn key_order(text: &str, field: &str) -> Vec<String> {
    struct Top<'a>(&'a str);
    struct Keys;
    impl<'de> Visitor<'de> for Keys {
        type Value = Vec<String>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("any value")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Vec<String>, A::Error> {
            let mut keys: Vec<String> = Vec::new();
            while let Some(key) = map.next_key::<String>()? {
                map.next_value::<IgnoredAny>()?;
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
            Ok(keys)
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Vec<String>, A::Error> {
            while seq.next_element::<IgnoredAny>()?.is_some() {}
            Ok(Vec::new())
        }
        fn visit_bool<E>(self, _: bool) -> Result<Vec<String>, E> {
            Ok(Vec::new())
        }
        fn visit_i64<E>(self, _: i64) -> Result<Vec<String>, E> {
            Ok(Vec::new())
        }
        fn visit_u64<E>(self, _: u64) -> Result<Vec<String>, E> {
            Ok(Vec::new())
        }
        fn visit_f64<E>(self, _: f64) -> Result<Vec<String>, E> {
            Ok(Vec::new())
        }
        fn visit_str<E>(self, _: &str) -> Result<Vec<String>, E> {
            Ok(Vec::new())
        }
        fn visit_unit<E>(self) -> Result<Vec<String>, E> {
            Ok(Vec::new())
        }
    }
    struct KeysSeed;
    impl<'de> serde::de::DeserializeSeed<'de> for KeysSeed {
        type Value = Vec<String>;
        fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Vec<String>, D::Error> {
            d.deserialize_any(Keys)
        }
    }
    impl<'de> Visitor<'de> for Top<'_> {
        type Value = Vec<String>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("an object")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Vec<String>, A::Error> {
            let mut found = Vec::new();
            while let Some(key) = map.next_key::<String>()? {
                if key == self.0 {
                    // A later duplicate wins, as in a dict.
                    found = map.next_value_seed(KeysSeed)?;
                } else {
                    map.next_value::<IgnoredAny>()?;
                }
            }
            Ok(found)
        }
    }
    let mut deserializer = serde_json::Deserializer::from_str(text);
    deserializer.deserialize_map(Top(field)).unwrap_or_default()
}

/// `product_sleep.lock_holders(ctx, meta)` reduced to "any live flock holder of this file".
pub fn lock_held(root: &Root, path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let dev = meta.dev();
    let (major, minor) = (libc::major(dev) as u64, libc::minor(dev) as u64);
    for line in root.read("/proc/locks").unwrap_or_default().lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 6
            || fields[1] != "FLOCK"
            || fields[4].is_empty()
            || !fields[4].chars().all(|c| c.is_numeric())
        {
            continue;
        }
        let parts: Vec<&str> = fields[5].split(':').collect();
        let [a, b, c] = parts.as_slice() else {
            continue;
        };
        let parsed = (
            u64::from_str_radix(a, 16),
            u64::from_str_radix(b, 16),
            c.parse::<u64>(),
        );
        if let (Ok(a), Ok(b), Ok(c)) = parsed {
            if (a, b, c) == (major, minor, meta.ino()) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn values_print_as_python_str() {
        assert_eq!(str_of(&json!("x")), "x");
        assert_eq!(str_of(&json!(5)), "5");
        assert_eq!(str_of(&json!(1.0)), "1.0");
        assert_eq!(str_of(&json!(true)), "True");
        assert_eq!(str_of(&json!([1, "a", null])), "[1, 'a', None]");
        assert_eq!(
            str_of(&json!({"b": 1, "a": "it's"})),
            "{'a': \"it's\", 'b': 1}"
        );
    }

    #[test]
    fn key_order_is_the_documents() {
        let text = r#"{"x": 1, "payloads": {"rootfs": {"a": 1}, "bootimg": {}, "recovery": [], "bootimg": 2}}"#;
        assert_eq!(
            key_order(text, "payloads"),
            ["rootfs", "bootimg", "recovery"]
        );
        assert!(key_order("[1]", "payloads").is_empty());
        assert!(key_order(r#"{"payloads": 3}"#, "payloads").is_empty());
    }

    #[test]
    fn lenient_json_takes_raw_tabs() {
        assert_eq!(
            loads_lenient("{\"printk\": \"4\t4\t1\t7\"}"),
            Some(json!({"printk": "4\t4\t1\t7"}))
        );
        assert_eq!(loads_lenient("nope"), None);
    }

    #[test]
    fn realpath_matches_posixpath() {
        let dir = std::env::temp_dir().join(format!("y2-realpath-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("devices/11230000.mmc/mmcblk0/mmcblk0p9")).unwrap();
        std::fs::create_dir_all(dir.join("dev/block")).unwrap();
        std::os::unix::fs::symlink(
            "../../devices/11230000.mmc/mmcblk0/mmcblk0p9",
            dir.join("dev/block/179:9"),
        )
        .unwrap();
        assert_eq!(
            realpath(&dir.join("dev/block/179:9")),
            dir.join("devices/11230000.mmc/mmcblk0/mmcblk0p9")
        );
        assert_eq!(
            realpath(&dir.join("dev/block/missing/../x")),
            dir.join("dev/block/x")
        );
        assert_eq!(
            realpath(&dir.join("dev/block/179:9/..")),
            dir.join("devices/11230000.mmc/mmcblk0")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
