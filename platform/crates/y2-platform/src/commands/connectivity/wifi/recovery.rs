//! The self-heals of a supplicant that does not connect: F52 (left idle) and F60 (blind scans).
// SPDX-License-Identifier: GPL-2.0-only
use super::journal::{journal, round1};
use super::monitor::{EventMonitor, CREDENTIAL_ERRORS};
use super::wpa::{digits, int, rows, signed, wpa};
use crate::commands::network::{wlan_rfkill_soft, wlan_switched_off};
use crate::commands::observation::snapshot::Wifi;
use serde::Serialize;
use std::{
    fs, thread,
    time::{Duration, Instant},
};
use y2p_core::{command, json::Object, Root};

/// F52: after a start or a wake the supplicant sat DISCONNECTED for up to 17 min with no failure
/// event while the saved network was in range; a Join (SELECT_NETWORK) connected in 3-4 s. Once that
/// lasts IDLE_S, do what the Join does: select the saved network in range and enable the others again
/// (Reborn's join ends so too). Not in range: one scan, so the next look sees fresh results. Backed
/// off per episode; every try is a journal row.
///
/// F60: after a wake the supplicant kept scanning for 45 s and never saw the saved network (scans
/// found four others, a Join said not found); Wi-Fi off and on in Reborn connected in 5 s at
/// -59 dBm. Once scans have missed every saved network RESTART_SCANS times over RESTART_AFTER_S,
/// do what that switch does, at most every RESTART_GAP_S until a connect.
pub struct WifiRecovery {
    root: Root,
    since: Option<f64>,
    tries: u64,
    next_try: f64,
    first_try: Option<f64>,
    /// F60: since when Wi-Fi is up and unconnected, and the supplicant's NETWORK-NOT-FOUND count then.
    unseen: Option<(f64, u64)>,
    /// F60: the last restart since the last connect.
    restarted: Option<f64>,
}

impl WifiRecovery {
    pub const IDLE_S: f64 = 30.0;
    pub const BACKOFF_S: [f64; 4] = [30.0, 60.0, 120.0, 300.0];
    pub const RESTART_AFTER_S: f64 = 60.0;
    pub const RESTART_SCANS: u64 = 4;
    pub const RESTART_GAP_S: f64 = 600.0;

    pub fn new(root: &Root) -> Self {
        Self {
            root: root.clone(),
            since: None,
            tries: 0,
            next_try: 0.0,
            first_try: None,
            unseen: None,
            restarted: None,
        }
    }

