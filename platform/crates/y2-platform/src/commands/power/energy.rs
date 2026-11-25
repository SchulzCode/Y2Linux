//! Where the battery went (power plan 2026-11, phase 2; Android's batterystats idea): the power
//! daemon counts, per boot, the time the player spent in each state and, while it runs on the
//! battery, how much the voltage estimate's remaining charge fell in each. The board has no current
//! sensor, so the charge figures are the fuel-gauge estimate's (noisy from one second to the next,
//! meaningful over hours); a measured current per state (`power-profile.json`, written once someone
//! measures with a meter) adds a second estimate, time × current.
//!
//! Written to `/data/system/platform/energy.json` every 10 minutes; the previous boot's record is
//! kept as `energy.previous.json`. Error reports carry both.
// SPDX-License-Identifier: GPL-2.0-only
use super::battery::BatteryStatus;
use crate::reply::{atomic_json, Failure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use y2p_core::Root;

pub const RECORD: &str = "/data/system/platform/energy.json";
pub const PREVIOUS: &str = "/data/system/platform/energy.previous.json";
/// Currents per state measured with a meter (absent until someone does).
pub const PROFILE: &str = "/data/system/platform/power-profile.json";
const WRITE_EVERY_NS: u64 = 600_000_000_000;
/// A gap longer than this between two awake samples is not counted (the daemon was stopped).
const LONGEST_SAMPLE_NS: u64 = 30_000_000_000;

/// The player's state between two samples; exactly one at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Suspended (CLOCK_BOOTTIME ran on, CLOCK_MONOTONIC did not).
    Asleep,
    /// A cable is in: the charger, not the battery, carries the load.
    Charger,
    ScreenOn,
    DarkPlaying,
    DarkIdle,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Asleep => "asleep",
            State::Charger => "charger",
            State::ScreenOn => "screen_on",
            State::DarkPlaying => "dark_playing",
            State::DarkIdle => "dark_idle",
        }
    }
}

/// What one awake sample sees.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Observation {
    pub cable: bool,
    pub screen_on: bool,
    /// The kernel's current workload lease (`Idle`, `PlaybackNormal`, …).
    pub lease: Option<String>,
    pub bluetooth: bool,
    pub wifi: bool,
}

impl Observation {
    pub fn read(root: &Root) -> Self {
        let functions = root
            .read("/sys/devices/platform/18070000.connectivity/status")
            .and_then(|s| connectivity_functions(&s))
            .unwrap_or(0);
        Observation {
            cable: root
                .read("/sys/class/power_supply/y2-usb-presence/online")
                .as_deref()
                == Some("1"),
            screen_on: root
                .glob("/sys/class/backlight/*/bl_power")
                .first()
                .and_then(|p| std::fs::read_to_string(p).ok())
                .is_some_and(|s| s.trim() == "0"),
            lease: root
                .read("/sys/module/workload/parameters/leases")
                .and_then(|s| s.split_whitespace().next().map(str::to_owned)),
            bluetooth: functions & 1 != 0,
            wifi: functions & (1 << 3) != 0,
        }
    }

    pub fn state(&self) -> State {
        if self.cable {
            State::Charger
        } else if self.screen_on {
            State::ScreenOn
        } else if self
            .lease
            .as_deref()
            .is_some_and(|l| l.starts_with("Playback"))
        {
            State::DarkPlaying
        } else {
            State::DarkIdle
        }
    }
}

/// `functions=0x…` of the connectivity status line (bit 0 Bluetooth, bit 3 Wi-Fi, conn.h).
fn connectivity_functions(status: &str) -> Option<u32> {
    let value = status
        .split_whitespace()
        .find_map(|field| field.strip_prefix("functions="))?;
    match value.strip_prefix("0x") {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => value.parse().ok(),
    }
}

/// `power-profile.json`: measured currents per state (mA, keyed like `seconds`).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Profile {
    pub schema: i64,
    /// How and when the currents were measured.
    pub source: String,
    pub currents_ma: BTreeMap<String, f64>,
}

/// `energy.json`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct EnergyRecord {
    pub schema: i64,
    pub boot_id: Option<String>,
    /// Wall time of the first sample (s since the epoch).
    pub since: f64,
    pub updated: f64,
    /// Seconds per state (`State::as_str`).
    pub seconds: BTreeMap<String, f64>,
    /// Awake seconds with Bluetooth or Wi-Fi powered, and per kernel workload lease.
    pub radio_seconds: BTreeMap<String, f64>,
    pub lease_seconds: BTreeMap<String, f64>,
    /// mAh the voltage estimate's remaining charge fell per state while on the battery (a rise,
    /// e.g. a voltage recovering after load, counts negative).
    pub gauge_mah: BTreeMap<String, f64>,
    /// Seconds × the profile's current per state, when a profile exists.
    pub profile_mah: BTreeMap<String, f64>,
    pub profile_source: Option<String>,
}

/// The two clocks and the remaining charge at one sample.
#[derive(Clone, Copy, Debug)]
struct Mark {
    monotonic_ns: u64,
    boottime_ns: u64,
    remaining_uah: Option<i64>,
}

