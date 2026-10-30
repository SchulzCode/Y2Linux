//! The F52 self-heal of a supplicant left idle.
// SPDX-License-Identifier: GPL-2.0-only
use super::journal::{journal, round1};
use super::monitor::{EventMonitor, CREDENTIAL_ERRORS};
use super::wpa::{digits, int, rows, signed, wpa};
use crate::commands::network::wlan_switched_off;
use crate::commands::observation::snapshot::Wifi;
use serde::Serialize;
use y2p_core::{command, json::Object, Root};

/// F52: after a start or a wake the supplicant sat DISCONNECTED for up to 17 min with no failure
/// event while the saved network was in range; a Join (SELECT_NETWORK) connected in 3-4 s. Once that
/// lasts IDLE_S, do what the Join does: select the saved network in range and enable the others again
/// (Reborn's join ends so too). Not in range: one scan, so the next look sees fresh results. Backed
/// off per episode; every try is a journal row.
pub struct WifiRecovery {
    root: Root,
    since: Option<f64>,
    tries: u64,
    next_try: f64,
    first_try: Option<f64>,
}

impl WifiRecovery {
    pub const IDLE_S: f64 = 30.0;
    pub const BACKOFF_S: [f64; 4] = [30.0, 60.0, 120.0, 300.0];

    pub fn new(root: &Root) -> Self {
        Self {
            root: root.clone(),
            since: None,
            tries: 0,
            next_try: 0.0,
            first_try: None,
        }
    }

    pub fn observe(
        &mut self,
        value: &Wifi,
        now: f64,
        monitor: Option<&EventMonitor>,
    ) -> Option<&'static str> {
        let root = &self.root;
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
            return None;
        }
        // The supplicant is trying (scan, association), a password verdict stands or the owner
        // disconnected: never interfere.
        let credential = value
            .reason
            .as_deref()
            .is_some_and(|r| CREDENTIAL_ERRORS.contains(&r));
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
        let listed = wpa(root, &["list_networks"]);
        let scan = wpa(root, &["scan_results"]);
        self.next_try = now + Self::BACKOFF_S[(self.tries as usize).min(Self::BACKOFF_S.len() - 1)];
        if !(listed.ok && scan.ok) {
            return None;
        }
        let saved: Vec<Vec<String>> = rows(listed.output.as_deref())
            .into_iter()
            .filter(|row| {
                row.len() >= 4
                    && digits(&row[0])
                    && !row[1].is_empty()
                    && !row[3].contains("[DISABLED]")
            })
            .collect();
        if saved.is_empty() {
            return None;
        }
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
        let strength = |name: &str| signal.iter().find(|(n, _)| n == name).map(|(_, s)| *s);
        let seen: Vec<&Vec<String>> = saved
            .iter()
            .filter(|row| strength(&row[1]).is_some())
            .collect();
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
