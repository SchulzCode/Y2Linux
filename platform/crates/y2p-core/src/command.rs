//! `common.command`: one subprocess without a shell, with bounded output and a deadline, its stdin and
//! stderr on /dev/null, in its own session, with only `PATH` and `LC_ALL` in its environment. On an
//! overrun the whole tree is SIGKILLed (`kill_tree`, V1 code review B5: a filesystem check outlived the
//! `media reconcile` that started it); a child in D state cannot be given a userspace deadline.
//!
//! Below a fixture root no process starts (Python's fixture `Context` answers `fixture`); a host test
//! may give the answers in `Y2_FIXTURE_COMMANDS` and receives every request in `Y2_FIXTURE_CALLS`
//! (tests/platform_differential.py).
// SPDX-License-Identifier: GPL-2.0-only
use crate::Root;
use serde::Deserialize;
use std::{
    env, fs,
    io::{Read, Write},
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ok: bool,
    /// `None` when ok; `command_unavailable`, `command_timeout`, `command_output_limit`,
    /// `command_failed` (or a fixture's own reason).
    pub reason: Option<String>,
    pub output: Option<String>,
}

/// `common.command` defaults: 1 s, 64 KiB, no output on failure.
pub const TIMEOUT: f64 = 1.0;
pub const LIMIT: usize = 65_536;

pub fn run(root: &Root, argv: &[&str], timeout: f64) -> Outcome {
    run_with(root, argv, timeout, LIMIT, false)
}

/// `common.command` through the root's runner: real processes on the player, fixture answers below a
/// fixture root, or a test's own runner.
pub fn run_with(
    root: &Root,
    argv: &[&str],
    timeout: f64,
    limit: usize,
    failure_output: bool,
) -> Outcome {
    root.runner().run(argv, timeout, limit, failure_output)
}

/// Who answers a root's subprocess requests.
pub trait Runner: Send + Sync {
    /// One request: `argv` with its deadline, output bounded by `limit`, the output kept on a
    /// failure only with `failure_output`.
    fn run(&self, argv: &[&str], timeout: f64, limit: usize, failure_output: bool) -> Outcome;

    /// The requests made so far (argv, timeout), for a runner that records them.
    fn calls(&self) -> Vec<(Vec<String>, f64)> {
        Vec::new()
    }
}

/// The player's processes (`Root::new("/")`).
pub struct Processes;

impl Runner for Processes {
    fn run(&self, argv: &[&str], timeout: f64, limit: usize, failure_output: bool) -> Outcome {
        real(argv, timeout, limit, failure_output)
    }
}

/// Below a fixture root no process starts: the answers of `Y2_FIXTURE_COMMANDS`, every request
/// appended to `Y2_FIXTURE_CALLS`.
pub struct FixtureFiles;

impl Runner for FixtureFiles {
    fn run(&self, argv: &[&str], timeout: f64, _limit: usize, _failure_output: bool) -> Outcome {
        fixture(argv, timeout)
    }
}

type Respond = Box<dyn FnMut(&[&str], f64) -> Outcome + Send>;

/// A test's runner: answers from a list or a function, every request recorded
/// (`Root::fixture(dir).with_answers(…)`, then `root.calls()`).
pub struct Scripted {
    respond: std::sync::Mutex<Respond>,
    calls: std::sync::Mutex<Vec<(Vec<String>, f64)>>,
}

impl Scripted {
    /// The answer whose argv equals the request; anything else is answered `fixture`.
    pub fn answers(answers: Vec<(Vec<String>, Outcome)>) -> Self {
        Self::responding(move |argv, _| {
            answers
                .iter()
                .find(|(a, _)| a.iter().map(String::as_str).eq(argv.iter().copied()))
                .map_or_else(unanswered, |(_, o)| o.clone())
        })
    }