    pub fn observe(
        &mut self,
        value: &Wifi,
        now: f64,
        monitor: Option<&EventMonitor>,
    ) -> Option<&'static str> {
        let root = &self.root.clone();
        let state = value.association.as_deref();
        let completed = state == Some("COMPLETED");
        if completed || !value.radio_present || wlan_switched_off(root) {
            if self.tries > 0 && completed {
                let seconds = round1(now - self.first_try.unwrap_or(now));
                journal(
                    root,
                    "wifi_recovered",
                    &Recovered {
                        tries: self.tries,
                        seconds,
                    },
                );
            }
            self.since = None;
            self.tries = 0;
            self.next_try = 0.0;
            self.first_try = None;
            self.unseen = None;
            if completed {
                self.restarted = None;
            }
            return None;
        }
        let credential = value
            .reason
            .as_deref()
            .is_some_and(|r| CREDENTIAL_ERRORS.contains(&r));
        if let Some(action) = self.restart_unseen(state, credential, now, monitor) {
            return Some(action);
        }
        // The supplicant is trying (scan, association), a password verdict stands or the owner
        // disconnected: never interfere.
        if !matches!(state, Some("DISCONNECTED" | "INACTIVE"))
            || credential
            || monitor.is_some_and(|m| m.held)
        {
            self.since = None;
            return None;
        }
        let since = *self.since.get_or_insert(now);
        if now - since < Self::IDLE_S || now < self.next_try {
            return None;
        }
        let look = Look::take(root);
        self.next_try = now + Self::BACKOFF_S[(self.tries as usize).min(Self::BACKOFF_S.len() - 1)];
        let look = look?;
        if look.saved.is_empty() {
            return None;
        }
        let (saved, signal) = (&look.saved, &look.signal);
        let strength = |name: &str| look.strength(name);
        let seen = look.seen();
        self.tries += 1;
        self.first_try.get_or_insert(now);
        let (action, outcome) = if !seen.is_empty() {
            let last = monitor.and_then(|m| m.connected_network);
            let chosen = seen
                .iter()
                .find(|row| Some(int(&row[0])) == last)
                .copied()
                .unwrap_or_else(|| {
                    // The strongest; of equal signals the lowest id.
                    let key =
                        |row: &Vec<String>| (strength(&row[1]).unwrap_or(-999), -int(&row[0]));
                    let mut best = seen[0];
                    for row in &seen[1..] {
                        if key(row) > key(best) {
                            best = row;
                        }
                    }
                    best
                });
            let selected = wpa(root, &["select_network", &chosen[0]]);
            let enabled = wpa(root, &["enable_network", "all"]);
            let outcome = Outcome::Reselect {
                network_id: int(&chosen[0]),
                last_connected: last.is_some_and(|l| chosen[0] == l.to_string()),
                ok: [answered_ok(&selected), answered_ok(&enabled)],
            };
            ("wifi_reselect", outcome)
        } else {
            let answer = wpa(root, &["scan"]);
            (
                "wifi_rescan",
                Outcome::Rescan {
                    ok: answered_ok(&answer),
                },
            )
        };
        let attempt = Attempt {
            state,
            idle_s: round1(now - since),
            tries: self.tries,
            saved: saved.len(),
            in_range: seen.len(),
            scan_entries: signal.len(),
            temp_disabled: saved
                .iter()
                .filter(|row| row[3].contains("TEMP-DISABLED"))
                .count(),
            outcome,
            events: monitor.map(|m| m.summary(Some(now))),
        };
        journal(root, action, &attempt);
        Some(action)
    }

    /// F60: Wi-Fi off and on when scanning (or waiting between scans) misses every saved network
    /// for long enough; not while a password verdict stands or after the owner's Disconnect.
    fn restart_unseen(
        &mut self,
        state: Option<&str>,
        credential: bool,
        now: f64,
        monitor: Option<&EventMonitor>,
    ) -> Option<&'static str> {
        let monitor = monitor?;
        let not_found = monitor.total("NETWORK-NOT-FOUND");
        let (since, before) = *self.unseen.get_or_insert((now, not_found));
        if !matches!(state, Some("SCANNING" | "DISCONNECTED" | "INACTIVE"))
            || credential
            || monitor.held
            || now - since < Self::RESTART_AFTER_S
            || not_found.saturating_sub(before) < Self::RESTART_SCANS
            || self
                .restarted
                .is_some_and(|at| now - at < Self::RESTART_GAP_S)
        {
            return None;
        }
        let look = Look::take(&self.root)?;
        if look.saved.is_empty() || !look.seen().is_empty() {
            return None;
        }
        self.restarted = Some(now);
        self.unseen = None;
        let radio = restart_radio(&self.root);
        let restart = Restart {
            unseen_s: round1(now - since),
            not_found: not_found - before,
            saved: look.saved.len(),
            scan_entries: look.signal.len(),
            radio,
            events: monitor.summary(Some(now)),
        };
        journal(&self.root, "wifi_restart", &restart);
        Some("wifi_restart")
    }
}

/// How long a restart waits for the driver to take wlan0 away: a block undone before its worker
/// ran is no restart at all (ahb.c radio_work compares wanted with running).
const RADIO_GONE: Duration = Duration::from_secs(3);

