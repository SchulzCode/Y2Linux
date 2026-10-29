//! Shared helpers for the unit tests: a fixture root in a self-cleaning directory whose root carries
//! the test's subprocess answers and clock (`Fixture::with_answers`, `responding`, `with_clock`).
// SPDX-License-Identifier: GPL-2.0-only
#![allow(dead_code, unused_imports)] // each test module uses part of it
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
};
pub use y2p_core::command::testing::{failed, ok};
pub use y2p_core::command::{Outcome, Scripted};
use y2p_core::Root;

static NEXT: AtomicU32 = AtomicU32::new(0);

/// Process-wide signal state (dispositions, a signal sent to the whole test process, a flag a handler
/// sets) is shared by every test thread: tests that install a handler or signal the process take this,
/// one at a time (update `stage`'s SIGTERM cancel, the service daemon's wake signals).
pub fn process_signals() -> std::sync::MutexGuard<'static, ()> {
    static SIGNALS: std::sync::Mutex<()> = std::sync::Mutex::new(());
    SIGNALS.lock().unwrap_or_else(|e| e.into_inner())
}

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
            root: Root::fixture(&base),
            base,
        }
    }

    /// This fixture with its root's commands answered from `answers` (argv → outcome, anything else
    /// `fixture`) and recorded (`calls()`), in every thread the root goes to.
    pub fn with_answers(mut self, answers: Vec<(Vec<String>, Outcome)>) -> Self {
        self.root = self.root.clone().with_answers(answers);
        self
    }

    /// This fixture with its root's commands answered by `respond(argv, timeout)` and recorded.
    pub fn responding(
        mut self,
        respond: impl FnMut(&[&str], f64) -> Outcome + Send + 'static,
    ) -> Self {
        self.root = self
            .root
            .clone()
            .with_runner(std::sync::Arc::new(Scripted::responding(respond)));
        self
    }

    /// This fixture with its root reading `clock` (a `y2p_core::clock::Manual` a test advances).
    pub fn with_clock(mut self, clock: std::sync::Arc<dyn y2p_core::clock::Clock>) -> Self {
        self.root = self.root.clone().with_clock(clock);
        self
    }

    /// The commands the root's runner recorded (argv, timeout).
    pub fn calls(&self) -> Vec<(Vec<String>, f64)> {
        self.root.calls()
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
    fn a_fixture_root_carries_its_answers_and_clock() {
        let clock = std::sync::Arc::new(y2p_core::clock::Manual::at(1_000_000_000, 1_000_000_000));
        let fixture = Fixture::new()
            .with_answers(vec![ok(&["blkid"], "id")])
            .with_clock(clock.clone());
        let root = fixture.root.clone();
        let outcome = std::thread::spawn(move || y2p_core::command::run(&root, &["blkid"], 1.0))
            .join()
            .unwrap();
        assert_eq!(outcome.output.as_deref(), Some("id"));
        assert_eq!(fixture.calls(), vec![(vec!["blkid".to_string()], 1.0)]);
        clock.advance(500_000_000);
        assert_eq!(fixture.root.clock().monotonic(), 1.5);
        let counting = Fixture::new().responding(|argv, _| Outcome {
            ok: true,
            reason: None,
            output: Some(argv.join(" ")),
        });
        let echoed = y2p_core::command::run(&counting.root, &["a", "b"], 1.0);
        assert_eq!(echoed.output.as_deref(), Some("a b"));
    }
}
