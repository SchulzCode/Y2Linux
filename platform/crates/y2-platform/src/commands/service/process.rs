//! The processes `service-daemon` starts itself with `subprocess.Popen` (service.py, usb.py), not
//! through `common.command`: `y2-platform media reconcile|verify`, `update auto-check`, `report work`,
//! `firmware scan` (stdin/stdout/stderr on /dev/null, a new session, the daemon's environment) and the
//! MTP responder `umtprd` (a fixed environment, RLIMIT_AS 64 MiB, no core file).
//!
//! Below a fixture root nothing starts: the request is recorded in `Y2_FIXTURE_CALLS` as
//! `{"argv": …, "timeout": 0}` and refused like a missing binary (`[Errno 2] No such file or
//! directory: '<argv[0]>'`), as tests/platform_cases/service.py `daemon_ticks` does to Python's Popen.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::Failure;
use std::{
    env, fs,
    io::{self, Write},
    os::unix::process::CommandExt,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use y2p_core::Root;

/// One started child (`Popen`): `poll`, `wait(timeout)`, `terminate`, `kill`.
pub struct Child {
    child: std::process::Child,
    exited: bool,
}

/// How a child is started.
pub enum Start {
    /// service.py: a new session, the daemon's environment.
    Service,
    /// usb.py `USBService.spawn`: a new session, a fixed environment, RLIMIT_AS 64 MiB, RLIMIT_CORE 0.
    Responder,
    /// usb.py `USBService.command` (`subprocess.run`): a fixed environment, the daemon's session.
    Command,
}

fn record(argv: &[&str]) {
    if let Some(calls) = env::var_os("Y2_FIXTURE_CALLS") {
        let line = serde_json::json!({"argv": argv, "timeout": 0}).to_string() + "\n";
        if let Ok(mut file) = fs::OpenOptions::new().append(true).create(true).open(calls) {
            let _ = file.write_all(line.as_bytes());
        }
    }
}

/// `subprocess.Popen(argv, stdin=DEVNULL, stdout=DEVNULL, stderr=DEVNULL, …)`; an OSError when it
/// cannot start.
pub fn spawn(root: &Root, argv: &[&str], how: Start) -> Result<Child, Failure> {
    let missing = |program: &str| {
        Failure::os(
            io::Error::from_raw_os_error(libc::ENOENT),
            Path::new(program),
        )
    };
    let Some((program, args)) = argv.split_first() else {
        return Err(Failure::os_message("empty argv"));
    };
    if !root.is_real() {
        #[cfg(test)]
        if let Some(instead) = testing::instead(argv) {
            let Some(instead) = instead else {
                return Err(missing(program));
            };
            let instead: Vec<&str> = instead.iter().map(String::as_str).collect();
            return spawn(&Root::new("/"), &instead, how);
        }
        record(argv);
        return Err(missing(program));
    }
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if !matches!(how, Start::Service) {
        command
            .env_clear()
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .env("LC_ALL", "C");
    }
    let session = !matches!(how, Start::Command);
    let limits = matches!(how, Start::Responder);
    // SAFETY: setsid() and setrlimit() are async-signal-safe; they run in the forked child before exec
    // (Python's start_new_session=True, then the responder's preexec_fn).
    unsafe {
        command.pre_exec(move || {
            if session && libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            if limits {
                // RLIMIT_AS bounds the lazy handle database as well as fixed USB buffers.
                let memory = libc::rlimit {
                    rlim_cur: 64 * 1024 * 1024,
                    rlim_max: 64 * 1024 * 1024,
                };
                let core = libc::rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                };
                if libc::setrlimit(libc::RLIMIT_AS, &memory) != 0
                    || libc::setrlimit(libc::RLIMIT_CORE, &core) != 0
                {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    match command.spawn() {
        Ok(child) => Ok(Child {
            child,
            exited: false,
        }),
        Err(error) => Err(Failure::os(error, Path::new(program))),
    }
}

impl Child {
    pub fn pid(&self) -> i32 {
        self.child.id() as i32
    }

    /// `poll() is None`: still running (an exited child is reaped).
    pub fn running(&mut self) -> bool {
        if !self.exited {
            // Python's poll() reads an unknown child (ECHILD) as exited too.
            self.exited = !matches!(self.child.try_wait(), Ok(None));
        }
        !self.exited
    }

    /// `wait(timeout)`: whether it ended within `seconds`.
    pub fn wait(&mut self, seconds: f64) -> bool {
        let deadline = Instant::now() + Duration::from_secs_f64(seconds);
        while self.running() {
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(5));
        }
        true
    }

    fn signal(&mut self, signal: libc::c_int) {
        if self.running() {
            // SAFETY: a signal to our own unreaped child.
            unsafe { libc::kill(self.pid(), signal) };
        }
    }

    /// `terminate()`.
    pub fn terminate(&mut self) {
        self.signal(libc::SIGTERM);
    }

    /// `kill()`.
    pub fn kill(&mut self) {
        self.signal(libc::SIGKILL);
    }
}

/// usb.py `USBService.command`: `subprocess.run(argv, timeout=3, env=fixed)`; true for exit 0, false
/// when it cannot start or overruns (killed, as subprocess.run does).
pub fn run(root: &Root, argv: &[&str], seconds: f64) -> bool {
    let Ok(mut child) = spawn(root, argv, Start::Command) else {
        return false;
    };
    let deadline = Instant::now() + Duration::from_secs_f64(seconds);
    loop {
        match child.child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                let _ = child.child.kill();
                let _ = child.child.wait();
                return false;
            }
            Err(_) => return false,
        }
    }
}

/// Unit tests: what a fixture root starts in place of a request (per thread), as Python's tests hand
/// `USBService` a `launch` and patch its `command`. `None` refuses the request as without a script,
/// but records nothing: `Y2_FIXTURE_CALLS` is process-wide (the cross-checks' while they run).
#[cfg(test)]
pub mod testing {
    use std::cell::RefCell;

    type Launch = Box<dyn FnMut(&[&str]) -> Option<Vec<String>>>;

    thread_local! {
        static LAUNCH: RefCell<Option<Launch>> = const { RefCell::new(None) };
    }

    /// Run `f` with `launch` answering this thread's starts below a fixture root: the host argv to
    /// start instead (a stand-in responder, `true` for an acknowledged command).
    pub fn launching<T>(
        launch: impl FnMut(&[&str]) -> Option<Vec<String>> + 'static,
        f: impl FnOnce() -> T,
    ) -> T {
        LAUNCH.with(|l| *l.borrow_mut() = Some(Box::new(launch)));
        let value = f();
        LAUNCH.with(|l| *l.borrow_mut() = None);
        value
    }

    /// `None` without `launching`; else what `launch` answers.
    pub(super) fn instead(argv: &[&str]) -> Option<Option<Vec<String>>> {
        LAUNCH.with(|l| l.borrow_mut().as_mut().map(|launch| launch(argv)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn children_start_poll_and_end() {
        let root = Root::new("/");
        let mut child = spawn(&root, &["/bin/sh", "-c", "sleep 5"], Start::Service).unwrap();
        assert!(child.running());
        assert!(!child.wait(0.05));
        child.terminate();
        assert!(child.wait(2.0));
        assert!(!child.running());
        assert!(run(&root, &["/bin/sh", "-c", "exit 0"], 2.0));
        assert!(!run(&root, &["/bin/sh", "-c", "exit 3"], 2.0));
        let started = Instant::now();
        assert!(!run(&root, &["/bin/sh", "-c", "sleep 5"], 0.1));
        assert!(started.elapsed() < Duration::from_secs(2));
        let error = spawn(&root, &["/nonexistent/y2-platform"], Start::Service)
            .err()
            .unwrap();
        assert_eq!(
            error.message(),
            "[Errno 2] No such file or directory: '/nonexistent/y2-platform'"
        );
    }

    #[test]
    fn fixture_roots_record_the_request_and_refuse_it() {
        let error = spawn(
            &Root::new("/nonexistent-fixture"),
            &["/usr/sbin/umtprd", "-conf", "/run/y2/umtprd.conf"],
            Start::Responder,
        )
        .err()
        .unwrap();
        assert_eq!(
            error.message(),
            "[Errno 2] No such file or directory: '/usr/sbin/umtprd'"
        );
    }
}
