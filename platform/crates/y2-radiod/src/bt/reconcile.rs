//! Each pass: look again when something changed, notice what started and ended, then move one step
//! toward what the owner wants: the adapter's power, a sleep's power-down, the headset to keep
//! connected and every connected headset's stream. Stuck steps go to the recovery ladder.
// SPDX-License-Identifier: GPL-2.0-only
use super::{bus::Kind, ladder::Rung, links, world, Controller, Sleep};
use std::time::{Duration, Instant};
use y2p_contract::radiod::{Ended, Reply};

const POWER_ON_DEADLINE: Duration = Duration::from_secs(100);
const POWER_OFF_DEADLINE: Duration = Duration::from_secs(15);

impl Controller {
    /// Read the world again when a signal said something changed (and once a minute regardless).
    pub(super) fn observe(&mut self, now: Instant) {
        if !self.dirty && now < self.observed_at + Duration::from_secs(60) {
            return;
        }
        self.dirty = false;
        self.observed_at = now;
        let Some(bus) = &self.bus else {
            return;
        };
        match world::observe(&bus.channel, &mut self.generations) {
            Ok(mut new) => {
                self.observe_misses = 0;
                self.playing_only_with_a_lease(&mut new);
                let old = std::mem::replace(&mut self.world, new);
                self.transitions(&old, now);
            }
            Err(error) => {
                self.observe_misses += 1;
                self.journal.event(
                    "observe_missed",
                    serde_json::json!({"error": error, "misses": self.observe_misses}),
                );
                if self.observe_misses >= 3 {
                    self.observe_misses = 0;
                    self.recover(Rung::BluezRestart, None);
                }
            }
        }
    }

    /// BlueALSA reports a stream Running until its keep-alive after the player closed it has passed
    /// (`KEEP_ALIVE_S`, supervise.rs): only a stream the player holds open (its `Stream` lease) counts as
    /// playing, so a pause allows codec changes, new codec settings, rate learning and Wi-Fi's power
    /// save at once.
    fn playing_only_with_a_lease(&self, world: &mut world::World) {
        let devices = world.devices.clone();
        for pcm in world.pcms.iter_mut().filter(|p| p.running == Some(true)) {
            let held = devices
                .values()
                .find(|d| d.path == pcm.device)
                .is_some_and(|d| self.streams.contains_key(&d.address));
            if !held {
                pcm.running = Some(false);
            }
        }
    }

    /// What started and ended between two looks.
    fn transitions(&mut self, old: &world::World, now: Instant) {
        if self.world.powered() && !old.powered() && matches!(self.sleep, Sleep::Awake) {
            self.journal.event("adapter_on", serde_json::json!({}));
            // The owner's own tap keeps its episode (it connects without waiting for adoption).
            let tapped = self.asked.as_ref().is_some_and(|a| {
                self.records
                    .get(a)
                    .and_then(|r| r.episode.as_ref())
                    .is_some_and(|e| e.trigger == links::Trigger::User)
            });
            let alone = self.asleep_alone.take() == Some(true);
            if !tapped {
                self.trigger_preferred(match alone {
                    true => links::Trigger::Wake,
                    false => links::Trigger::PowerOn,
                });
            }
        }
        let devices: Vec<world::Dev> = self.world.devices.values().cloned().collect();
        for dev in devices {
            let was = old.devices.get(&dev.address).is_some_and(|d| d.connected);
            if dev.connected && !was {
                self.connected(&dev, now);
            } else if !dev.connected && was {
                self.disconnected(&dev);
            }
        }
        let gone: Vec<String> = self
            .records
            .keys()
            .filter(|address| !self.world.devices.contains_key(*address))
            .cloned()
            .collect();
        for address in gone {
            self.records.remove(&address);
            self.codec.forget(&address);
        }
    }