/// Wi-Fi off and on as Reborn's switch does it: block, wait for wlan0 to go, unblock. The owner's
/// preference is Reborn's file, untouched.
fn restart_radio(root: &Root) -> RadioRestart {
    let Some(soft) = wlan_rfkill_soft(root) else {
        return RadioRestart::default();
    };
    let netdev = root.path("/sys/class/net/wlan0");
    let blocked = fs::write(&soft, "1\n").is_ok();
    let deadline = Instant::now() + RADIO_GONE;
    while blocked && netdev.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(100));
    }
    RadioRestart {
        blocked,
        gone: !netdev.exists(),
        unblocked: fs::write(&soft, "0\n").is_ok(),
    }
}

/// What a restart did: blocked Wi-Fi, saw wlan0 go, unblocked it.
#[derive(Serialize, Default)]
struct RadioRestart {
    blocked: bool,
    gone: bool,
    unblocked: bool,
}

/// The journal row of an F60 restart (`wifi_restart`).
#[derive(Serialize)]
struct Restart {
    unseen_s: f64,
    not_found: u64,
    saved: usize,
    scan_entries: usize,
    #[serde(flatten)]
    radio: RadioRestart,
    events: Object,
}

/// The supplicant's saved networks (not disabled, as `list_networks` rows) and the strongest signal
/// per network name in its last scan.
struct Look {
    saved: Vec<Vec<String>>,
    signal: Vec<(String, i64)>,
}

impl Look {
    /// None when wpa_cli did not answer both.
    fn take(root: &Root) -> Option<Self> {
        let listed = wpa(root, &["list_networks"]);
        let scan = wpa(root, &["scan_results"]);
        if !(listed.ok && scan.ok) {
            return None;
        }
        let saved = rows(listed.output.as_deref())
            .into_iter()
            .filter(|row| {
                row.len() >= 4
                    && digits(&row[0])
                    && !row[1].is_empty()
                    && !row[3].contains("[DISABLED]")
            })
            .collect();
        let mut signal: Vec<(String, i64)> = Vec::new();
        for row in rows(scan.output.as_deref()) {
            if row.len() >= 5 && !row[4].is_empty() && signed(&row[2]) {
                let level = int(&row[2]);
                match signal.iter_mut().find(|(name, _)| *name == row[4]) {
                    Some(entry) => entry.1 = entry.1.max(level),
                    None => signal.push((row[4].clone(), level.max(-999))),
                }
            }
        }
        Some(Self { saved, signal })
    }

    fn strength(&self, name: &str) -> Option<i64> {
        self.signal.iter().find(|(n, _)| n == name).map(|(_, s)| *s)
    }

    /// The saved networks the last scan saw.
    fn seen(&self) -> Vec<&Vec<String>> {
        (self.saved.iter())
            .filter(|row| self.strength(&row[1]).is_some())
            .collect()
    }
}

/// Whether wpa_cli answered `OK`.
fn answered_ok(answer: &command::Outcome) -> bool {
    y2p_core::root::python_strip(answer.output.as_deref().unwrap_or_default()) == "OK"
}

/// The journal row of a recovery: tries until the supplicant completed, seconds since the first.
#[derive(Serialize)]
struct Recovered {
    tries: u64,
    seconds: f64,
}

/// The journal row of one self-heal try (`wifi_reselect` or `wifi_rescan`).
#[derive(Serialize)]
struct Attempt<'a> {
    state: Option<&'a str>,
    idle_s: f64,
    tries: u64,
    saved: usize,
    in_range: usize,
    scan_entries: usize,
    temp_disabled: usize,
    #[serde(flatten)]
    outcome: Outcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    events: Option<Object>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Outcome {
    /// The saved network selected (the last that connected, else the strongest) and the others
    /// enabled again; whether each answered OK.
    Reselect {
        network_id: i64,
        last_connected: bool,
        ok: [bool; 2],
    },
    /// None in range: one scan.
    Rescan { ok: bool },
}
