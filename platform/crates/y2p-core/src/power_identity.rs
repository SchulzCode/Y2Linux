//! `power.process_identity`: Reborn's (or its supervisor's) pid from `/run/reborn/<name>.pid`, accepted
//! only while `/proc/<pid>/exe` resolves to the expected executable, with its start time in clock ticks
//! (stat field 22) so a reused pid is told apart. The shutdown coordinator records it with an intent
//! and accepts an acknowledgement only from the same process; `status --reborn` reads it too.
// SPDX-License-Identifier: GPL-2.0-only
pub use crate::paths::realpath;
use crate::{power_text::python_split, root::parse_int, Root};
use serde::{Deserialize, Serialize};

/// A live process told apart from a later one with its pid: its start time in clock ticks.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: i64,
    pub start_ticks: i64,
}

/// The identity of the live process running `expected` whose pid `/run/reborn/<name>.pid` names, or
/// `None`.
pub fn process_identity(root: &Root, name: &str, expected: &str) -> Option<ProcessIdentity> {
    let pid = root.integer(&format!("/run/reborn/{name}.pid"))?;
    if pid <= 1 {
        return None;
    }
    if realpath(&root.path(&format!("/proc/{pid}/exe"))) != realpath(&root.path(expected)) {
        return None;
    }
    let value = root.read(&format!("/proc/{pid}/stat"))?;
    let (_, tail) = value.rsplit_once(") ")?;
    let ticks = parse_int(python_split(tail).get(19)?)?;
    Some(ProcessIdentity {
        pid,
        start_ticks: ticks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::root::tests::tempdir::Dir;
    use std::path::{Path, PathBuf};
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
            Some(ProcessIdentity {
                pid: 77,
                start_ticks: 4242
            })
        );
        assert_eq!(process_identity(&root, "process", "/usr/bin/other"), None);
        assert_eq!(
            process_identity(&root, "supervisor", "/usr/bin/reborn"),
            None
        );
    }
}
