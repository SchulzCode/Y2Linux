//! `power.process_identity`: Reborn's (or its supervisor's) pid from `/run/reborn/<name>.pid`, accepted
//! only while `/proc/<pid>/exe` resolves to the expected executable, with its start time in clock ticks
//! (stat field 22) so a reused pid is told apart. The shutdown coordinator records it with an intent
//! and accepts an acknowledgement only from the same process; `status --reborn` reads it too.
// SPDX-License-Identifier: GPL-2.0-only
use crate::{
    json::{Object, ObjectExt},
    power_text::python_split,
    root::python_int,
    Root,
};
use std::{
    collections::HashMap,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

/// `os.path.realpath(path, strict=False)` (`Path.resolve()`): symlinks resolved as far as the path
/// exists, the rest kept as written; a loop leaves the looping link unresolved.
pub fn realpath(path: &Path) -> PathBuf {
    let text = path.to_string_lossy().into_owned();
    let mut rest: Vec<Option<String>> = text.split('/').rev().map(|s| Some(s.to_owned())).collect();
    let mut part_count = rest.len();
    let mut resolved = if text.starts_with('/') {
        "/".to_owned()
    } else {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "/".into())
    };
    let mut seen: HashMap<String, Option<String>> = HashMap::new();
    while part_count > 0 {
        let Some(name) = rest.pop() else { break };
        let Some(name) = name else {
            // A symlink's target is resolved: remember it for the link popped next.
            if let Some(Some(link)) = rest.pop() {
                seen.insert(link, Some(resolved.clone()));
            }
            continue;
        };
        part_count -= 1;
        if name.is_empty() || name == "." {
            continue;
        }
        if name == ".." {
            resolved = match resolved.rfind('/') {
                Some(0) | None => "/".into(),
                Some(index) => resolved[..index].to_owned(),
            };
            continue;
        }
        let candidate = if resolved == "/" {
            format!("/{name}")
        } else {
            format!("{resolved}/{name}")
        };
        let Ok(meta) = std::fs::symlink_metadata(&candidate) else {
            resolved = candidate;
            continue;
        };
        if !meta.file_type().is_symlink() {
            resolved = candidate;
            continue;
        }
        if let Some(known) = seen.get(&candidate) {
            match known {
                Some(target) => resolved = target.clone(),
                None => resolved = candidate, // a symlink loop: kept unresolved
            }
            continue;
        }
        let Ok(target) = std::fs::read_link(&candidate) else {
            resolved = candidate;
            continue;
        };
        let target = String::from_utf8_lossy(target.as_os_str().as_bytes()).into_owned();
        if target.starts_with('/') {
            resolved = "/".into();
        }
        seen.insert(candidate.clone(), None);
        rest.push(Some(candidate));
        rest.push(None);
        let parts: Vec<Option<String>> = target
            .split('/')
            .rev()
            .map(|s| Some(s.to_owned()))
            .collect();
        part_count += parts.len();
        rest.extend(parts);
    }
    PathBuf::from(resolved)
}

/// `{'pid': pid, 'start_ticks': ticks}` of a live process running `expected`, or `None`.
pub fn process_identity(root: &Root, name: &str, expected: &str) -> Option<Object> {
    let pid = root.integer(&format!("/run/reborn/{name}.pid"))?;
    if pid <= 1 {
        return None;
    }
    if realpath(&root.path(&format!("/proc/{pid}/exe"))) != realpath(&root.path(expected)) {
        return None;
    }
    let value = root.read(&format!("/proc/{pid}/stat"))?;
    let (_, tail) = value.rsplit_once(") ")?;
    let ticks = python_int(python_split(tail).get(19)?)?;
    Some(Object::new().with("pid", pid).with("start_ticks", ticks))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::root::tests::tempdir::Dir;
    use std::{fs, os::unix::fs::symlink};

    #[test]
    fn paths_resolve_as_os_path_realpath() {
        let dir = Dir::new();
        let base = fs::canonicalize(dir.path()).unwrap();
        fs::create_dir_all(base.join("usr/bin")).unwrap();
        fs::write(base.join("usr/bin/reborn"), "").unwrap();
        fs::create_dir_all(base.join("proc/77")).unwrap();
        symlink("../../usr/bin/reborn", base.join("proc/77/exe")).unwrap();
        symlink("loop", base.join("loop")).unwrap();
        assert_eq!(
            realpath(&base.join("proc/77/exe")),
            base.join("usr/bin/reborn")
        );
        assert_eq!(realpath(&base.join("missing/../x/./y")), base.join("x/y"));
        assert_eq!(realpath(&base.join("loop")), base.join("loop"));
        assert_eq!(realpath(Path::new("/")), PathBuf::from("/"));
        let root = Root::new(&base);
        fs::create_dir_all(base.join("run/reborn")).unwrap();
        fs::write(base.join("run/reborn/process.pid"), "77\n").unwrap();
        let stat = "77 (reborn) S 1 77 77 0 -1 4194560 100 0 0 0 5 3 0 0 20 0 3 0 4242 1000 50";
        fs::write(base.join("proc/77/stat"), stat).unwrap();
        assert_eq!(
            process_identity(&root, "process", "/usr/bin/reborn"),
            Some(
                Object::new()
                    .with("pid", 77i64)
                    .with("start_ticks", 4242i64)
            )
        );
        assert_eq!(process_identity(&root, "process", "/usr/bin/other"), None);
        assert_eq!(
            process_identity(&root, "supervisor", "/usr/bin/reborn"),
            None
        );
    }
}
