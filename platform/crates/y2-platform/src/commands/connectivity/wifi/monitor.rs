//! The supplicant event monitor: event counts for reports, the failure record with the F52 password
//! latch Reborn reads, and the scan evidence of a "not found".
// SPDX-License-Identifier: GPL-2.0-only
use super::super::observe_wifi::{clock_value, eq_text};
use super::journal::round1;
use super::scan::scan_evidence;
use super::wpa::{search_value, value_of};
use crate::commands::network::{publish, wlan_switched_off};
use crate::reply::Failure;
use serde_json::{json, Value};
use std::{
    fs, io,
    os::{
        fd::{AsRawFd, RawFd},
        unix::{fs::MetadataExt, net::UnixDatagram},
    },
    path::PathBuf,
    time::Duration,
};
use y2p_core::{
    json::{Object, ObjectExt},
    py::{self, truthy},
    Root,
};

/// Only a password verdict is kept until a connect. A rejected or failed association (CONN_FAILED,
/// AUTH-REJECT) is no password on WPA2-PSK.
pub const CREDENTIAL_ERRORS: [&str; 1] = ["wrong_credentials"];
const LAST_FAILURE_KEYS: [&str; 8] = [
    "error",
    "reason",
    "epoch",
    "failures",
    "scan",
    "scan_monotonic_s",
    "boot_id",
    "monotonic_s",
];
/// F52 (aa96e00a): wpa_supplicant says WRONG_KEY for any 4-way handshake that ends early (events.c
/// could_be_psk_mismatch), also one this player cut short (a Join, the sleep's radio off). A password
/// is wrong only when one network fails this often with no connect between; a single failure is
/// `handshake_failed`, one after our own disconnect is not counted.
pub const WRONG_KEY_REPEAT: i64 = 2;
/// Supplicant events counted for reports: what it did while it sat DISCONNECTED (F52). Kinds and times
/// only, never the event text.
const EVENT_KINDS: [&str; 11] = [
    "CONNECTED",
    "DISCONNECTED",
    "SSID-TEMP-DISABLED",
    "SSID-REENABLED",
    "ASSOC-REJECT",
    "AUTH-REJECT",
    "NETWORK-NOT-FOUND",
    "SCAN-STARTED",
    "SCAN-RESULTS",
    "SCAN-FAILED",
    "AUTH-TIMEOUT",
];

/// Passive supplicant events. Never issues CONNECT or owns reconnect.
pub struct EventMonitor {
    root: Root,
    pub(crate) sock: Option<UnixDatagram>,
    path: Option<PathBuf>,
    generation: Option<(u64, i128)>,
    started: f64,
    counts: Vec<(String, u64)>,
    last: Vec<(String, f64)>,
    last_disconnect: Value,
    pub(crate) interrupted_handshakes: u64,
    /// The network of the last connect: the one a self-heal selects.
    pub connected_network: Option<i64>,
    /// wpa_supplicant's handshake timeout (≈10 s; how a wrong password ends on the Y2) also
    /// disconnects locally; any other local disconnect cut a handshake short and proves nothing
    /// about the password.
    timed_out: bool,
    interrupted: bool,
    /// A local disconnect with the radio on and no try since: the owner's Disconnect, which a
    /// self-heal must not undo.
    pub held: bool,
}

impl EventMonitor {
    pub const SCAN_EVIDENCE_S: f64 = 30.0;

    pub fn new(root: &Root) -> Self {
        Self {
            root: root.clone(),
            sock: None,
            path: None,
            generation: None,
            started: root.clock().monotonic(),
            counts: Vec::new(),
            last: Vec::new(),
            last_disconnect: Value::Null,
            interrupted_handshakes: 0,
            connected_network: None,
            timed_out: false,
            interrupted: false,
            held: false,
        }
    }

    /// Counts and ages of the supplicant's events for network.json and the journal.
    pub fn summary(&self, now: Option<f64>) -> Object {
        let now = now.unwrap_or_else(|| self.root.clock().monotonic());
        let mut counts = Object::new();
        for (kind, n) in &self.counts {
            counts = counts.with(kind, *n);
        }
        let mut ago = Object::new();
        for (kind, at) in &self.last {
            ago = ago.with(kind, round1(now - at));
        }
        Object::new()
            .with("since_s", round1(now - self.started))
            .with("counts", counts)
            .with("ago_s", ago)
            .with("last_disconnect", self.last_disconnect.clone())
            .with("interrupted_handshakes", self.interrupted_handshakes)
    }