/// The daemon's accounting between samples.
pub struct Energy {
    record: EnergyRecord,
    last: Option<Mark>,
    written_ns: Option<u64>,
    saved: bool,
    profile: Option<Profile>,
}

impl Energy {
    pub fn new(root: &Root, boot_id: Option<String>) -> Self {
        let profile = root
            .json(PROFILE)
            .and_then(|v| serde_json::from_value::<Profile>(v).ok());
        Energy {
            record: EnergyRecord {
                schema: 1,
                boot_id,
                since: root.clock().wall_ns() as f64 / 1e9,
                profile_source: profile.as_ref().map(|p| p.source.clone()),
                ..EnergyRecord::default()
            },
            last: None,
            written_ns: None,
            saved: false,
            profile,
        }
    }

    /// One sample: the time and charge since the previous one go to the state seen now (the state
    /// of an awake second rarely changes mid-second) or, for the suspended part, to `asleep`.
    pub fn observe(&mut self, root: &Root, battery: &BatteryStatus, seen: &Observation) {
        let clock = root.clock();
        let mark = Mark {
            monotonic_ns: clock.monotonic_ns(),
            boottime_ns: clock.boottime_ns(),
            remaining_uah: battery
                .estimate
                .as_ref()
                .and_then(|e| e.remaining_uah_estimate),
        };
        if let Some(last) = self.last {
            self.count(&last, &mark, seen, battery.charging);
        }
        self.last = Some(mark);
    }

    fn count(&mut self, last: &Mark, now: &Mark, seen: &Observation, charging: bool) {
        let awake_ns = now.monotonic_ns.saturating_sub(last.monotonic_ns);
        let asleep_ns = now
            .boottime_ns
            .saturating_sub(last.boottime_ns)
            .saturating_sub(awake_ns);
        if awake_ns > LONGEST_SAMPLE_NS {
            return;
        }
        let state = seen.state();
        let (awake, asleep) = (awake_ns as f64 / 1e9, asleep_ns as f64 / 1e9);
        add(&mut self.record.seconds, state.as_str(), awake);
        if asleep > 0.0 {
            add(&mut self.record.seconds, State::Asleep.as_str(), asleep);
        }
        if seen.bluetooth {
            add(&mut self.record.radio_seconds, "bluetooth", awake);
        }
        if seen.wifi {
            add(&mut self.record.radio_seconds, "wifi", awake);
        }
        if let Some(lease) = &seen.lease {
            add(&mut self.record.lease_seconds, lease, awake);
        }
        if let (Some(before), Some(after), false) =
            (last.remaining_uah, now.remaining_uah, charging)
        {
            if state != State::Charger {
                // A wake splits the fall by time: what the night cost and what the second awake did.
                let fall = (before - after) as f64 / 1000.0;
                let total = awake + asleep;
                if total > 0.0 {
                    add(
                        &mut self.record.gauge_mah,
                        state.as_str(),
                        fall * awake / total,
                    );
                    add(&mut self.record.gauge_mah, "asleep", fall * asleep / total);
                }
            }
        }
        if let Some(profile) = &self.profile {
            for (name, seconds) in [(state.as_str(), awake), ("asleep", asleep)] {
                if let Some(ma) = profile.currents_ma.get(name) {
                    add(&mut self.record.profile_mah, name, ma * seconds / 3600.0);
                }
            }
        }
    }

    /// Write the record every 10 minutes, the first time 10 minutes after the daemon started (a
    /// bounded test run or a daemon restarted in a loop writes nothing). The minutes run on the
    /// clock that counts suspended time: a player asleep for days is awake only seconds of them,
    /// and an awake-time cadence never wrote that record.
    pub fn save_due(&mut self, root: &Root) -> Result<(), Failure> {
        let now = root.clock().boottime_ns();
        match self.written_ns {
            None => {
                self.written_ns = Some(now);
                Ok(())
            }
            Some(written) if now.saturating_sub(written) < WRITE_EVERY_NS => Ok(()),
            Some(_) => self.save(root),
        }
    }

    /// Write the record now; the first write of this daemon keeps another boot's record as the
    /// previous one.
    pub fn save(&mut self, root: &Root) -> Result<(), Failure> {
        if !self.saved {
            self.rotate(root);
            self.saved = true;
        }
        self.record.updated = root.clock().wall_ns() as f64 / 1e9;
        self.written_ns = Some(root.clock().boottime_ns());
        atomic_json(&root.path(RECORD), &self.record, true)
    }

    /// A record of another boot becomes the previous one; this boot's own is overwritten.
    fn rotate(&self, root: &Root) {
        let old = root
            .json(RECORD)
            .and_then(|v| serde_json::from_value::<EnergyRecord>(v).ok());
        if old.is_some_and(|o| o.boot_id != self.record.boot_id) {
            let _ = std::fs::rename(root.path(RECORD), root.path(PREVIOUS));
        }
    }
}

fn add(map: &mut BTreeMap<String, f64>, key: &str, value: f64) {
    *map.entry(key.to_owned()).or_insert(0.0) += value;
}

#[cfg(test)]
#[path = "energy_tests.rs"]
mod energy_tests;
