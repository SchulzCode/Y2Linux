//! What clients ask of Bluetooth. A request changes what the controller wants and is answered at
//! once (except a sleep's power-down, answered when it is done); the passes that follow carry it
//! out and the published state shows how it went. Nothing is refused as busy.
// SPDX-License-Identifier: GPL-2.0-only
use super::{bus::Kind, links::Trigger, Controller, Discovery, Sleep};
use crate::server::Client;
use std::time::{Duration, Instant};
use y2p_contract::radiod::{Ended, Radio, Reply, Request, Scan, NAME_MAX_BYTES};

const SCAN_FOR: Duration = Duration::from_secs(15);
const PAIR_DEADLINE: Duration = Duration::from_secs(70);
/// How long a pairing this player starts keeps the adapter Pairable.
const PAIRABLE_FOR: Duration = Duration::from_secs(90);

impl Controller {
    pub(super) fn request(&mut self, request: Request, client: Client) {
        self.journal
            .event("request", serde_json::json!({"op": operation(&request)}));
        self.dirty = true;
        let reply = match request {
            Request::BluetoothPower { on, persist } => self.set_power(on, persist),
            Request::Scan { on } => self.scan(on),
            Request::Pair { address } => self.pair(&address),
            Request::Connect { address } => self.connect(&address),
            Request::Disconnect { address } => self.user_disconnect(&address),
            Request::Forget { address } => self.forget(&address),
            Request::Confirm { accept } => match self.confirm(accept) {
                true => Reply::done(),
                false => Reply::refused("no_pairing_question"),
            },
            Request::CodecPreference { preference } => self.preference(&preference),
            Request::DeviceName { name } => self.rename(&name),
            Request::Volume { address, volume } => self.volume(&address, volume),
            Request::AutoConnect { address, on } => self.auto_connect(&address, on),
            Request::Stream { address, open } => self.stream(address, open),
            Request::RestartBluetooth => self.restart(),
            Request::ReloadCodecs => {
                self.reload_codecs = true;
                Reply::done()
            }
            Request::Quiesce {
                radio: Radio::Bluetooth,
            } => return self.quiesce(client),
            Request::Restore {
                radio: Radio::Bluetooth,
            } => {
                // A wake before the power-down finished: whoever waits for it hears it ended,
                // and no wake follows that the sleep's headset state could speak for.
                if let Sleep::Going { clients, .. } = std::mem::take(&mut self.sleep) {
                    self.asleep_alone = None;
                    for waiting in clients {
                        waiting.reply(Reply::refused("woken_before_power_down"));
                    }
                }
                Reply::done()
            }
            _ => Reply::refused("not_a_bluetooth_request"),
        };
        client.reply(reply);
    }

    fn set_power(&mut self, on: bool, persist: bool) -> Reply {
        // The owner's switch is a power-on of its own (three tries), not a wake.
        self.asleep_alone = None;
        self.desired.bluetooth = on;
        if persist {
            self.desired.save_bluetooth(&self.root);
        }
        if on {
            // The owner switched it on: whatever they disconnected may connect again, and every
            // recovery budget starts over. Off and on is also the owner's restart: when Bluetooth
            // had failed (recovery spent, a daemon given up, BlueZ or the adapter gone), BlueALSA
            // and BlueZ start again too.
            self.held.clear();
            let failed = self.ladder.failed
                || self.supervise.exhausted.is_some()
                || self.world.bluez.is_none()
                || self.world.adapter.is_none();
            if failed {
                self.restart();
            } else {
                self.ladder.reset();
                self.supervise.reset();
                self.failure = None;
            }
        } else {
            self.asked = None;
            self.stop_discovery(false);
            self.close_pairing_window();
        }
        if on {
            // The owner is using the player: whatever a sleep left is over.
            self.sleep = Sleep::Awake;
        }
        self.power.next = Instant::now();
        Reply::done()
    }

