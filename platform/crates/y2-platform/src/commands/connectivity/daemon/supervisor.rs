//! The radio helpers' supervisor: restart a dead helper (rate-bounded), attach wpa_supplicant to
//! wlan0 with the wpa-event action hook, re-add a stale interface (F53), stop every helper at the end.
// SPDX-License-Identifier: GPL-2.0-only
use super::super::wifi::journal;
use crate::commands::{codec, network::wlan_switched_off};
use crate::reply::{unlink_missing_ok, Failure};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    cell::OnceCell,
    ffi::OsString,
    fs,
    path::{Component, Path, PathBuf},
};
use y2p_core::json::{Object, ObjectExt};
use y2p_core::{
    command,
    py::{number, truthy},
    Root,
};

pub(crate) const HELPERS: [(&str, &str); 4] = [
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

/// A path with its symlinks resolved as far as it exists, `..` after resolving, the rest appended as
/// written (what `/proc/<pid>/exe` is compared with).
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

/// The helper's pid when its pid file names a live process running that binary (a reused pid runs
/// something else). The liveness probe is a real `kill(pid, 0)`, also below a fixture root.
/// `resolved` is the binary's path below the root with its symlinks resolved (`realpath`).
fn alive(root: &Root, name: &str, resolved: &Path) -> Option<i32> {
    let pid = root.integer(&format!("/run/y2/{name}.pid"))?;
    if pid <= 1 {
        return None;
    }
    let exe = fs::read_link(root.path(&format!("/proc/{pid}/exe"))).ok()?;
    if exe != resolved {
        return None;
    }
    let pid = i32::try_from(pid).ok()?;
    // SAFETY: signal 0 only checks that the process exists.
    (unsafe { libc::kill(pid, 0) } == 0).then_some(pid)
}

// ---- the supervisor -----------------------------------------------------------------------------

pub struct Supervisor {
    root: Root,
    pub attached: bool,
    pub next_health: f64,
    pub retry_at: f64,
    pub(crate) retry_delay: f64,
    next_restart: Vec<(&'static str, f64)>,
    pub pids: Vec<(&'static str, i32)>,
    /// The netdev the supplicant interface was added for.
    pub ifindex: Option<String>,
    disabled_since: Option<f64>,
    pub renewals: u32,
    pub next_renew: f64,
    /// Read once: neither changes while the daemon runs (the helpers are on the read-only root).
    boot_id: OnceCell<Option<String>>,
    binaries: OnceCell<Vec<PathBuf>>,
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
            boot_id: OnceCell::new(),
            binaries: OnceCell::new(),
        }
    }

    /// This boot's id.
    fn boot_id(&self) -> Option<&str> {
        self.boot_id.get_or_init(|| self.root.boot_id()).as_deref()
    }

    /// The helpers' binaries with their symlinks resolved, in `HELPERS` order.
    fn binaries(&self) -> &[PathBuf] {
        self.binaries.get_or_init(|| {
            HELPERS
                .iter()
                .map(|(_, binary)| realpath(&self.root.path(binary)))
                .collect()
        })
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

    pub(crate) fn restart(&mut self, name: &str) -> Result<(), Failure> {
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
                    // A policy or inventory codec-runtime refuses (or a receipt it cannot write):
                    // BlueALSA stays down until the next health look, the other helpers run on.
                    Err(_) => return Ok(()),
                }
                "/usr/bin/bluealsad"
            }
            _ => match HELPERS.iter().find(|(n, _)| *n == name) {
                Some((_, binary)) => binary,
                // Not a helper of this daemon: nothing to start.
                None => return Ok(()),
            },
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
        for (index, (name, _)) in HELPERS.into_iter().enumerate() {
            let mut pid = alive(&self.root, name, &self.binaries()[index]);
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
                pid = alive(&self.root, name, &self.binaries()[index]);
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
        let fresh = super::super::observe_wifi::eq_text(net.get("boot_id"), self.boot_id())
            && number(net.get("monotonic_s"))
                .is_some_and(|at| (0.0..30.0).contains(&(root.clock().monotonic() - at)));
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
        let mut rfkill = Object::new();
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
            &Renewed {
                reason: "interface_disabled",
                renewals: Some(self.renewals),
                disabled_s: Some(super::super::wifi::round1(now - since)),
                rfkill: Some(rfkill),
            },
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
                &Renewed {
                    reason: "new_netdev",
                    ..Renewed::default()
                },
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

/// The journal row of a supplicant interface added again (F53): why; for one left disabled, how
/// often and how long, and every rfkill switch as `[type, soft, hard]`.
#[derive(Serialize, Default)]
struct Renewed {
    reason: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    renewals: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    disabled_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rfkill: Option<Object>,
}

#[cfg(test)]
#[path = "supervisor_tests.rs"]
mod supervisor_tests;

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
}
