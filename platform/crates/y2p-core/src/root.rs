//! Device paths below a root: `/` on the player, a fixture tree in tests (`Y2_ROOT`, as Python's
//! `Context(root)`).
// SPDX-License-Identifier: GPL-2.0-only
use crate::{
    clock::{Clock, Kernel},
    command::{FixtureFiles, Outcome, Processes, Runner, Scripted},
};
use serde_json::Value;
use std::{
    env, fmt,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

/// `ctx.read`'s default limit in characters.
pub const READ_LIMIT: usize = 131_072;

/// Where the platform's device paths are, and what answers its subprocess requests and clock reads:
/// the player's own (`Root::new("/")`), or a fixture's. Tests inject their own runner and clock here
/// instead of patching process-wide state.
#[derive(Clone)]
pub struct Root {
    base: PathBuf,
    runner: Arc<dyn Runner>,
    clock: Arc<dyn Clock>,
}

impl fmt::Debug for Root {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Root").field("base", &self.base).finish()
    }
}

impl Root {
    /// The device paths below `base`: the player's processes at `/`, the fixture answers
    /// (`Y2_FIXTURE_COMMANDS`, `Y2_FIXTURE_CALLS`) below any other base; the kernel clocks.
    pub fn new(base: impl Into<PathBuf>) -> Self {
        let base = base.into();
        let runner: Arc<dyn Runner> = if base == Path::new("/") {
            Arc::new(Processes)
        } else {
            Arc::new(FixtureFiles)
        };
        Self {
            base,
            runner,
            clock: Arc::new(Kernel),
        }
    }

    /// The real root, or the fixture tree a host test names in `Y2_ROOT`.
    pub fn from_env() -> Self {
        Self::new(env::var_os("Y2_ROOT").map_or_else(|| PathBuf::from("/"), PathBuf::from))
    }

    /// A fixture tree for a test (as `new`; a base of `/` would be the player itself).
    pub fn fixture(base: impl Into<PathBuf>) -> Self {
        let root = Self::new(base);
        assert!(!root.is_real(), "a fixture root is not the player's");
        root
    }

    /// This root with its subprocess requests answered by `runner`.
    pub fn with_runner(mut self, runner: Arc<dyn Runner>) -> Self {
        self.runner = runner;
        self
    }

    /// This root with its requests answered from `answers` (argv → outcome, anything else
    /// `fixture`) and recorded: `calls()` returns them.
    pub fn with_answers(self, answers: Vec<(Vec<String>, Outcome)>) -> Self {
        self.with_runner(Arc::new(Scripted::answers(answers)))
    }

    /// This root with its clock reads answered by `clock`.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// What answers this root's subprocess requests (`command::run`).
    pub fn runner(&self) -> &dyn Runner {
        &*self.runner
    }

    /// The requests this root's runner recorded (argv, timeout); none for the player's or the
    /// fixture files' runner (those go to `Y2_FIXTURE_CALLS`).
    pub fn calls(&self) -> Vec<(Vec<String>, f64)> {
        self.runner.calls()
    }

