//! `y2-platform connectivity-daemon` (`connectivity.serve`): monitor the boot-started radio helpers
//! without shell polling or reconnect ownership. `usr/libexec/y2/connectivity` starts calibration,
//! wpa_supplicant, BlueALSA and y2-bt-reconnect and then `exec`s this daemon (so
//! `/run/y2/connectivity.pid` names it); it restarts a dead helper, attaches wpa_supplicant to wlan0
//! with the wpa-event action hook, re-adds a stale interface (F53) and stops every helper when it ends.
//! While healthy it starts no process: helper death is a pidfd event, link changes a netlink event.
//!
//! Below a fixture root the loop does not wait, sees no netlink events (a socket that never becomes
//! readable) and ends after `Y2_DAEMON_TICKS` iterations, as tests/platform_cases/connectivity.py stops
//! the Python daemon.
// SPDX-License-Identifier: GPL-2.0-only
use super::wifi::{journal, Json};
use crate::commands::{codec, network::wlan_switched_off};
use crate::reply::{private_directory, unlink_missing_ok, Failure, Outcome};
use serde_json::{json, Value};
use std::{
    env,
    ffi::OsString,
    fs, io,
    os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicI32, Ordering},
};
use y2p_core::{
    clock, command,
    py::{number, truthy},
    Root,
};

const HELPERS: [(&str, &str); 4] = [
    ("calibration", "/usr/sbin/y2-calibration"),
    ("wpa", "/usr/sbin/wpa_supplicant"),
    ("bluealsa", "/usr/bin/bluealsad"),
    ("reconnect", "/usr/sbin/y2-bt-reconnect"),
];
const HEALTH_S: f64 = 30.0;
/// Without this the supplicant rescans every ~5 s forever while the saved AP is absent (measured: 11
/// active scans/min, ~7 Wi-Fi IRQ/s). Exponential backoff to two minutes bounds that; association with
/// a present AP is unaffected.
const AUTOSCAN: &str = "exponential:2:120";
/// F53 (aa96e00a, 8.38): after a wake wlan0 stayed INTERFACE_DISABLED for 106 min with every rfkill
/// unblocked; Joins failed until Wi-Fi off/on, which re-adds the interface. Re-add it after this long,
/// backed off.
const DISABLED_S: f64 = 30.0;
const RENEW_MAX_S: f64 = 600.0;
const GLOBAL: &str = "/run/wpa_supplicant/global";

/// `os.path.realpath(path)` (not strict): symlinks resolved as far as the path exists, `..` after
/// resolving, the rest appended as written.
fn realpath(path: &Path) -> PathBuf {
    let mut pending: Vec<OsString> = path
        .components()
        .rev()
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n.to_owned()),
            Component::ParentDir => Some("..".into()),
            _ => None,
        })
        .collect();
    let mut resolved = PathBuf::from("/");
    let mut hops = 0;
    while let Some(part) = pending.pop() {
        if part == ".." {
            resolved.pop();
            continue;
        }
        let next = resolved.join(&part);
        match fs::read_link(&next) {
            Ok(target) if hops < 40 => {
                hops += 1;
                if target.is_absolute() {
                    resolved = PathBuf::from("/");
                }
                pending.extend(target.components().rev().filter_map(|c| match c {
                    Component::Normal(n) => Some(n.to_owned()),
                    Component::ParentDir => Some("..".into()),
                    _ => None,
                }));
            }
            _ => resolved = next,
        }
    }
    resolved
}

/// `alive(ctx, name, binary)`: the helper's pid when its pid file names a live process running that
/// binary. The liveness probe is a real `kill(pid, 0)`, also below a fixture root (as Python's).
fn alive(root: &Root, name: &str, binary: &str) -> Option<i32> {
    let pid = root.integer(&format!("/run/y2/{name}.pid"))?;
    if pid <= 1 {
        return None;
    }
    let exe = fs::read_link(root.path(&format!("/proc/{pid}/exe"))).ok()?;
    if exe != realpath(&root.path(binary)) {
        return None;
    }
    let pid = i32::try_from(pid).ok()?;
    // SAFETY: signal 0 only checks that the process exists.
    (unsafe { libc::kill(pid, 0) } == 0).then_some(pid)
}

fn os(error: io::Error) -> Failure {
    let (text, class) = y2p_core::py::os_error(&error, None);
    Failure::Os { text, class }
}

