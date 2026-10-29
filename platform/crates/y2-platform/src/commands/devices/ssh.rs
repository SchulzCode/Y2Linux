//! `ssh.py`: `y2-platform ssh-daemon`, USB-only owner SSH, started only after the kernel CRNG is
//! initialized. One loop iteration is one `tick`; the loop publishes /run/y2/ssh.json each time (1 s
//! while not Ready, 5 s while Ready, at once on a netlink address event or a signal), starts one
//! dropbear per daemon lifetime once every prerequisite holds and calls it Ready only when its socket
//! listens on 10.42.0.1:22. SIGTERM/SIGINT end it with exit 0 and no output, after which
//! platform-supervise stops supervising for good; a dropbear that exits is `ssh_listener_exited`
//! (exit 1, the supervisor restarts the daemon). Below a fixture root `Y2_DAEMON_TICKS=n` ends the
//! loop after n iterations, as SIGTERM would.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::{atomic_json, private_directory, Failure, Outcome};
use serde::Serialize;
use serde_json::Value;
use std::{
    io::{self, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::process::CommandExt,
    },
    process::{Child, Command, Stdio},
    sync::atomic::Ordering,
    thread,
    time::{Duration, Instant},
};
use wake::{AddressEvents, Signals, RUNNING};
use y2p_core::{
    command,
    py::{self, eq_int},
    Root,
};

const DROPBEAR: &str = "/usr/sbin/dropbear";
/// Fixed policy, never credentials/configuration-derived shell text.
const DROPBEAR_ARGS: [&str; 15] = [
    "-F",
    "-E",
    "-R",
    "-s",
    "-g",
    "-j",
    "-k",
    "-p",
    "10.42.0.1:22",
    "-l",
    "usb0",
    "-K",
    "10",
    "-I",
    "600",
];

mod wake;