    fn connected(&mut self, dev: &world::Dev, now: Instant) {
        let record = self.record(&dev.address);
        record.connected_since = Some(now);
        record.connecting = None;
        record.in_progress = false;
        record.episode = None;
        record.parked = false;
        record.ended = None;
        self.journal
            .event("peer_connected", serde_json::json!({"audio": dev.audio}));
        // A trusted headset that connects becomes the one to reconnect (as y2-bt-reconnect did).
        if dev.reconnectable() && self.desired.preferred.as_deref() != Some(dev.address.as_str()) {
            self.desired.preferred = Some(dev.address.clone());
            self.desired.save_preferred(&self.root);
        }
    }

    fn disconnected(&mut self, dev: &world::Dev) {
        let quiescing = !matches!(self.sleep, Sleep::Awake);
        let reported = self.reasons.remove(&dev.path).unwrap_or(if quiescing {
            Ended::Sleep
        } else {
            Ended::Unknown
        });
        let record = self.record(&dev.address);
        // The disconnect for a wide stream is ours, though BlueZ named "Remote" for it (measured):
        // the headset gets the tries and the short wait of a local end, not one look after 4 s.
        let ended = if std::mem::take(&mut record.stereo_reconnect) && !quiescing {
            Ended::Local
        } else {
            reported
        };
        let lasted = record.connected_since.map(|since| since.elapsed());
        record.quick_drops = if links::quick_drop(lasted, ended) {
            record.quick_drops + 1
        } else {
            0
        };
        let drops = record.quick_drops;
        record.ended = Some(ended);
        record.connected_since = None;
        record.pcm_missing_since = None;
        record.profile_asked = false;
        self.codec.ended(&dev.address);
        self.streams.remove(&dev.address);
        if self.target().as_deref() != Some(dev.address.as_str()) || !self.world.powered() {
            return;
        }
        if drops < links::QUICK_DROPS {
            self.trigger(&dev.address, links::Trigger::Lost(ended));
            return;
        }
        let record = self.record(&dev.address);
        record.parked = true;
        record.episode = None;
        self.journal.event(
            "reconnect_given_up",
            serde_json::json!({"address": dev.address, "quick_drops": drops}),
        );
        let message = format!(
            "Couldn't stay connected to {}. Remove them in Bluetooth and pair them again.",
            dev.name
        );
        self.fail("connect_failed", &message, Some(&dev.address));
    }

    /// Deadlines: calls, the pairing question, the scan, connects that stopped moving.
    pub(super) fn expire(&mut self, now: Instant) {
        self.expire_calls(now);
        self.expire_question(now);
        if self.discovery.as_ref().is_some_and(|d| d.until <= now) {
            self.stop_discovery(true);
        }
        for (address, record) in &mut self.records {
            let unset = self
                .world
                .devices
                .get(address)
                .is_some_and(|d| d.connected && d.audio && !self.world.set_up(d));
            record.unset_since = unset.then(|| record.unset_since.unwrap_or(now));
        }
        let stuck: Vec<String> = self
            .records
            .iter()
            .filter(|(address, record)| {
                let dev = self.world.devices.get(*address);
                let waiting = record.in_progress
                    && record
                        .connecting
                        .is_some_and(|at| now.duration_since(at) >= links::STUCK)
                    && !dev.is_some_and(|d| d.connected);
                let changing = self
                    .in_flight(|k| matches!(k, Kind::Select { address: a, .. } if a == *address));
                let half = !changing
                    && record
                        .unset_since
                        .is_some_and(|at| now.duration_since(at) >= links::HALF_CONNECTED);
                waiting || half
            })
            .map(|(address, _)| address.clone())
            .collect();
        for address in stuck {
            self.cancel_connect(&address);
        }
    }

    pub(super) fn reconcile(&mut self, now: Instant) {
        if self.world.bluez.is_some() {
            self.register_agent();
        }
        self.reconcile_power(now);
        self.reconcile_sleep(now);
        self.reconcile_codec_reload();
        if !self.world.powered() || !matches!(self.sleep, Sleep::Awake) {
            return;
        }
        self.reconcile_discovery();
        self.adopt_welcomed();
        self.reconcile_target(now);
        let ready: Vec<String> = self
            .world
            .devices
            .values()
            .filter(|d| d.connected && d.audio && self.world.set_up(d))
            .map(|d| d.address.clone())
            .collect();
        for address in ready {
            self.settle(&address, now);
        }
    }