/// Readable descriptors among `fds` within `timeout` seconds (`select.select(fds, [], [], timeout)`;
/// a hang-up or error reads as readable, as select reports it; EINTR waits again for the rest).
pub fn select(fds: &[RawFd], timeout: f64) -> io::Result<Vec<RawFd>> {
    let mut polled: Vec<libc::pollfd> = fds
        .iter()
        .map(|&fd| libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();
    let deadline = clock::monotonic() + timeout.max(0.0);
    loop {
        let left = (deadline - clock::monotonic()).max(0.0);
        let spec = libc::timespec {
            tv_sec: left.trunc() as libc::time_t,
            tv_nsec: (left.fract() * 1e9) as libc::c_long,
        };
        // SAFETY: `polled` is a valid array of `len` pollfd structures for the call.
        let n = unsafe {
            libc::ppoll(
                polled.as_mut_ptr(),
                polled.len() as libc::nfds_t,
                &spec,
                std::ptr::null(),
            )
        };
        if n >= 0 {
            break;
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINTR) {
            return Err(error);
        }
    }
    Ok(polled
        .iter()
        .filter(|p| p.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0)
        .map(|p| p.fd)
        .collect())
}

// ---- signals ------------------------------------------------------------------------------------

static RUNNING: AtomicBool = AtomicBool::new(true);
static WAKE_FD: AtomicI32 = AtomicI32::new(-1);

extern "C" fn on_signal(signal: libc::c_int) {
    if signal != libc::SIGUSR1 {
        RUNNING.store(false, Ordering::SeqCst);
    }
    let fd = WAKE_FD.load(Ordering::SeqCst);
    if fd >= 0 {
        let byte = signal as u8;
        // SAFETY: write(2) is async-signal-safe; a full pipe drops the byte (warn_on_full_buffer=False).
        unsafe { libc::write(fd, (&byte as *const u8).cast(), 1) };
    }
}

/// `service.SignalWake`: SIGTERM and SIGINT end the loop, SIGUSR1 only wakes it; each writes to a
/// non-blocking pipe the loop selects on. The previous handlers come back on drop.
struct SignalWake {
    read: OwnedFd,
    write: OwnedFd,
    previous: Vec<(libc::c_int, libc::sigaction)>,
}

impl SignalWake {
    fn new() -> Result<Self, Failure> {
        let mut fds = [0; 2];
        // SAFETY: pipe2 fills two descriptors this struct then owns.
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) } != 0 {
            return Err(os(io::Error::last_os_error()));
        }
        // SAFETY: both descriptors were just created and are owned once.
        let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
        WAKE_FD.store(write.as_raw_fd(), Ordering::SeqCst);
        let mut previous = Vec::new();
        for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGUSR1] {
            // SAFETY: a plain handler that only touches atomics and write(2); the old action is kept.
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = on_signal as *const () as usize;
                action.sa_flags = libc::SA_RESTART;
                libc::sigemptyset(&mut action.sa_mask);
                let mut old: libc::sigaction = std::mem::zeroed();
                libc::sigaction(signal, &action, &mut old);
                previous.push((signal, old));
            }
        }
        Ok(Self {
            read,
            write,
            previous,
        })
    }

    fn drain(&self) {
        let mut buffer = [0u8; 4096];
        // SAFETY: reading into a local buffer from our own non-blocking pipe.
        unsafe {
            libc::read(
                self.read.as_raw_fd(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
            )
        };
    }
}

impl Drop for SignalWake {
    fn drop(&mut self) {
        for (signal, old) in &self.previous {
            // SAFETY: restoring the action saved in new().
            unsafe { libc::sigaction(*signal, old, std::ptr::null_mut()) };
        }
        WAKE_FD.store(-1, Ordering::SeqCst);
        let _ = &self.write;
    }
}

// ---- netlink ------------------------------------------------------------------------------------

/// `ssh.AddressEvents`: passive link/IPv4-address events (RTMGRP_LINK | RTMGRP_IPV4_IFADDR), never
/// parsed. `Fixture` is a socket that never becomes readable.
enum Events {
    Real(OwnedFd),
    Fixture,
    Closed,
}