type Address = (bool, Option<&'static str>);

/// `usb_address(ctx)`: usb0 carries exactly 10.42.0.1/24 (`ip -j` JSON); an answer that is not the
/// expected shape is `usb_interface_not_ready`.
fn usb_address(root: &Root) -> Address {
    let result = command::run(
        root,
        &["ip", "-j", "-4", "addr", "show", "dev", "usb0"],
        1.0,
    );
    let parsed: Option<Value> = serde_json::from_str(result.output.as_deref().unwrap_or("")).ok();
    match parsed.as_ref().map(configured) {
        Some(Ok(true)) => (true, None),
        Some(Ok(false)) => (false, Some("usb_address_not_ready")),
        _ => (false, Some("usb_interface_not_ready")),
    }
}

/// Python iteration of a parsed JSON value (`py::iterate`); anything else is a TypeError (`Err`).
fn items(value: &Value) -> Result<Vec<Value>, ()> {
    y2p_core::py::iterate(value).ok_or(())
}

/// `any(a.get('local') == '10.42.0.1' and a.get('prefixlen') == 24 for i in interfaces if
/// i.get('ifname') == 'usb0' for a in i.get('addr_info', []))`, evaluated lazily as the generator:
/// `.get` of a non-dict is an AttributeError, iterating a non-iterable a TypeError (both `Err`).
fn configured(interfaces: &Value) -> Result<bool, ()> {
    for interface in items(interfaces)? {
        let interface = interface.as_object().ok_or(())?;
        if interface.get("ifname").and_then(Value::as_str) != Some("usb0") {
            continue;
        }
        let addresses = match interface.get("addr_info") {
            None => Vec::new(),
            Some(value) => items(value)?,
        };
        for address in addresses {
            let address = address.as_object().ok_or(())?;
            if address.get("local").and_then(Value::as_str) == Some("10.42.0.1")
                && eq_int(address.get("prefixlen"), 24)
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// `prerequisites(ctx, entropy, address)`, in order: key, CRNG, usb0 address, verified /data.
fn prerequisites(root: &Root, entropy: bool, address: Address) -> (&'static str, &'static str) {
    if root
        .read("/root/.ssh/authorized_keys")
        .is_none_or(|k| k.is_empty())
    {
        return ("Unavailable", "no_owner_authorized_key");
    }
    if !entropy {
        return ("Starting", "kernel_crng_not_ready");
    }
    if !address.0 {
        return ("Starting", address.1.unwrap_or("usb_interface_not_ready"));
    }
    if root.read("/data/.y2data-schema").as_deref() != Some("1") {
        return ("Failed", "data_schema_unavailable");
    }
    ("Ready", "usb_address_key_and_crng_ready")
}

/// Child success is not readiness: its bound listening address (10.42.0.1:22 in /proc/net/tcp,
/// state 0A).
fn listening(root: &Root) -> bool {
    let table = root.read("/proc/net/tcp").unwrap_or_default();
    table.lines().skip(1).any(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        fields.len() > 3 && line.contains("01002A0A:0016") && fields[3] == "0A"
    })
}

// ---- the daemon ----------------------------------------------------------------------------------

/// /run/y2/ssh.json, published every tick (sorted, compact: sftp-server greps `"state":"Ready"`);
/// `monotonic_s` tells a reader how fresh it is.
#[derive(Serialize)]
struct State {
    schema: u8,
    boot_id: Option<String>,
    monotonic_s: f64,
    state: &'static str,
    reason: &'static str,
    bind: &'static str,
    interface: &'static str,
    authentication: &'static str,
    /// Whether the SFTP subsystem is installed.
    sftp: bool,
    /// dropbear's process id once it was started.
    pid: Option<u32>,
}

struct Daemon<'a> {
    root: &'a Root,
    runtime: std::path::PathBuf,
    events: AddressEvents,
    address: Option<Address>,
    refresh_at: f64,
    changed: bool,
    child: Option<Child>,
}

impl<'a> Daemon<'a> {
    /// A daemon that has read nothing yet: the first tick reads the address.
    fn new(root: &'a Root, events: AddressEvents) -> Daemon<'a> {
        Daemon {
            root,
            runtime: root.path("/run/y2"),
            events,
            address: None,
            refresh_at: 0.0,
            changed: true,
            child: None,
        }
    }

    /// One loop iteration up to (not including) its wait; the state it published.
    fn tick(&mut self) -> Result<&'static str, Failure> {
        let now = self.root.clock().monotonic();
        if self.changed
            || self.events.sock.is_none()
            || self.address.is_none()
            || now >= self.refresh_at
        {
            let address = usb_address(self.root);
            self.refresh_at = now + if address.0 { 60.0 } else { 1.0 };
            self.address = Some(address);
        }
        let address = self.address.expect("just refreshed");
        let (mut ready, mut reason) = prerequisites(self.root, py::entropy_ready(), address);
        if self.child.is_none() && ready == "Ready" {
            self.child = Some(spawn_dropbear()?);
            (ready, reason) = ("Starting", "listener_starting");
        } else if let Some(child) = &mut self.child {
            if matches!(child.try_wait(), Ok(Some(_))) {
                // A RuntimeError in Python: main.py's record names its class.
                return Err(Failure::Os {
                    text: "ssh_listener_exited".into(),
                    class: "RuntimeError",
                });
            }
            (ready, reason) = if listening(self.root) && ready == "Ready" {
                ("Ready", "usb_bound_key_only_listener")
            } else {
                ("Starting", reason)
            };
        }
        let record = State {
            schema: 1,
            boot_id: self.root.boot_id(),
            monotonic_s: self.root.clock().monotonic(),
            state: ready,
            reason,
            bind: "10.42.0.1:22",
            interface: "usb0",
            authentication: "owner_public_key",
            sftp: self.root.path("/usr/libexec/y2-sftp-server").is_file(),
            pid: self.child.as_ref().map(Child::id),
        };
        atomic_json(&self.runtime.join("ssh.json"), &record, false)?;
        Ok(ready)
    }

    /// On the way out, as Python's `finally`: a running dropbear gets SIGTERM, 2 s, then SIGKILL.
    fn stop_child(&mut self) {
        let Some(child) = &mut self.child else {
            return;
        };
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        let group = child.id() as libc::pid_t;
        // SAFETY: signals to the session dropbear leads (start_new_session).
        unsafe { libc::killpg(group, libc::SIGTERM) };
        let until = Instant::now() + Duration::from_secs(2);
        while Instant::now() < until {
            if !matches!(child.try_wait(), Ok(None)) {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
        // SAFETY: as above.
        unsafe { libc::killpg(group, libc::SIGKILL) };
    }
}

/// `subprocess.Popen([dropbear, …], stdin/stdout/stderr=DEVNULL, start_new_session=True)`: started
/// directly (not the command runner), inheriting the whole environment, never below a fixture guard.
/// As Popen, the child gets no other descriptor (close_fds) and default SIGPIPE/SIGXFSZ.
fn spawn_dropbear() -> Result<Child, Failure> {
    let mut command = Command::new(DROPBEAR);
    command
        .args(DROPBEAR_ARGS)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: only async-signal-safe calls between fork and exec. CLOSE_RANGE_CLOEXEC (4) marks the
    // inherited descriptors close-on-exec without closing std's own exec-error pipe.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 4u32);
            libc::signal(libc::SIGXFSZ, libc::SIG_DFL);
            Ok(())
        });
    }
    command.spawn().map_err(|error| {
        let (text, class) = py::os_error(&error, Some(DROPBEAR));
        Failure::Os { text, class }
    })
}

/// `y2-platform ssh-daemon`.
pub fn serve(root: &Root) -> Outcome {
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    let path = runtime.join("ssh.lock");
    let lock = y2p_core::lock::try_exclusive(&path)
        .map_err(|e| Failure::os(e, &path))?
        .ok_or_else(|| Failure::Os {
            text: "[Errno 11] Resource temporarily unavailable".into(),
            class: "BlockingIOError",
        })?;
    RUNNING.store(true, Ordering::SeqCst);
    let mut pipe = [-1; 2];
    // SAFETY: a two-element array for the new descriptors.
    if unsafe { libc::pipe2(pipe.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) } != 0 {
        let (text, class) = py::os_error(&io::Error::last_os_error(), None);
        return Err(Failure::Os { text, class });
    }
    // SAFETY: both descriptors were just created and are owned here.
    let (wake_read, wake_write) =
        unsafe { (OwnedFd::from_raw_fd(pipe[0]), OwnedFd::from_raw_fd(pipe[1])) };
    let signals = Signals::install(wake_write.as_raw_fd());
    // Below a fixture root a test may end the loop after n iterations (tests/platform_differential.py).
    let ticks = (!root.is_real())
        .then(|| std::env::var("Y2_DAEMON_TICKS").ok())
        .flatten()
        .and_then(|n| n.trim().parse::<u64>().ok());
    let mut daemon = Daemon::new(root, AddressEvents::new());
    let mut result = Ok(());
    let mut done = 0;
    while RUNNING.load(Ordering::SeqCst) {
        let ready = match daemon.tick() {
            Ok(ready) => ready,
            Err(error) => {
                result = Err(error);
                break;
            }
        };
        done += 1;
        if ticks.is_some_and(|n| done >= n) {
            break;
        }
        // Keep the existing <10-second readiness freshness contract. SIGTERM/child death and kernel
        // address changes wake immediately.
        daemon.changed = daemon.events.wait(
            if ready == "Ready" { 5.0 } else { 1.0 },
            wake_read.as_raw_fd(),
        );
    }
    daemon.events.sock = None;
    signals.restore();
    drop((wake_read, wake_write));
    daemon.stop_child();
    drop(lock);
    result?;
    // cli.py returns 0 without printing anything (a Reply would print a line).
    let _ = io::stdout().flush();
    std::process::exit(0)
}

#[cfg(test)]
#[path = "ssh_tests.rs"]
mod ssh_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn addresses_are_read_as_the_generator() {
        let ok =
            json!([{"ifname": "usb0", "addr_info": [{"local": "10.42.0.1", "prefixlen": 24}]}]);
        assert_eq!(configured(&ok), Ok(true));
        let wrong =
            json!([{"ifname": "usb0", "addr_info": [{"local": "10.42.0.2", "prefixlen": 24}]}]);
        assert_eq!(configured(&wrong), Ok(false));
        assert_eq!(
            configured(&json!([{"ifname": "wlan0", "addr_info": 3}])),
            Ok(false)
        );
        assert_eq!(
            configured(&json!([{"ifname": "usb0", "addr_info": 3}])),
            Err(())
        );
        assert_eq!(configured(&json!({"a": 1})), Err(()));
        assert_eq!(configured(&json!({})), Ok(false));
        assert_eq!(configured(&json!(null)), Err(()));
        // The first match ends the search before a later malformed entry.
        let early = json!([{"ifname": "usb0", "addr_info": [{"local": "10.42.0.1", "prefixlen": 24.0}]}, 5]);
        assert_eq!(configured(&early), Ok(true));
    }
}