    fn reconcile_power(&mut self, now: Instant) {
        let (Some(owner), Some(adapter)) = (self.world.bluez.clone(), self.world.adapter.clone())
        else {
            return;
        };
        if let Some(at) = self.power.cycle {
            if !adapter.powered || now.duration_since(at) > Duration::from_secs(20) {
                self.power.cycle = None;
            }
        }
        let want = self.desired.bluetooth
            && matches!(self.sleep, Sleep::Awake)
            && self.power.cycle.is_none();
        if adapter.powered == want {
            self.power.failures = 0;
            return;
        }
        if self.power.in_flight || now < self.power.next {
            return;
        }
        if want {
            crate::wifi::recover_if_failed(&self.root);
        }
        let Some(bus) = self.bus.as_mut() else {
            return;
        };
        let target = (
            owner.as_str(),
            adapter.path.as_str(),
            "org.bluez.Adapter1",
            "Powered",
        );
        let within = if want {
            POWER_ON_DEADLINE
        } else {
            POWER_OFF_DEADLINE
        };
        if bus.set(target, want, Kind::Power(want), within) {
            self.power.in_flight = true;
            self.journal
                .event("power_request", serde_json::json!({"on": want}));
        }
    }

    /// A scan asked for while the adapter was off starts once it is on; the player is visible to
    /// other devices from then on (`show_to_others`).
    fn reconcile_discovery(&mut self) {
        let start = self.discovery.as_ref().is_some_and(|d| !d.started);
        if start && !self.in_flight(|k| matches!(k, Kind::Discovery(_))) {
            if let Some(discovery) = self.discovery.as_mut() {
                discovery.started = true;
            }
            self.discovery_call(true);
            self.show_to_others();
        }
    }

    /// New codec settings: BlueALSA starts again with them when nothing plays (its restart ends the
    /// link; the headphones come back as after any local end).
    fn reconcile_codec_reload(&mut self) {
        let playing =
            !self.streams.is_empty() || self.world.pcms.iter().any(|pcm| pcm.running == Some(true));
        if !self.reload_codecs || playing {
            return;
        }
        self.reload_codecs = false;
        self.journal
            .event("codec_settings_reload", serde_json::json!({}));
        // A BlueALSA started under a connected headset has no stream for it, and the link then hung
        // half set up for 30 s (owner's player, 9.3.3). End the links cleanly first, restart, and
        // connect them again straight away (episode Lost(Local): 2 s for BlueALSA's endpoints).
        let connected: Vec<String> = self
            .world
            .connected_audio()
            .map(|d| d.address.clone())
            .collect();
        for address in &connected {
            self.disconnect(address, Kind::Cancel(address.clone()));
        }
        self.supervise.restart_bluealsa();
        for address in connected {
            self.reasons.insert(
                self.world
                    .devices
                    .get(&address)
                    .map(|d| d.path.clone())
                    .unwrap_or_default(),
                Ended::Local,
            );
        }
        self.dirty = true;
    }

    /// A sleep waits for the adapter to be off (or for its 15 s).
    fn reconcile_sleep(&mut self, now: Instant) {
        let Sleep::Going { until, .. } = &self.sleep else {
            return;
        };
        let down = !self.world.powered() || self.world.bluez.is_none();
        let late = now >= *until;
        if !down && !late {
            return;
        }
        let Sleep::Going { clients, .. } = std::mem::replace(&mut self.sleep, Sleep::Down) else {
            return;
        };
        let reply = if down {
            Reply::done()
        } else {
            Reply::refused("bluetooth_power_down_timeout")
        };
        self.journal
            .event("quiesced", serde_json::json!({"down": down}));
        for client in clients {
            client.reply(reply.clone());
        }
    }