impl Events {
    fn open(root: &Root) -> Self {
        if !root.is_real() {
            return Events::Fixture;
        }
        // SAFETY: socket/bind with a zeroed sockaddr_nl; the descriptor is owned on success.
        unsafe {
            let fd = libc::socket(
                libc::AF_NETLINK,
                libc::SOCK_RAW | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                libc::NETLINK_ROUTE,
            );
            if fd < 0 {
                return Events::Closed;
            }
            let fd = OwnedFd::from_raw_fd(fd);
            let mut address: libc::sockaddr_nl = std::mem::zeroed();
            address.nl_family = libc::AF_NETLINK as libc::sa_family_t;
            address.nl_groups = 1 | 0x10;
            if libc::bind(
                fd.as_raw_fd(),
                (&address as *const libc::sockaddr_nl).cast(),
                std::mem::size_of::<libc::sockaddr_nl>() as libc::socklen_t,
            ) != 0
            {
                return Events::Closed;
            }
            Events::Real(fd)
        }
    }

    fn present(&self) -> bool {
        !matches!(self, Events::Closed)
    }

    fn fd(&self) -> Option<RawFd> {
        match self {
            Events::Real(fd) => Some(fd.as_raw_fd()),
            _ => None,
        }
    }

    /// `AddressEvents.wait(timeout, wake_fd)`: drain the wake pipe and a bounded burst of events;
    /// whether a link or address changed.
    fn wait(&mut self, timeout: f64, wake: RawFd) -> Result<bool, Failure> {
        let fds: Vec<RawFd> = std::iter::once(wake).chain(self.fd()).collect();
        let ready = select(&fds, timeout).map_err(os)?;
        if ready.contains(&wake) {
            let mut buffer = [0u8; 4096];
            // SAFETY: reads into a local buffer from our own non-blocking pipe until it is empty.
            while unsafe { libc::read(wake, buffer.as_mut_ptr().cast(), buffer.len()) } > 0 {}
        }
        let Some(fd) = self.fd().filter(|fd| ready.contains(fd)) else {
            return Ok(false);
        };
        // Drain a bounded burst. Remaining events stay readable next time.
        let mut buffer = vec![0u8; 65536];
        for _ in 0..32 {
            // SAFETY: receiving into a local buffer from our own non-blocking socket.
            let n = unsafe { libc::recv(fd, buffer.as_mut_ptr().cast(), buffer.len(), 0) };
            if n < 0 {
                if io::Error::last_os_error().kind() != io::ErrorKind::WouldBlock {
                    *self = Events::Closed; // refresh on the bounded fallback cadence
                }
                break;
            }
        }
        Ok(true)
    }
}

// ---- the supervisor -----------------------------------------------------------------------------

pub struct Supervisor {
    root: Root,
    pub attached: bool,
    pub next_health: f64,
    pub retry_at: f64,
    retry_delay: f64,
    next_restart: Vec<(&'static str, f64)>,
    pub pids: Vec<(&'static str, i32)>,
    /// The netdev the supplicant interface was added for.
    pub ifindex: Option<String>,
    disabled_since: Option<f64>,
    pub renewals: u32,
    pub next_renew: f64,
}

impl Supervisor {
    pub fn new(root: &Root) -> Self {
        Self {
            root: root.clone(),
            attached: false,
            next_health: 0.0,
            retry_at: 0.0,
            retry_delay: 2.0,
            next_restart: Vec::new(),
            pids: Vec::new(),
            ifindex: None,
            disabled_since: None,
            renewals: 0,
            next_renew: 0.0,
        }
    }

    fn run(&self, argv: &[&str], timeout: f64) -> command::Outcome {
        command::run(&self.root, argv, timeout)
    }

    /// Preserve the existing hook cleanup; these pid files are private and produced only by the
    /// boot-started helper/its fixed wpa action hook.
    fn stop(&self, name: &str) {
        self.run(
            &[
                "start-stop-daemon",
                "-K",
                "-p",
                &format!("/run/y2/{name}.pid"),
            ],
            2.0,
        );
    }

    fn restart(&mut self, name: &str) -> Result<(), Failure> {
        let mut extra: Vec<String> = Vec::new();
        let binary = match name {
            "wpa" => {
                self.run(&["/usr/libexec/y2/wpa-event", "wlan0", "DISCONNECTED"], 3.0);
                self.run(
                    &["/usr/sbin/y2-platform", "wifi-event", "SERVICE_RESTART"],
                    3.0,
                );
                self.stop("wifi-action");
                self.attached = false;
                self.run(
                    &[
                        "wpa_supplicant",
                        "-B",
                        "-u",
                        "-g",
                        GLOBAL,
                        "-P",
                        "/run/y2/wpa.pid",
                    ],
                    3.0,
                );
                return Ok(());
            }
            "bluealsa" => {
                match codec::runtime(&self.root, true) {
                    Ok(reply) => {
                        extra = ["-p", "a2dp-source", "--loglevel=warning"]
                            .map(str::to_owned)
                            .to_vec();
                        extra.extend(reply.stdout.split(' ').map(str::to_owned));
                    }
                    // codec_policy_unavailable: BlueALSA stays down until the next health look.
                    Err(Failure::Value(_) | Failure::Os { .. }) => return Ok(()),
                    // An inventory Python cannot read (AttributeError): the daemon ends as Python's,
                    // with its traceback (main.rs).
                    Err(error) => return Err(error),
                }
                "/usr/bin/bluealsad"
            }
            _ => HELPERS
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, b)| *b)
                .expect("a helper"),
        };
        if name == "calibration" {
            unlink_missing_ok(&self.root.path("/run/y2/calibration-ready"))?;
        }
        let pidfile = format!("/run/y2/{name}.pid");
        let mut argv = vec![
            "start-stop-daemon",
            "-S",
            "-b",
            "-m",
            "-p",
            &pidfile,
            "-x",
            binary,
            "--",
        ];
        argv.extend(extra.iter().map(String::as_str));
        self.run(&argv, 3.0);
        Ok(())
    }