    /// The clocks this root reads.
    pub fn clock(&self) -> &dyn Clock {
        &*self.clock
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

    /// `ctx.read('/proc/sys/kernel/random/boot_id')`: this boot's id, which records carry to tell a
    /// fact of this boot from one a restart left behind (`None` when unreadable; as a value, null).
    pub fn boot_id(&self) -> Option<String> {
        // The player's boot id cannot change while a process runs: read once. A fixture root's is a
        // file a test may rewrite, so it is read each time.
        static REAL: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        if self.is_real() {
            return REAL
                .get_or_init(|| self.read("/proc/sys/kernel/random/boot_id"))
                .clone();
        }
        self.read("/proc/sys/kernel/random/boot_id")
    }

    /// Whether a record's `boot_id` field names this boot: the same text as this boot's id, or no id
    /// (null or missing) while this boot's id is unreadable; anything else never (as
    /// `lenient::BootId::is`, and as Python's `record.get('boot_id') == boot_id`).
    pub fn is_this_boot(&self, field: Option<&Value>) -> bool {
        match (field, self.boot_id()) {
            (None | Some(Value::Null), None) => true,
            (Some(Value::String(text)), Some(boot)) => *text == boot,
            _ => false,
        }
    }

    /// `ctx.integer`: Python `int()` of the stripped text.
    pub fn integer(&self, name: &str) -> Option<i64> {
        self.read(name).and_then(|text| parse_int(&text))
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

impl Root {
    /// `ctx.glob(pattern)`: pathlib's glob below the root (`*`, `?`, `[...]` within one component; dot
    /// files match too, symlinked directories are followed), sorted by path components, at most 256.
    pub fn glob(&self, pattern: &str) -> Vec<PathBuf> {
        let mut found = vec![self.base.clone()];
        for component in pattern
            .trim_start_matches('/')
            .split('/')
            .filter(|c| !c.is_empty())
        {
            let wild = component.contains(['*', '?', '[']);
            let mut next = Vec::new();
            for directory in &found {
                if !wild {
                    let path = directory.join(component);
                    if path.symlink_metadata().is_ok() {
                        next.push(path);
                    }
                    continue;
                }
                let Ok(entries) = std::fs::read_dir(directory) else {
                    continue;
                };
                for entry in entries.flatten() {
                    if entry
                        .file_name()
                        .to_str()
                        .is_some_and(|name| fnmatch(component, name))
                    {
                        next.push(entry.path());
                    }
                }
            }
            found = next;
        }
        found.sort_by(|a, b| a.components().cmp(b.components()));
        found.truncate(256);
        found
    }
}

/// `fnmatch.fnmatchcase` for one path component.
pub fn fnmatch(pattern: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    fn class(p: &[char], c: char) -> Option<(bool, usize)> {
        // p starts after '['; returns (matched, length including the closing bracket).
        let (negate, mut i) = if matches!(p.first(), Some('!')) {
            (true, 1)
        } else {
            (false, 0)
        };
        let start = i;
        let mut matched = false;
        while i < p.len() {
            if p[i] == ']' && i > start {
                return Some((matched != negate, i + 1));
            }
            if i + 2 < p.len() && p[i + 1] == '-' && p[i + 2] != ']' {
                matched |= p[i] <= c && c <= p[i + 2];
                i += 3;
            } else {
                matched |= p[i] == c;
                i += 1;
            }
        }
        None // no closing bracket: '[' is literal
    }
    fn go(p: &[char], n: &[char]) -> bool {
        match p.first() {
            None => n.is_empty(),
            Some('*') => (0..=n.len()).any(|k| go(&p[1..], &n[k..])),
            Some('?') => !n.is_empty() && go(&p[1..], &n[1..]),
            Some('[') => match (n.first(), class(&p[1..], *n.first().unwrap_or(&'\0'))) {
                (Some(_), Some((true, len))) => go(&p[1 + len..], &n[1..]),
                (Some(_), Some((false, _))) | (None, _) => false,
                (Some(c), None) => *c == '[' && go(&p[1..], &n[1..]),
            },
            Some(c) => n.first() == Some(c) && go(&p[1..], &n[1..]),
        }
    }
    go(&p, &n)
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

/// A sysfs attribute kept open and read again from offset 0 each time (kernfs makes the text anew at
/// offset 0): one `pread` instead of open, fstat, read, read and close, for the attributes a daemon
/// reads every second. The same answer as [`read`]; a failed read drops the descriptor, so the next
/// read opens the file anew. Only for files that are never replaced (sysfs on the player).
pub struct Attribute {
    path: PathBuf,
    file: Option<File>,
    buffer: Vec<u8>,
}

impl Attribute {
    pub fn new(path: PathBuf) -> Attribute {
        Attribute {
            path,
            file: None,
            buffer: vec![0; 4096],
        }
    }

    /// [`read`]'s answer for this file now.
    pub fn read(&mut self, limit: usize) -> Option<String> {
        use std::os::unix::fs::FileExt;
        if self.file.is_none() {
            self.file = File::open(&self.path).ok();
        }
        let file = self.file.as_ref()?;
        let cap = (limit + 1) * 4;
        let mut filled = 0;
        loop {
            if filled == self.buffer.len() {
                if filled >= cap {
                    break;
                }
                self.buffer.resize((filled * 2).min(cap), 0);
            }
            match file.read_at(&mut self.buffer[filled..], filled as u64) {
                Ok(0) => break,
                Ok(n) => {
                    filled += n;
                    // kernfs answers at offset 0 with all it has: a short read is the end.
                    if filled < self.buffer.len() {
                        break;
                    }
                }
                Err(_) => {
                    self.file = None;
                    return None;
                }
            }
        }
        let text = String::from_utf8_lossy(&self.buffer[..filled.min(cap)]);
        if text.chars().count() > limit {
            return None;
        }
        Some(python_strip(&text).to_owned())
    }
}

/// Python `str.strip()` (`text::strip`).
pub use crate::text::strip as python_strip;

/// An integer read from a kernel or record text: the trimmed text as an i64 (sign and ASCII digits);
/// anything else none.
pub fn parse_int(text: &str) -> Option<i64> {
    text.trim().parse().ok()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn an_attribute_reads_as_read_does_and_again_from_its_start() {
        let dir = tempdir::Dir::new();
        let path = dir.path().join("voltage_now");
        fs::write(&path, " 3912000\n").unwrap();
        let mut attribute = Attribute::new(path.clone());
        assert_eq!(attribute.read(READ_LIMIT), read(&path, READ_LIMIT));
        assert_eq!(attribute.read(READ_LIMIT).as_deref(), Some("3912000"));
        // The same file with new text (sysfs makes it anew at offset 0).
        fs::write(&path, "3800000\n").unwrap();
        assert_eq!(attribute.read(READ_LIMIT).as_deref(), Some("3800000"));
        // Longer than the first buffer, and over a limit.
        let long = "x".repeat(10_000);
        fs::write(&path, &long).unwrap();
        assert_eq!(attribute.read(READ_LIMIT).as_deref(), Some(long.as_str()));
        assert_eq!(attribute.read(9_999), None);
        assert_eq!(read(&path, 9_999), None);
        // Gone: none, and opened anew once it is back.
        fs::remove_file(&path).unwrap();
        let mut missing = Attribute::new(path.clone());
        assert_eq!(missing.read(READ_LIMIT), None);
        fs::write(&path, "1\n").unwrap();
        assert_eq!(missing.read(READ_LIMIT).as_deref(), Some("1"));
    }

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
    fn integers_and_json_read_as_written() {
        let (dir, root) = fixture();
        for (text, value) in [
            ("42\n", Some(42)),
            ("-7", Some(-7)),
            ("+3", Some(3)),
            ("1_000", None),
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

    #[test]
    fn globs_follow_pathlib() {
        let (dir, root) = fixture();
        for name in [
            "sys/class/udc/a/state",
            "sys/class/udc/.hidden/state",
            "sys/class/udc/b/other",
            "sys/devices/system/cpu/cpu0/cpuidle/state1",
            "sys/devices/system/cpu/cpu10/cpuidle/state0",
            "sys/devices/system/cpu/cpufreq/x",
        ] {
            fs::create_dir_all(dir.path().join(name).parent().unwrap()).unwrap();
            fs::write(dir.path().join(name), "").unwrap();
        }
        let names = |pattern: &str| -> Vec<String> {
            root.glob(pattern)
                .iter()
                .map(|p| p.strip_prefix(dir.path()).unwrap().display().to_string())
                .collect()
        };
        assert_eq!(
            names("/sys/class/udc/*/state"),
            ["sys/class/udc/.hidden/state", "sys/class/udc/a/state"]
        );
        assert_eq!(
            names("/sys/devices/system/cpu/cpu[0-9]*/cpuidle/state*"),
            [
                "sys/devices/system/cpu/cpu0/cpuidle/state1",
                "sys/devices/system/cpu/cpu10/cpuidle/state0"
            ]
        );
        assert!(names("/sys/class/none/*").is_empty());
        assert!(
            fnmatch("slow_*", "slow_x")
                && !fnmatch("slow_*", "fast")
                && fnmatch("a[!b]c", "axc")
                && !fnmatch("a[!b]c", "abc")
                && fnmatch("x[", "x[")
        );
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