    /// The player opens or closes its stream to a headset: no codec change while one is open. An open
    /// while a codec change is still under way is refused (the headset is still being set up).
    fn stream(&mut self, address: String, open: bool) -> Reply {
        if open {
            let selecting =
                self.in_flight(|k| matches!(k, Kind::Select { address: a, .. } if *a == address));
            if selecting {
                return Reply::refused("codec_change_in_progress");
            }
            *self.streams.entry(address).or_insert(0) += 1;
        } else if let Some(count) = self.streams.get_mut(&address) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.streams.remove(&address);
            }
        }
        Reply::done()
    }

    fn scan(&mut self, on: bool) -> Reply {
        if !on {
            self.stop_discovery(false);
            return Reply::done();
        }
        if !self.desired.bluetooth {
            self.set_power(true, true);
        }
        self.discovery = Some(Discovery {
            until: Instant::now() + SCAN_FOR,
            seen: self.world.devices.keys().cloned().collect(),
            started: false,
        });
        self.scan = Scan::Scanning;
        Reply::done()
    }

    pub(super) fn discovery_call(&mut self, on: bool) {
        let (Some(owner), Some(adapter)) = (
            self.world.bluez.clone(),
            self.world.adapter.as_ref().map(|a| a.path.clone()),
        ) else {
            return;
        };
        if let Some(bus) = self.bus.as_mut() {
            let method = if on {
                "StartDiscovery"
            } else {
                "StopDiscovery"
            };
            let target = (
                owner.as_str(),
                adapter.as_str(),
                "org.bluez.Adapter1",
                method,
            );
            bus.call(target, (), Kind::Discovery(on), SCAN_FOR);
        }
    }

    /// End a scan: `complete` counts what it found that was not known before.
    pub(super) fn stop_discovery(&mut self, complete: bool) {
        let Some(discovery) = self.discovery.take() else {
            return;
        };
        if complete {
            let found = self
                .world
                .devices
                .values()
                .filter(|d| !d.paired && d.audio && !discovery.seen.contains(&d.address))
                .count();
            self.scan = Scan::Complete { found };
        } else {
            self.scan = Scan::Idle;
        }
        if self.world.adapter.as_ref().is_some_and(|a| a.discovering) {
            self.discovery_call(false);
        }
    }

    fn pair(&mut self, address: &str) -> Reply {
        let Some(path) = self.world.devices.get(address).map(|d| d.path.clone()) else {
            return Reply::refused("device_not_found");
        };
        let Some(owner) = self.world.bluez.clone() else {
            return Reply::refused("bluetooth_not_ready");
        };
        self.stop_discovery(false);
        if let Err(error) = self.open_pairing_window(PAIRABLE_FOR) {
            self.fail("pair_failed", &error, Some(address));
            return Reply::refused("pairing_window");
        }
        self.held.remove(address);
        if let Some(bus) = self.bus.as_mut() {
            let target = (owner.as_str(), path.as_str(), "org.bluez.Device1", "Pair");
            bus.call(target, (), Kind::Pair(address.to_owned()), PAIR_DEADLINE);
        }
        Reply::done()
    }

    fn connect(&mut self, address: &str) -> Reply {
        if !self.world.devices.contains_key(address) {
            return Reply::refused("device_not_found");
        }
        if !self.desired.bluetooth {
            self.set_power(true, true);
        }
        self.held.remove(address);
        self.asked = Some(address.to_owned());
        self.failure = None;
        self.trigger(address, Trigger::User);
        Reply::done()
    }

    fn user_disconnect(&mut self, address: &str) -> Reply {
        self.held.insert(address.to_owned());
        if self.asked.as_deref() == Some(address) {
            self.asked = None;
        }
        let record = self.record(address);
        record.episode = None;
        record.ended = Some(Ended::User);
        self.reasons.insert(
            self.world
                .devices
                .get(address)
                .map(|d| d.path.clone())
                .unwrap_or_default(),
            Ended::User,
        );
        self.disconnect(address, Kind::Disconnect(address.to_owned()));
        Reply::done()
    }

    fn forget(&mut self, address: &str) -> Reply {
        let (Some(owner), Some(adapter), Some(path)) = (
            self.world.bluez.clone(),
            self.world.adapter.as_ref().map(|a| a.path.clone()),
            self.world.devices.get(address).map(|d| d.path.clone()),
        ) else {
            return Reply::refused("device_not_found");
        };
        self.held.remove(address);
        if self.asked.as_deref() == Some(address) {
            self.asked = None;
        }
        if self.desired.preferred.as_deref() == Some(address) {
            self.desired.preferred = None;
            self.desired.save_preferred(&self.root);
        }
        if self.desired.policy.manual.remove(address) {
            self.desired.save_policy(&self.root);
        }
        self.records.remove(address);
        let Ok(path) = dbus::Path::new(path) else {
            return Reply::refused("device_not_found");
        };
        if let Some(bus) = self.bus.as_mut() {
            let target = (
                owner.as_str(),
                adapter.as_str(),
                "org.bluez.Adapter1",
                "RemoveDevice",
            );
            let kind = Kind::Remove(address.to_owned());
            bus.call(target, (path,), kind, Duration::from_secs(10));
        }
        Reply::done()
    }

    fn preference(&mut self, preference: &str) -> Reply {
        if !crate::desired::PREFERENCES.contains(&preference) {
            return Reply::refused("unknown_codec_preference");
        }
        if self.desired.policy.preference != preference {
            self.desired.policy.preference = preference.to_owned();
            self.desired.save_policy(&self.root);
        }
        Reply::done()
    }

    /// The owner's name for the player: BlueZ keeps it (`/data/bluetooth/bluez`), so it outlives
    /// this daemon and a restore of the system; the next published state carries it.
    fn rename(&mut self, name: &str) -> Reply {
        let name = match device_name(name) {
            Ok(name) => name,
            Err(reason) => return Reply::refused(reason),
        };
        let (Some(owner), Some(adapter), Some(bus)) = (
            self.world.bluez.clone(),
            self.world.adapter.as_ref().map(|a| a.path.clone()),
            self.bus.as_ref(),
        ) else {
            return Reply::refused("bluetooth_not_ready");
        };
        match bus.set_now(
            (&owner, &adapter, "org.bluez.Adapter1", "Alias"),
            name.to_owned(),
        ) {
            Ok(()) => Reply::done(),
            Err(error) => {
                self.journal
                    .event("rename_failed", serde_json::json!({"error": error}));
                Reply::refused("device_name_not_set")
            }
        }
    }

    fn volume(&mut self, address: &str, volume: u8) -> Reply {
        let (Some(owner), Some(transport)) = (
            self.world.bluez.clone(),
            self.world
                .devices
                .get(address)
                .and_then(|d| d.transport.clone()),
        ) else {
            return Reply::refused("no_absolute_volume");
        };
        if let Some(bus) = self.bus.as_mut() {
            let target = (
                owner.as_str(),
                transport.as_str(),
                "org.bluez.MediaTransport1",
                "Volume",
            );
            bus.set(
                target,
                volume.min(127) as u16,
                Kind::Volume,
                Duration::from_secs(2),
            );
        }
        Reply::done()
    }

    fn auto_connect(&mut self, address: &str, on: bool) -> Reply {
        let changed = if on {
            self.desired.policy.manual.remove(address)
        } else {
            self.desired.policy.manual.insert(address.to_owned())
        };
        if changed {
            self.desired.save_policy(&self.root);
        }
        Reply::done()
    }

    /// The owner's restart (Bluetooth off and on after a failure, or `RestartBluetooth`): every
    /// budget starts over, BlueALSA and BlueZ start again.
    fn restart(&mut self) -> Reply {
        self.ladder.reset();
        self.supervise.reset();
        self.failure = None;
        crate::wifi::recover_if_failed(&self.root);
        if let Some(bus) = self.bus.as_mut() {
            bus.agent_owner = None;
        }
        self.supervise.restart_bluealsa();
        self.supervise.restart_bluez();
        self.journal
            .event("restart_by_owner", serde_json::json!({}));
        Reply::done()
    }

    /// A sleep: stop scanning, switch the adapter off, answer when it is off.
    fn quiesce(&mut self, client: Client) {
        if matches!(self.sleep, Sleep::Awake) {
            self.asleep_alone = Some(self.world.connected_audio().next().is_none());
        }
        self.stop_discovery(false);
        self.confirm(false);
        self.close_pairing_window();
        if !self.desired.bluetooth && !self.world.powered() {
            // Off already, and a wake restores only radios that are on: nothing to take down
            // and nothing to come back from.
            client.reply(Reply::done());
            return;
        }
        match &mut self.sleep {
            Sleep::Going { clients, .. } => clients.push(client),
            Sleep::Down if !self.world.powered() => client.reply(Reply::done()),
            _ => {
                self.sleep = Sleep::Going {
                    clients: vec![client],
                    until: Instant::now() + Duration::from_secs(15),
                };
                self.power.next = Instant::now();
            }
        }
    }
}