    pub fn health(&mut self, now: f64, force: bool) -> Result<(), Failure> {
        if !force && now < self.next_health {
            return Ok(());
        }
        self.next_health = now + HEALTH_S;
        self.pids.clear();
        for (name, binary) in HELPERS {
            let mut pid = alive(&self.root, name, binary);
            let due = self
                .next_restart
                .iter()
                .find(|(n, _)| *n == name)
                .map_or(0.0, |(_, at)| *at);
            if pid.is_none() && now >= due {
                match self.next_restart.iter_mut().find(|(n, _)| *n == name) {
                    Some(entry) => entry.1 = now + HEALTH_S,
                    None => self.next_restart.push((name, now + HEALTH_S)),
                }
                unlink_missing_ok(&self.root.path(&format!("/run/y2/{name}.pid")))?;
                self.restart(name)?;
                pid = alive(&self.root, name, binary);
            }
            if let Some(pid) = pid {
                self.pids.push((name, pid));
            }
        }
        Ok(())
    }

    fn detach(&mut self) {
        self.run(&["/usr/libexec/y2/wpa-event", "wlan0", "DISCONNECTED"], 3.0);
        self.stop("wifi-action");
        self.run(&["wpa_cli", "-g", GLOBAL, "interface_remove", "wlan0"], 2.0);
        self.attached = false;
        self.ifindex = None;
        self.retry_at = 0.0;
        self.retry_delay = 2.0;
    }

    /// F53: re-add a supplicant interface left INTERFACE_DISABLED while wlan0 exists and no rfkill
    /// blocks it (the platform's own readiness record; no command while healthy).
    pub fn verify(&mut self, now: f64) {
        let root = &self.root;
        let net = root.json_object("/run/y2/network.json");
        let boot = root.read("/proc/sys/kernel/random/boot_id");
        let fresh = super::observe_wifi::eq_text(net.get("boot_id"), boot.as_deref())
            && number(net.get("monotonic_s"))
                .is_some_and(|at| (0.0..30.0).contains(&(clock::monotonic() - at)));
        let association = net.get("association").and_then(Value::as_str);
        if fresh && association == Some("COMPLETED") {
            self.renewals = 0;
        }
        if !self.attached
            || !fresh
            || association != Some("INTERFACE_DISABLED")
            || !net.get("radio_present").is_some_and(truthy)
            || wlan_switched_off(root)
        {
            self.disabled_since = None;
            return;
        }
        let since = *self.disabled_since.get_or_insert(now);
        if now - since < DISABLED_S || now < self.next_renew {
            return;
        }
        self.renewals += 1;
        self.next_renew = now + RENEW_MAX_S.min(60.0 * 2f64.powi(self.renewals as i32 - 1));
        let mut rfkill = Json::object();
        for path in root.glob("/sys/class/rfkill/rfkill*") {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let fields: Vec<Option<String>> = ["type", "soft", "hard"]
                .iter()
                .map(|f| root.read(&format!("/sys/class/rfkill/{name}/{f}")))
                .collect();
            rfkill = rfkill.with(&name, json!(fields));
        }
        journal(
            root,
            "wifi_interface_renewed",
            Json::object()
                .with("reason", "interface_disabled")
                .with("renewals", u64::from(self.renewals))
                .with("disabled_s", super::wifi::round1(now - since))
                .with("rfkill", rfkill),
        );
        self.disabled_since = None;
        self.detach();
    }