    /// The socket a caller's select waits on, if attached.
    pub fn fd(&self) -> Option<RawFd> {
        self.sock.as_ref().map(AsRawFd::as_raw_fd)
    }

    pub fn close(&mut self) {
        self.sock = None;
        if let Some(path) = self.path.take() {
            let _ = fs::remove_file(path);
        }
        self.generation = None;
    }

    pub fn poll(&mut self) {
        let remote = self.root.path("/run/wpa_supplicant/wlan0");
        if !remote.exists() {
            self.close();
            return;
        }
        if self.attach_and_read(&remote).is_err() {
            self.close();
        }
    }

    fn attach_and_read(&mut self, remote: &std::path::Path) -> Result<(), ()> {
        let meta = fs::metadata(remote).map_err(drop)?;
        let generation = (
            meta.ino(),
            i128::from(meta.ctime()) * 1_000_000_000 + i128::from(meta.ctime_nsec()),
        );
        if self.generation.is_some_and(|g| g != generation) {
            self.close();
            self.held = false;
        }
        if self.sock.is_none() {
            let path = self.root.path("/run/y2/wpa-events.sock");
            match fs::remove_file(&path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(()),
                _ => {}
            }
            self.path = Some(path.clone());
            let sock = &*self.sock.insert(UnixDatagram::bind(&path).map_err(drop)?);
            let timeout = Some(Duration::from_millis(200));
            sock.set_read_timeout(timeout).map_err(drop)?;
            sock.set_write_timeout(timeout).map_err(drop)?;
            sock.connect(remote).map_err(drop)?;
            sock.send(b"ATTACH").map_err(drop)?;
            let mut answer = [0u8; 1024];
            let n = sock.recv(&mut answer).map_err(drop)?;
            if answer[..n].trim_ascii() != b"OK" {
                return Err(()); // monitor_attach_failed
            }
            sock.set_nonblocking(true).map_err(drop)?;
            self.generation = Some(generation);
        }
        for _ in 0..16 {
            let mut buffer = [0u8; 4096];
            let Some(sock) = &self.sock else {
                return Err(());
            };
            match sock.recv(&mut buffer) {
                Ok(n) => {
                    let text = String::from_utf8_lossy(&buffer[..n]).into_owned();
                    self.event(&text).map_err(drop)?;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => return Err(()),
            }
        }
        Ok(())
    }

    /// Events wake readiness immediately. DHCP remains owned by wpa-event; the bounded acquisition
    /// cadence also observes its lease publication.
    pub fn wait(&mut self, timeout: f64, wake_fd: Option<RawFd>) {
        let readers: Vec<RawFd> = self.fd().into_iter().chain(wake_fd).collect();
        if readers.is_empty() {
            std::thread::sleep(Duration::from_secs_f64(timeout.max(0.0)));
            return;
        }
        if super::super::daemon::select(&readers, timeout).is_err() {
            self.close();
        }
    }

    fn count(&mut self, kind: &str) {
        let now = self.root.clock().monotonic();
        match self.counts.iter_mut().find(|(k, _)| k == kind) {
            Some(entry) => entry.1 += 1,
            None => self.counts.push((kind.to_owned(), 1)),
        }
        match self.last.iter_mut().find(|(k, _)| k == kind) {
            Some(entry) => entry.1 = now,
            None => self.last.push((kind.to_owned(), now)),
        }
    }

    /// One supplicant event line (`<3>CTRL-EVENT-… …`).
    pub fn event(&mut self, text: &str) -> Result<(), Failure> {
        let body = text
            .strip_prefix('<')
            .and_then(|rest| rest.split_once('>'))
            .map_or(text, |(_, body)| body);
        let body = y2p_core::root::python_strip(body);
        if body.starts_with("Trying to associate with ") {
            self.held = false;
            return Ok(());
        }
        let kind = if body.starts_with("Authentication with ") && body.ends_with("timed out.") {
            "AUTH-TIMEOUT".to_owned()
        } else if let Some(rest) = body.strip_prefix("CTRL-EVENT-") {
            rest.split(' ').next().unwrap_or_default().to_owned()
        } else {
            return Ok(());
        };
        if EVENT_KINDS.contains(&kind.as_str()) {
            self.count(&kind);
        }
        if kind == "AUTH-TIMEOUT" {
            self.timed_out = true;
            return Ok(());
        }
        if kind == "DISCONNECTED" {
            let code = search_value(body, &["reason"], |c| c.is_ascii_digit(), 5)
                .and_then(|c| c.parse::<i64>().ok());
            let local = body.contains("locally_generated=1");
            self.last_disconnect = Value::Object(
                Object::new()
                    .with("reason", json!(code))
                    .with("local", local)
                    .with("timeout", self.timed_out),
            );
            self.interrupted = local && !self.timed_out;
            self.held = self.interrupted && !wlan_switched_off(&self.root);
            self.timed_out = false;
            return Ok(());
        }
        if [
            "CONNECTED",
            "SSID-TEMP-DISABLED",
            "ASSOC-REJECT",
            "AUTH-REJECT",
        ]
        .contains(&kind.as_str())
        {
            self.held = false;
        }
        let network = search_value(text, &["id"], |c| c.is_ascii_digit(), usize::MAX)
            .and_then(|c| c.parse::<i64>().ok());
        let mut error = match kind.as_str() {
            "SSID-TEMP-DISABLED" => {
                // A sleep blocks the radio mid-handshake: cfg80211 disconnects locally (950a93ac
                // 246 s); the rfkill read covers a missed event.
                let interrupted = self.interrupted || wlan_switched_off(&self.root);
                self.interrupted = false;
                if !text.contains("reason=WRONG_KEY") {
                    Some("authentication_failed")
                } else if interrupted {
                    self.interrupted_handshakes += 1;
                    return Ok(());
                } else {
                    Some("handshake_failed")
                }
            }
            "AUTH-REJECT" => Some("authentication_rejected"),
            "NETWORK-NOT-FOUND" => Some("ap_unavailable"),
            "CONNECTED" => {
                self.connected_network = network;
                None
            }
            _ => return Ok(()),
        };
        let root = &self.root;
        let old = root.json_object("/run/y2/wifi-failure.json");
        let boot = root.boot_id();
        // Failed handshakes per network since the last connect (kept here, so a service restart does
        // not forget the first one).
        let valid = match old.get("wrong_key") {
            Some(Value::Object(w)) => {
                eq_text(w.get("boot_id"), boot.as_deref())
                    && w.get("count")
                        .is_some_and(|c| c.is_boolean() || c.is_i64() || c.is_u64())
            }
            _ => false,
        };
        let mut wrong_key = (valid && kind != "CONNECTED").then(|| old["wrong_key"].clone());
        if error == Some("handshake_failed") {
            let count = match &wrong_key {
                Some(w) if py::eq(w.get("network_id"), Some(&json!(network))) => {
                    y2p_core::py::number(w.get("count")).unwrap_or(0.0) as i64 + 1
                }
                _ => 1,
            };
            wrong_key = Some(value_of(
                Object::new()
                    .with("network_id", json!(network))
                    .with("count", count)
                    .with("boot_id", json!(boot)),
            ));
            if count >= WRONG_KEY_REPEAT {
                error = Some("wrong_credentials");
            }
        }
        let failures = match old.get("failures") {
            None => 0,
            Some(Value::Bool(b)) => i64::from(*b),
            Some(v) => v.as_i64().unwrap_or(0),
        };
        let mut record = Object::new()
            .with("error", json!(error))
            .with("epoch", json!(root.read("/run/y2/wifi-epoch")))
            .with("failures", failures + i64::from(error.is_some()));
        if let Some(w) = wrong_key {
            record.set("wrong_key", w);
        }
        // wpa_supplicant's own code (reason=CONN_FAILED, status_code=15): it separates a password
        // from a timeout or SAE (F31). Never a name.
        if error.is_some() {
            let class = |c: char| c.is_ascii_alphanumeric() || c == '_';
            if let Some(detail) = search_value(text, &["reason", "status_code"], class, 32) {
                record.set("reason", detail);
            }
        }
        // A connect ends a failure run: keep what the last failure showed (F13's scan evidence was
        // gone by the time a report was saved).
        let last = if old.get("error").is_some_and(truthy) {
            Some(Value::Object(old.clone()))
        } else {
            old.get("last_failure").cloned()
        };
        if let Some(Value::Object(last)) = last {
            if last.get("error").is_some_and(truthy) {
                let mut kept = Object::new();
                for key in LAST_FAILURE_KEYS {
                    if let Some(value) = last.get(key) {
                        kept.set(key, value.clone());
                    }
                }
                record.set("last_failure", value_of(kept));
            }
        }
        // Y2Reborn #20: a wrong password stays reported until a connect. The "network not found"
        // that follows (the network is disabled for a while after each failure) never replaces it;
        // it names the network it was.
        if error.is_some_and(|e| CREDENTIAL_ERRORS.contains(&e)) {
            let reason = record.get("reason").cloned().unwrap_or(Value::Null);
            record.set(
                "credential",
                value_of(
                    Object::new()
                        .with("error", json!(error))
                        .with("reason", reason)
                        .with("boot_id", json!(boot))
                        .with("network_id", json!(network))
                        .with("monotonic_s", root.clock().monotonic()),
                ),
            );
        } else if error.is_some() {
            if let Some(Value::Object(credential)) = old.get("credential") {
                if eq_text(credential.get("boot_id"), boot.as_deref()) {
                    record.set("credential", Value::Object(credential.clone()));
                }
            }
        }
        if error == Some("ap_unavailable") {
            // wpa_supplicant reports this after every scan: look at most every 30 s.
            let last = clock_value(old.get("scan_monotonic_s"));
            let cached = eq_text(old.get("boot_id"), boot.as_deref())
                && last.is_some_and(|at| {
                    (0.0..Self::SCAN_EVIDENCE_S).contains(&(root.clock().monotonic() - at))
                })
                && matches!(old.get("scan"), Some(Value::Object(_)));
            if cached {
                record
                    .set("scan", old["scan"].clone())
                    .set("scan_monotonic_s", old["scan_monotonic_s"].clone());
            } else {
                record
                    .set("scan", value_of(scan_evidence(root)))
                    .set("scan_monotonic_s", root.clock().monotonic());
            }
        }
        publish(root, "wifi-failure", &record).map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A supplicant control socket that answers ATTACH and then sends two events.
    #[test]
    fn the_monitor_attaches_to_the_supplicant_and_counts_its_events() {
        let dir = std::env::temp_dir().join(format!("y2-wpa-{}", std::process::id()));
        fs::create_dir_all(dir.join("run/wpa_supplicant")).unwrap();
        fs::create_dir_all(dir.join("run/y2")).unwrap();
        let server = UnixDatagram::bind(dir.join("run/wpa_supplicant/wlan0")).unwrap();
        let supplicant = std::thread::spawn(move || {
            let mut buffer = [0u8; 64];
            let (n, peer) = server.recv_from(&mut buffer).unwrap();
            assert_eq!(&buffer[..n], b"ATTACH");
            let peer = peer.as_pathname().unwrap().to_owned();
            server.send_to(b"OK\n", &peer).unwrap();
            server
                .send_to(b"<3>CTRL-EVENT-SCAN-STARTED ", &peer)
                .unwrap();
            let connected =
                b"<3>CTRL-EVENT-CONNECTED - Connection to 00:11:22:33:44:55 completed [id=7 id_str=]";
            server.send_to(connected, &peer).unwrap();
        });
        let root = Root::new(&dir);
        let mut monitor = EventMonitor::new(&root);
        monitor.poll();
        supplicant.join().unwrap();
        assert!(monitor.fd().is_some(), "attached");
        assert!(dir.join("run/y2/wpa-events.sock").exists());
        monitor.poll();
        let summary = monitor.summary(None).into_sorted();
        assert_eq!(
            summary["counts"],
            json!({"SCAN-STARTED": 1, "CONNECTED": 1})
        );
        assert_eq!(monitor.connected_network, Some(7));
        assert!(dir.join("run/y2/wifi-failure.json").exists());
        // The supplicant went away: the monitor lets go of its socket.
        fs::remove_file(dir.join("run/wpa_supplicant/wlan0")).unwrap();
        monitor.poll();
        assert!(monitor.fd().is_none());
        assert!(!dir.join("run/y2/wpa-events.sock").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