/// The name the owner asked for, trimmed (empty: main.conf's), or why it cannot be one.
fn device_name(name: &str) -> Result<&str, &'static str> {
    let name = name.trim();
    if name.len() > NAME_MAX_BYTES {
        Err("device_name_too_long")
    } else if name.chars().any(char::is_control) {
        Err("device_name_invalid")
    } else {
        Ok(name)
    }
}

/// A request's name for the journal (never its addresses).
fn operation(request: &Request) -> &'static str {
    match request {
        Request::Subscribe => "subscribe",
        Request::State => "state",
        Request::BluetoothPower { on: true, .. } => "bluetooth_on",
        Request::BluetoothPower { on: false, .. } => "bluetooth_off",
        Request::Scan { .. } => "scan",
        Request::Pair { .. } => "pair",
        Request::Connect { .. } => "connect",
        Request::Disconnect { .. } => "disconnect",
        Request::Forget { .. } => "forget",
        Request::Confirm { .. } => "confirm",
        Request::CodecPreference { .. } => "codec_preference",
        Request::DeviceName { .. } => "device_name",
        Request::Volume { .. } => "volume",
        Request::AutoConnect { .. } => "auto_connect",
        Request::Stream { open: true, .. } => "stream_open",
        Request::Stream { open: false, .. } => "stream_closed",
        Request::RestartBluetooth => "restart",
        Request::ReloadCodecs => "reload_codecs",
        Request::WifiPower { .. } => "wifi_power",
        Request::WifiRestart => "wifi_restart",
        Request::Quiesce { .. } => "quiesce",
        Request::Restore { .. } => "restore",
    }
}