    pub fn interface(&mut self, now: f64) {
        let root = self.root.clone();
        let present = root.path("/sys/class/net/wlan0").is_dir();
        let index = if present {
            root.read("/sys/class/net/wlan0/ifindex")
        } else {
            None
        };
        if self.attached && present && index != self.ifindex {
            // F53: wlan0 went and came back between two looks (one netlink wake drained both). The
            // supplicant still had its interface for the old netdev; a stale one is never reused.
            journal(
                &root,
                "wifi_interface_renewed",
                Json::object().with("reason", "new_netdev"),
            );
            self.detach();
        }
        if !present {
            if self.attached {
                self.detach();
            }
            self.attached = false;
            self.retry_at = 0.0;
            self.retry_delay = 2.0;
            return;
        }
        if self.attached || now < self.retry_at {
            return;
        }
        let added = self.run(
            &[
                "wpa_cli",
                "-g",
                GLOBAL,
                "interface_add",
                "wlan0",
                "/data/network/wpa_supplicant.conf",
                "nl80211",
                "/run/wpa_supplicant",
            ],
            3.0,
        );
        let mut exists =
            added.ok && added.output.as_deref().unwrap_or_default().lines().last() == Some("OK");
        if !exists {
            // A monitor restart must reuse the existing supplicant interface.
            let listed = self.run(&["wpa_cli", "-g", GLOBAL, "interface_list"], 2.0);
            exists = listed.ok
                && listed
                    .output
                    .as_deref()
                    .unwrap_or_default()
                    .lines()
                    .any(|l| l == "wlan0");
        }
        if exists {
            // Runtime-only supplicant setting, re-applied whenever the interface is (re)added; best
            // effort, never blocks the action-hook attach.
            self.run(&["wpa_cli", "-i", "wlan0", "autoscan", AUTOSCAN], 2.0);
            let action = self.run(
                &[
                    "wpa_cli",
                    "-i",
                    "wlan0",
                    "-a",
                    "/usr/libexec/y2/wpa-event",
                    "-B",
                    "-P",
                    "/run/y2/wifi-action.pid",
                ],
                3.0,
            );
            self.attached = action.ok;
            self.ifindex = if self.attached { index } else { None };
        }
        self.retry_at = now + self.retry_delay;
        self.retry_delay = 60f64.min(self.retry_delay * 2.0);
    }

    pub fn cleanup(&self) {
        for name in [
            "wifi-action",
            "wifi-dhcp",
            "wpa",
            "reconnect",
            "bluealsa",
            "calibration",
        ] {
            self.stop(name);
        }
    }
}

fn pidfd_open(pid: i32) -> Option<OwnedFd> {
    // SAFETY: pidfd_open(2) returns a new descriptor this function then owns.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    (fd >= 0).then(|| unsafe { OwnedFd::from_raw_fd(fd as RawFd) })
}