    /// Connect the headset to keep connected when its episode says so.
    fn reconcile_target(&mut self, now: Instant) {
        let Some(target) = self.target() else {
            return;
        };
        let Some(dev) = self.world.devices.get(&target).cloned() else {
            return;
        };
        if dev.connected || (self.asked.is_none() && !dev.reconnectable()) {
            return;
        }
        let others: Vec<world::Dev> = self
            .world
            .connected_audio()
            .filter(|d| d.address != target)
            .cloned()
            .collect();
        if !others.is_empty() {
            // Never displace another headset on its own; the owner's tap does.
            if self.asked.as_deref() != Some(target.as_str()) {
                return;
            }
            for other in others {
                self.disconnect(&other.address, Kind::Disconnect(other.address.clone()));
            }
            return;
        }
        if self.in_flight(|k| matches!(k, Kind::Connect(a) | Kind::Cancel(a) if *a == target)) {
            return;
        }
        let record = self.record(&target);
        let Some(episode) = record.episode.as_mut() else {
            return;
        };
        if record.connecting.is_some() || !episode.due(now) {
            return;
        }
        episode.begin();
        let attempt = episode.attempts;
        record.connecting = Some(now);
        record.in_progress = false;
        let Some(owner) = self.world.bluez.clone() else {
            return;
        };
        if let Some(bus) = self.bus.as_mut() {
            let call = (
                owner.as_str(),
                dev.path.as_str(),
                "org.bluez.Device1",
                "ConnectProfile",
            );
            let kind = Kind::Connect(target.clone());
            bus.call(call, (world::SINK,), kind, links::CONNECT_DEADLINE);
        }
        self.journal.event(
            "connect_attempt",
            serde_json::json!({"address": target, "attempt": attempt}),
        );
    }

    /// A connect stopped moving: cancel it (a Disconnect ends BlueZ's page), count the try; a
    /// second cancel in one episode switches the adapter off and on instead.
    pub(super) fn cancel_connect(&mut self, address: &str) {
        let record = self.record(address);
        record.connecting = None;
        record.in_progress = false;
        record.connected_since = None;
        record.cancels += 1;
        let wanted = if record.cancels >= 2 {
            Rung::PowerCycle
        } else {
            Rung::CancelPage
        };
        let now = Instant::now();
        let more = record
            .episode
            .as_mut()
            .is_some_and(|episode| episode.failed(now));
        if !more {
            record.parked = true;
            record.episode = None;
        }
        self.recover(wanted, Some(address));
    }

    /// Ask BlueZ to end a link.
    pub(super) fn disconnect(&mut self, address: &str, kind: Kind) {
        let (Some(owner), Some(path)) = (
            self.world.bluez.clone(),
            self.world.devices.get(address).map(|d| d.path.clone()),
        ) else {
            return;
        };
        if let Some(bus) = self.bus.as_mut() {
            let target = (
                owner.as_str(),
                path.as_str(),
                "org.bluez.Device1",
                "Disconnect",
            );
            bus.call(target, (), kind, Duration::from_secs(10));
        }
    }

    /// Take a recovery step for a problem that calls for `wanted` first; which one ran.
    pub(super) fn recover(&mut self, wanted: Rung, address: Option<&str>) -> Option<Rung> {
        let now = Instant::now();
        let Some(rung) = self.ladder.choose(wanted, now) else {
            self.fail(
                "bluetooth_recovery_exhausted",
                "Bluetooth stopped working. Turn Bluetooth off and on in Settings.",
                address,
            );
            return None;
        };
        self.journal
            .event("recovery", serde_json::json!({"step": rung.name()}));
        match rung {
            Rung::CancelPage => {
                if let Some(address) = address {
                    self.disconnect(address, Kind::Cancel(address.to_owned()));
                }
            }
            Rung::PowerCycle => self.power.cycle = Some(now),
            Rung::BluealsaRestart => self.supervise.restart_bluealsa(),
            Rung::BluezRestart => {
                if let Some(bus) = self.bus.as_mut() {
                    bus.agent_owner = None;
                }
                self.supervise.restart_bluez();
            }
        }
        self.dirty = true;
        Some(rung)
    }
}