    /// Every answer from `respond(argv, timeout)`: a fake whose state changes with the commands (a
    /// `mount` that adds its mountinfo line).
    pub fn responding(respond: impl FnMut(&[&str], f64) -> Outcome + Send + 'static) -> Self {
        Self {
            respond: std::sync::Mutex::new(Box::new(respond)),
            calls: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl Runner for Scripted {
    fn run(&self, argv: &[&str], timeout: f64, _limit: usize, _failure_output: bool) -> Outcome {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((argv.iter().map(|a| a.to_string()).collect(), timeout));
        (self
            .respond
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner))(argv, timeout)
    }

    fn calls(&self) -> Vec<(Vec<String>, f64)> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// What a fixture gives a request it has no answer for.
fn unanswered() -> Outcome {
    Outcome {
        ok: false,
        reason: Some("fixture".into()),
        output: None,
    }
}

#[derive(Deserialize)]
struct Answer {
    argv: Vec<String>,
    ok: bool,
    reason: Option<String>,
    output: Option<String>,
}

/// The child starts a session of its own (Python's `start_new_session=True`): a timeout that kills
/// its process group never reaches this process.
pub fn new_session(command: &mut Command) {
    // SAFETY: setsid() is async-signal-safe; it runs in the forked child before exec.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
}

fn fixture(argv: &[&str], timeout: f64) -> Outcome {
    if let Some(calls) = env::var_os("Y2_FIXTURE_CALLS") {
        let line = serde_json::json!({"argv": argv, "timeout": timeout}).to_string() + "\n";
        if let Ok(mut file) = fs::OpenOptions::new().append(true).create(true).open(calls) {
            let _ = file.write_all(line.as_bytes());
        }
    }
    let answers: Vec<Answer> = env::var_os("Y2_FIXTURE_COMMANDS")
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    answers
        .into_iter()
        .find(|a| a.argv.iter().map(String::as_str).eq(argv.iter().copied()))
        .map(|a| Outcome {
            ok: a.ok,
            reason: a.reason,
            output: a.output,
        })
        .unwrap_or_else(unanswered)
}

/// Answers for a test runner (`Root::fixture(dir).with_answers(…)`): as a fixture file gives them.
#[cfg(feature = "testing")]
pub mod testing {
    use super::Outcome;

    /// An answer as a fixture file gives it: ok with `output`, or failed with `reason`.
    pub fn ok(argv: &[&str], output: &str) -> (Vec<String>, Outcome) {
        let outcome = Outcome {
            ok: true,
            reason: None,
            output: Some(output.into()),
        };
        (argv.iter().map(|a| a.to_string()).collect(), outcome)
    }

    pub fn failed(argv: &[&str], reason: &str) -> (Vec<String>, Outcome) {
        let outcome = Outcome {
            ok: false,
            reason: Some(reason.into()),
            output: None,
        };
        (argv.iter().map(|a| a.to_string()).collect(), outcome)
    }
}

fn real(argv: &[&str], timeout: f64, limit: usize, failure_output: bool) -> Outcome {
    let unavailable = Outcome {
        ok: false,
        reason: Some("command_unavailable".into()),
        output: None,
    };
    let Some((program, args)) = argv.split_first() else {
        return unavailable;
    };
    let mut command = Command::new(program);
    command
        .args(args)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    new_session(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return unavailable,
    };
    let pid = child.id() as libc::pid_t;
    let deadline = Instant::now() + Duration::from_secs_f64(timeout.max(0.0));
    let mut stdout = child.stdout.take().expect("piped stdout");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if sender.send(chunk[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let mut output = Vec::new();
    let mut failure = None;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            failure = Some("command_timeout");
            break;
        }
        match receiver.recv_timeout(remaining) {
            Ok(chunk) => {
                output.extend_from_slice(&chunk);
                if output.len() > limit {
                    failure = Some("command_output_limit");
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                failure = Some("command_timeout");
                break;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let mut status = None;
    if failure.is_none() {
        loop {
            match child.try_wait() {
                Ok(Some(exit)) => {
                    status = Some(exit);
                    break;
                }
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
                _ => {
                    failure = Some("command_timeout");
                    break;
                }
            }
        }
    }
    if failure.is_some() {
        kill_tree(pid);
        // As Python's 50 ms wait: a child stuck in D state is left behind.
        let until = Instant::now() + Duration::from_millis(50);
        while Instant::now() < until && !matches!(child.try_wait(), Ok(Some(_))) {
            thread::sleep(Duration::from_millis(1));
        }
    }
    let ok = failure.is_none() && status.is_some_and(|s| s.success());
    let text = String::from_utf8_lossy(&output);
    Outcome {
        ok,
        reason: failure.map(str::to_owned).or(if ok {
            None
        } else {
            Some("command_failed".into())
        }),
        output: (ok || failure_output).then(|| crate::root::python_strip(&text).to_owned()),
    }
}

/// Every process below `pid`, also those that started their own session (`common.descendants`).
pub fn descendants(pid: libc::pid_t) -> Vec<libc::pid_t> {
    let mut children: std::collections::HashMap<libc::pid_t, Vec<libc::pid_t>> = Default::default();
    for entry in fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Some(child) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<libc::pid_t>().ok())
        else {
            continue;
        };
        let Ok(stat) = fs::read(entry.path().join("stat")) else {
            continue;
        };
        let Some(close) = stat.iter().rposition(|&b| b == b')') else {
            continue;
        };
        let fields: Vec<&[u8]> = stat[close + 1..]
            .split(|b| b.is_ascii_whitespace())
            .filter(|f| !f.is_empty())
            .collect();
        if let Some(parent) = fields
            .get(1)
            .and_then(|f| std::str::from_utf8(f).ok()?.parse().ok())
        {
            children.entry(parent).or_default().push(child);
        }
    }
    let (mut found, mut stack) = (Vec::new(), vec![pid]);
    while let Some(next) = stack.pop() {
        for &child in children.get(&next).into_iter().flatten() {
            found.push(child);
            stack.push(child);
        }
    }
    found
}

/// `common.kill_tree`: SIGKILL the child's group, then each descendant and its group.
pub fn kill_tree(pid: libc::pid_t) {
    let below = descendants(pid);
    // SAFETY: plain signals to processes this command started (or that already ended: ESRCH).
    unsafe {
        libc::killpg(pid, libc::SIGKILL);
        let group = libc::getpgid(pid);
        if group > 0 {
            libc::killpg(group, libc::SIGKILL);
        }
        for child in below {
            libc::kill(child, libc::SIGKILL);
            let group = libc::getpgid(child);
            if group > 0 {
                libc::killpg(group, libc::SIGKILL);
            }
        }
    }
}

#[cfg(test)]
#[path = "command_timeout_tests.rs"]
mod command_timeout_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_commands_are_bounded() {
        let root = Root::new("/");
        let echo = real(
            &["/bin/sh", "-c", "echo ' hi '; echo err >&2"],
            2.0,
            LIMIT,
            false,
        );
        assert_eq!(
            echo,
            Outcome {
                ok: true,
                reason: None,
                output: Some("hi".into())
            }
        );
        let failed = run(&root, &["/bin/sh", "-c", "echo out; exit 3"], 2.0);
        assert_eq!(
            failed,
            Outcome {
                ok: false,
                reason: Some("command_failed".into()),
                output: None
            }
        );
        let kept = run_with(
            &root,
            &["/bin/sh", "-c", "echo out; exit 3"],
            2.0,
            LIMIT,
            true,
        );
        assert_eq!(kept.output.as_deref(), Some("out"));
        let started = Instant::now();
        let slow = run(&root, &["/bin/sh", "-c", "sleep 5 & sleep 5"], 0.2);
        assert_eq!(slow.reason.as_deref(), Some("command_timeout"));
        assert!(started.elapsed() < Duration::from_secs(2));
        let long = real(
            &["/bin/sh", "-c", "head -c 100000 /dev/zero"],
            2.0,
            1000,
            false,
        );
        assert_eq!(long.reason.as_deref(), Some("command_output_limit"));
        assert_eq!(
            run(&root, &["/nonexistent/tool"], 1.0).reason.as_deref(),
            Some("command_unavailable")
        );
        let env = real(
            &["/bin/sh", "-c", "echo \"$PATH $LC_ALL ${HOME-none}\""],
            2.0,
            LIMIT,
            false,
        );
        assert_eq!(
            env.output.as_deref(),
            Some("/usr/sbin:/usr/bin:/sbin:/bin C none")
        );
    }

    #[test]
    fn fixture_roots_never_start_a_process() {
        let outcome = fixture(&["/bin/true"], 1.0);
        assert_eq!(
            outcome,
            Outcome {
                ok: false,
                reason: Some("fixture".into()),
                output: None
            }
        );
    }

    #[test]
    fn a_root_carries_its_own_runner_into_other_threads() {
        let root = Root::fixture("/nonexistent-fixture").with_answers(vec![(
            vec!["blkid".to_string()],
            Outcome {
                ok: true,
                reason: None,
                output: Some("uuid".into()),
            },
        )]);
        let shared = root.clone();
        let answer = std::thread::spawn(move || run(&shared, &["blkid"], 2.0))
            .join()
            .unwrap();
        assert_eq!(answer.output.as_deref(), Some("uuid"));
        assert_eq!(run(&root, &["true"], 1.0), unanswered());
        assert_eq!(
            root.calls(),
            [
                (vec!["blkid".to_string()], 2.0),
                (vec!["true".to_string()], 1.0)
            ]
        );
        let counted = Root::fixture("/nonexistent-fixture").with_runner(std::sync::Arc::new(
            Scripted::responding(|argv, _| Outcome {
                ok: true,
                reason: None,
                output: Some(argv.len().to_string()),
            }),
        ));
        assert_eq!(run(&counted, &["a", "b"], 1.0).output.as_deref(), Some("2"));
        assert!(Root::new("/nonexistent-fixture").calls().is_empty());
    }
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod command_tests;