/// One run of the loop until SIGTERM/SIGINT (or the fixture's tick count).
fn monitor(
    root: &Root,
    supervisor: &mut Supervisor,
    pidfds: &mut Vec<(&'static str, i32, OwnedFd)>,
    ticks: Option<u64>,
) -> Result<(), Failure> {
    let wake = SignalWake::new()?;
    let mut events = Events::open(root);
    let (mut changed, mut dead) = (true, true);
    let mut done = 0u64;
    while RUNNING.load(Ordering::SeqCst) {
        wake.drain();
        let now = clock::monotonic();
        supervisor.health(now, dead)?;
        supervisor.verify(now);
        if changed || !supervisor.attached || !events.present() {
            supervisor.interface(now);
        }
        // pidfds make helper death an event. Older hosts retain the existing 30 s health cadence;
        // healthy checks use native reads, no children.
        pidfds.retain(|(name, pid, _)| supervisor.pids.iter().any(|(n, p)| n == name && p == pid));
        for &(name, pid) in &supervisor.pids {
            if !pidfds.iter().any(|(n, _, _)| *n == name) {
                if let Some(fd) = pidfd_open(pid) {
                    pidfds.push((name, pid, fd));
                }
            }
        }
        let mut timeout = (supervisor.next_health - clock::monotonic()).max(0.0);
        if !supervisor.attached && root.path("/sys/class/net/wlan0").is_dir() {
            timeout = timeout.min((supervisor.retry_at - clock::monotonic()).max(0.05));
        }
        if ticks.is_some() {
            timeout = 0.0;
        }
        let readers: Vec<RawFd> = std::iter::once(wake.read.as_raw_fd())
            .chain(pidfds.iter().map(|(_, _, fd)| fd.as_raw_fd()))
            .chain(events.fd())
            .collect();
        let ready = select(&readers, timeout).map_err(os)?;
        changed = events.wait(0.0, wake.read.as_raw_fd())?;
        dead = pidfds
            .iter()
            .any(|(_, _, fd)| ready.contains(&fd.as_raw_fd()));
        if dead {
            // A failed restart must not leave an exited pidfd spinning.
            let mut index = 0;
            while index < pidfds.len() {
                if ready.contains(&pidfds[index].2.as_raw_fd()) {
                    let (name, _, _) = pidfds.remove(index);
                    supervisor.pids.retain(|(n, _)| *n != name);
                } else {
                    index += 1;
                }
            }
        }
        done += 1;
        if ticks.is_some_and(|n| done >= n) {
            break;
        }
    }
    drop(events);
    drop(wake);
    Ok(())
}

/// `y2-platform connectivity-daemon`: prints nothing and exits 0 when stopped; a second instance is
/// refused (BlockingIOError) before it touches any helper.
pub fn serve(root: &Root) -> Outcome {
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    let path = runtime.join("connectivity-monitor.lock");
    let lock = y2p_core::lock::try_exclusive(&path)
        .map_err(|e| Failure::os(e, &path))?
        .ok_or_else(|| os(io::Error::from_raw_os_error(libc::EWOULDBLOCK)))?;
    let ticks = if root.is_real() {
        None
    } else {
        env::var("Y2_DAEMON_TICKS")
            .ok()
            .and_then(|t| t.parse().ok())
    };
    let mut supervisor = Supervisor::new(root);
    let mut pidfds = Vec::new();
    let result = monitor(root, &mut supervisor, &mut pidfds, ticks);
    drop(pidfds);
    supervisor.cleanup();
    drop(lock);
    match result {
        // A daemon prints nothing (main.rs would print a newline for a reply).
        Ok(()) => std::process::exit(0),
        // An OSError is main.py's record; an AttributeError (an inventory `codecs` that is not an
        // object) its traceback on stderr, no stdout, exit 1 (main.rs prints both).
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realpath_resolves_as_python_does() {
        let dir = std::env::temp_dir().join(format!("y2-realpath-{}", std::process::id()));
        fs::create_dir_all(dir.join("usr/bin")).unwrap();
        fs::write(dir.join("usr/bin/tool"), "").unwrap();
        std::os::unix::fs::symlink("bin/tool", dir.join("usr/link")).unwrap();
        std::os::unix::fs::symlink(dir.join("usr/link"), dir.join("abs")).unwrap();
        let base = realpath(&dir);
        assert_eq!(realpath(&dir.join("usr/link")), base.join("usr/bin/tool"));
        assert_eq!(realpath(&dir.join("abs")), base.join("usr/bin/tool"));
        assert_eq!(realpath(&dir.join("missing/x")), base.join("missing/x"));
        assert_eq!(realpath(&dir.join("usr/../usr/bin")), base.join("usr/bin"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_second_instance_is_refused() {
        let dir = std::env::temp_dir().join(format!("y2-conn-{}", std::process::id()));
        fs::create_dir_all(dir.join("run/y2")).unwrap();
        let held =
            y2p_core::lock::exclusive(&dir.join("run/y2/connectivity-monitor.lock")).unwrap();
        let error = serve(&Root::new(&dir)).err().expect("refused");
        assert_eq!(
            error.message(),
            "[Errno 11] Resource temporarily unavailable"
        );
        assert!(matches!(
            error,
            Failure::Os {
                class: "BlockingIOError",
                ..
            }
        ));
        drop(held);
        fs::remove_dir_all(dir).unwrap();
    }
}
