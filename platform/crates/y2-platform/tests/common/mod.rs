//! Shared helpers for tests that run the built `y2-platform` binary on a fixture root (`Y2_ROOT`),
//! with subprocess answers in `Y2_FIXTURE_COMMANDS` and the requests from `Y2_FIXTURE_CALLS`.
// SPDX-License-Identifier: GPL-2.0-only
#![allow(dead_code)]
use serde_json::{json, Value};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU32, Ordering},
};

pub const BINARY: &str = env!("CARGO_BIN_EXE_y2-platform");
static NEXT: AtomicU32 = AtomicU32::new(0);

pub fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// One fixture root and the files the harness keeps beside it, removed on drop.
pub struct Fixture {
    scratch: PathBuf,
    pub root: PathBuf,
    answers: Vec<Value>,
}

pub struct Run {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// Every subprocess asked for: {"argv": [...], "timeout": seconds}.
    pub calls: Vec<Value>,
}

impl Run {
    /// stdout as one JSON document.
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.stdout).unwrap_or_else(|e| panic!("{e}: {}", self.stdout))
    }

    pub fn argvs(&self) -> Vec<Vec<String>> {
        self.calls
            .iter()
            .map(|c| serde_json::from_value(c["argv"].clone()).unwrap())
            .collect()
    }
}

impl Fixture {
    pub fn new() -> Self {
        let scratch = std::env::temp_dir().join(format!(
            "y2-platform-it-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let root = scratch.join("root");
        fs::create_dir_all(&root).unwrap();
        Self {
            scratch,
            root,
            answers: Vec::new(),
        }
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.root.join(name.trim_start_matches('/'))
    }

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

    pub fn put_json(&self, name: &str, value: &Value) -> &Self {
        self.put(name, &value.to_string())
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

    /// A subprocess answer: `ok` with `output`, or failed (`command_failed`).
    pub fn answer(&mut self, argv: &[&str], ok: bool, output: &str) -> &mut Self {
        self.answers.push(json!({"argv": argv, "ok": ok,
            "reason": if ok { Value::Null } else { json!("command_failed") }, "output": output}));
        self
    }

    /// Run `y2-platform args…` (argv[0] `program` when given: the multicall names) with `env`.
    pub fn run_as(&self, program: Option<&str>, args: &[&str], env: &[(&str, &str)]) -> Run {
        let commands = self.scratch.join("commands.json");
        let calls = self.scratch.join("calls.jsonl");
        fs::write(&commands, Value::Array(self.answers.clone()).to_string()).unwrap();
        let _ = fs::remove_file(&calls);
        let mut command = Command::new(BINARY);
        if let Some(program) = program {
            std::os::unix::process::CommandExt::arg0(&mut command, program);
        }
        let output = command
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .env("Y2_ROOT", &self.root)
            .env("Y2_FIXTURE_COMMANDS", &commands)
            .env("Y2_FIXTURE_CALLS", &calls)
            .envs(env.iter().copied())
            .stdin(Stdio::null())
            .output()
            .unwrap();
        Run {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            calls: fs::read_to_string(&calls)
                .unwrap_or_default()
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect(),
        }
    }

    pub fn run(&self, args: &[&str]) -> Run {
        self.run_as(None, args, &[])
    }
}

impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.scratch);
    }
}
