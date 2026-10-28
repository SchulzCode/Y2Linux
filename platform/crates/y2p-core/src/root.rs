//! Device paths below a root: `/` on the player, a fixture tree in tests (`Y2_ROOT`, as Python's
//! `Context(root)`).
// SPDX-License-Identifier: GPL-2.0-only
use serde_json::Value;
use std::{
    env,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

/// `ctx.read`'s default limit in characters.
pub const READ_LIMIT: usize = 131_072;

#[derive(Clone, Debug)]
pub struct Root {
    base: PathBuf,
}

impl Root {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }

    /// The real root, or the fixture tree a host test names in `Y2_ROOT`.
    pub fn from_env() -> Self {
        Self::new(env::var_os("Y2_ROOT").map_or_else(|| PathBuf::from("/"), PathBuf::from))
    }

    /// Whether this is the player's own root; fixture roots never run device commands.
    pub fn is_real(&self) -> bool {
        self.base == Path::new("/")
    }

    /// `ctx.path`: the base joined with the absolute device path.
    pub fn path(&self, name: &str) -> PathBuf {
        self.base.join(name.trim_start_matches('/'))
    }

    /// `ctx.read`: text with surrounding whitespace removed; `None` when it cannot be read or is longer
    /// than `limit` characters. Invalid UTF-8 becomes U+FFFD as Python's `errors='replace'` does.
    pub fn read_limited(&self, name: &str, limit: usize) -> Option<String> {
        read(&self.path(name), limit)
    }

    pub fn read(&self, name: &str) -> Option<String> {
        self.read_limited(name, READ_LIMIT)
    }

    /// `ctx.integer`: Python `int()` of the stripped text.
    pub fn integer(&self, name: &str) -> Option<i64> {
        self.read(name).and_then(|text| python_int(&text))
    }

    /// `ctx.json` without a default: the parsed document, or `None` when missing or not JSON.
    pub fn json(&self, name: &str) -> Option<Value> {
        serde_json::from_str(&self.read(name)?).ok()
    }

    /// `ctx.json(name, {})`: an object, or an empty one for anything else.
    pub fn json_object(&self, name: &str) -> serde_json::Map<String, Value> {
        match self.json(name) {
            Some(Value::Object(map)) => map,
            _ => serde_json::Map::new(),
        }
    }
}

/// `common.read` on an absolute host path.
pub fn read(path: &Path, limit: usize) -> Option<String> {
    let file = File::open(path).ok()?;
    // Python reads `limit + 1` characters; one character is at most four bytes.
    let cap = (limit as u64 + 1) * 4;
    let mut bytes = Vec::new();
    file.take(cap).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    if text.chars().count() > limit {
        return None;
    }
    Some(python_strip(&text).to_owned())
}

/// Python `str.strip()`: Unicode whitespace plus the separators U+001C..U+001F.
pub fn python_strip(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c))
}

/// Python `int(text)` for the decimal forms the platform reads (sign, digits, `_` between digits).
pub fn python_int(text: &str) -> Option<i64> {
    let text = python_strip(text);
    let (sign, digits) = match text.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, text.strip_prefix('+').unwrap_or(text)),
    };
    let valid = !digits.is_empty()
        && !digits.starts_with('_')
        && !digits.ends_with('_')
        && !digits.contains("__")
        && digits.chars().all(|c| c.is_ascii_digit() || c == '_');
    if !valid {
        return None;
    }
    digits
        .replace('_', "")
        .parse::<i64>()
        .ok()
        .map(|v| sign * v)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::fs;

    fn fixture() -> (tempdir::Dir, Root) {
        let dir = tempdir::Dir::new();
        let root = Root::new(dir.path());
        (dir, root)
    }

    #[test]
    fn paths_and_reads_follow_ctx() {
        let (dir, root) = fixture();
        assert_eq!(root.path("/proc/uptime"), dir.path().join("proc/uptime"));
        assert_eq!(root.path("run/y2"), dir.path().join("run/y2"));
        fs::create_dir_all(dir.path().join("proc")).unwrap();
        fs::write(dir.path().join("proc/uptime"), " 12.5 3.0\n\x1c").unwrap();
        assert_eq!(root.read("/proc/uptime").as_deref(), Some("12.5 3.0"));
        assert_eq!(root.read("/missing"), None);
        fs::write(dir.path().join("proc/long"), "abcd").unwrap();
        assert_eq!(root.read_limited("/proc/long", 4).as_deref(), Some("abcd"));
        assert_eq!(root.read_limited("/proc/long", 3), None);
        // Four two-byte characters fit a limit of four characters.
        fs::write(dir.path().join("proc/wide"), "äöüß").unwrap();
        assert_eq!(root.read_limited("/proc/wide", 4).as_deref(), Some("äöüß"));
        fs::write(dir.path().join("proc/bad"), b"a\xffb").unwrap();
        assert_eq!(root.read("/proc/bad").as_deref(), Some("a\u{fffd}b"));
    }

    #[test]
    fn integers_and_json_follow_python() {
        let (dir, root) = fixture();
        for (text, value) in [
            ("42\n", Some(42)),
            ("-7", Some(-7)),
            ("+3", Some(3)),
            ("1_000", Some(1000)),
            ("1__0", None),
            ("0x10", None),
            ("", None),
            ("4.0", None),
        ] {
            fs::write(dir.path().join("n"), text).unwrap();
            assert_eq!(root.integer("/n"), value, "{text:?}");
        }
        fs::write(dir.path().join("j"), "[1]").unwrap();
        assert!(root.json_object("/j").is_empty());
        fs::write(dir.path().join("j"), "{\"a\": 1, \"a\": 2}").unwrap();
        assert_eq!(root.json_object("/j")["a"], 2);
        fs::write(dir.path().join("j"), "not json").unwrap();
        assert_eq!(root.json("/j"), None);
    }

    /// A self-cleaning directory (no tempfile crate in the vendored set).
    pub mod tempdir {
        use std::{
            path::PathBuf,
            sync::atomic::{AtomicU32, Ordering},
        };
        static NEXT: AtomicU32 = AtomicU32::new(0);
        pub struct Dir(PathBuf);
        impl Dir {
            pub fn new() -> Self {
                let path = std::env::temp_dir().join(format!(
                    "y2p-core-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::SeqCst)
                ));
                std::fs::create_dir_all(&path).unwrap();
                Self(path)
            }
            pub fn path(&self) -> &std::path::Path {
                &self.0
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }
}
