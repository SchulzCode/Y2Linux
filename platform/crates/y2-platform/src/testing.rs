//! Shared helpers for the unit tests: a fixture root in a self-cleaning directory and scripted
//! subprocess answers (`y2p_core::command::testing`, per thread).
// SPDX-License-Identifier: GPL-2.0-only
#![allow(dead_code, unused_imports)] // each test module uses part of it
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
};
pub use y2p_core::command::testing::{failed, ok, scripted};
use y2p_core::Root;

static NEXT: AtomicU32 = AtomicU32::new(0);

/// The repository (for the overlay's files, e.g. the stock battery profile).
pub fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// A fixture root: device paths below a temporary directory, removed on drop.
pub struct Fixture {
    base: PathBuf,
    pub root: Root,
}

impl Fixture {
    pub fn new() -> Self {
        let base = std::env::temp_dir().join(format!(
            "y2-platform-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(&base).unwrap();
        Self {
            root: Root::new(&base),
            base,
        }
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    /// The host path of a device path.
    pub fn path(&self, name: &str) -> PathBuf {
        self.root.path(name)
    }

    /// Write `text` at a device path (parents made), mode 0644.
    pub fn put(&self, name: &str, text: &str) -> &Self {
        self.put_mode(name, text, 0o644)
    }

    pub fn put_mode(&self, name: &str, text: &str, mode: u32) -> &Self {
        let path = self.path(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        self
    }

    /// Write a JSON value (compact, as `atomic_json` does not matter here: readers parse it).
    pub fn put_json(&self, name: &str, value: &Value) -> &Self {
        self.put(name, &value.to_string())
    }

    /// Copy a file of the repository (relative path) to a device path.
    pub fn copy_from_repository(&self, relative: &str, name: &str) -> &Self {
        self.put(
            name,
            &fs::read_to_string(repository().join(relative)).unwrap(),
        )
    }

    pub fn mkdir(&self, name: &str) -> &Self {
        fs::create_dir_all(self.path(name)).unwrap();
        self
    }

    pub fn remove(&self, name: &str) -> &Self {
        let _ = fs::remove_file(self.path(name));
        self
    }

    pub fn exists(&self, name: &str) -> bool {
        self.path(name).symlink_metadata().is_ok()
    }

    pub fn read(&self, name: &str) -> String {
        fs::read_to_string(self.path(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    pub fn json(&self, name: &str) -> Value {
        serde_json::from_str(&self.read(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    pub fn mode(&self, name: &str) -> u32 {
        fs::metadata(self.path(name)).unwrap().permissions().mode() & 0o7777
    }
}

impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripted_commands_answer_this_thread_only() {
        let fixture = Fixture::new();
        fixture.put("/etc/x", "1\n");
        assert_eq!(fixture.root.read("/etc/x").as_deref(), Some("1"));
        let (outcome, calls) = scripted(vec![ok(&["true"], "yes")], || {
            y2p_core::command::run(&fixture.root, &["true"], 2.0)
        });
        assert!(outcome.ok);
        assert_eq!(outcome.output.as_deref(), Some("yes"));
        assert_eq!(calls, vec![(vec!["true".to_string()], 2.0)]);
        let unknown = y2p_core::command::run(&fixture.root, &["true"], 1.0);
        assert_eq!(unknown.reason.as_deref(), Some("fixture"));
    }
}
