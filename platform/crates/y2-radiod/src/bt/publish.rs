//! The controller's state as clients see it (`y2p_contract::radiod::Bluetooth`), sent to the
//! arbiter whenever it changed.
// SPDX-License-Identifier: GPL-2.0-only
use super::{bus::Kind, world, Controller, Sleep};
use crate::arbiter::Event;
use std::time::Instant;
use y2p_contract::radiod::{Bluetooth, Device, Link, Pcm, Service};

impl Controller {
    pub(super) fn publish(&mut self) {
        let state = self.state(Instant::now());
        if self.published.as_ref() == Some(&state) {
            return;
        }
        self.sequence += 1;
        self.published = Some(state.clone());
        let _ = self.events.send(Event::Bluetooth(Box::new(state)));
    }

    fn state(&self, now: Instant) -> Bluetooth {
        let mut devices: Vec<Device> = self
            .world
            .devices
            .values()
            .map(|dev| self.device(dev))
            .collect();
        devices.sort_by(|a, b| a.name.cmp(&b.name).then(a.address.cmp(&b.address)));
        devices.truncate(128);
        let codec = self
            .world
            .connected_audio()
            .find_map(|dev| self.codec.shown(&dev.address, &dev.path));
        Bluetooth {
            service: self.service(now),
            wanted: self.desired.bluetooth,
            available: self.world.adapter.is_some(),
            powered: self.world.powered(),
            discovering: self.world.adapter.as_ref().is_some_and(|a| a.discovering),
            bluealsa: self.world.bluealsa.is_some(),
            devices,
            pcms: self.world.pcms.iter().map(pcm).collect(),
            pairing: self.pairing.shown(),
            codec,
            preference: self.desired.policy.preference.clone(),
            name: self.world.adapter.as_ref().map(|a| a.alias.clone()),
            scan: self.scan.clone(),
            recovery: self.ladder.current(now).map(str::to_owned),
            failure: self.failure.clone(),
        }
    }

    fn service(&self, now: Instant) -> Service {
        if !matches!(self.sleep, Sleep::Awake) {
            Service::Quiesced
        } else if self.ladder.failed || self.supervise.exhausted.is_some() {
            Service::Failed
        } else if self.ladder.current(now).is_some() {
            Service::Recovering
        } else if self.world.bluez.is_none() || self.world.adapter.is_none() {
            Service::Starting
        } else {
            Service::Ready
        }
    }

    fn device(&self, dev: &world::Dev) -> Device {
        let record = self.records.get(&dev.address);
        Device {
            path: dev.path.clone(),
            address: dev.address.clone(),
            name: dev.name.clone(),
            paired: dev.paired,
            bonded: dev.bonded,
            trusted: dev.trusted,
            connected: dev.connected,
            audio: dev.audio,
            link: self.link(dev),
            auto_connect: self.desired.auto_connect(&dev.address),
            volume: dev.volume,
            transport: dev.transport.clone(),
            ended: record.and_then(|r| r.ended),
        }
    }

    fn link(&self, dev: &world::Dev) -> Link {
        if self.in_flight(|k| matches!(k, Kind::Pair(a) if *a == dev.address)) {
            return Link::Pairing;
        }
        if dev.connected {
            let ready = !dev.audio
                || (self.world.pcm_for(&dev.address).is_some()
                    && !self.codec.selecting(&dev.address));
            return if ready { Link::Ready } else { Link::Settling };
        }
        let Some(record) = self.records.get(&dev.address) else {
            return Link::Idle;
        };
        if record.connecting.is_some() {
            Link::Connecting
        } else if record.episode.is_some() {
            Link::Waiting
        } else if record.parked {
            Link::Parked
        } else {
            Link::Idle
        }
    }
}

fn pcm(observed: &world::PcmObs) -> Pcm {
    Pcm {
        object: observed.object.clone(),
        device: observed.device.clone(),
        transport: observed.transport.clone(),
        mode: observed.mode.clone(),
        codec: observed.codec.clone(),
        format: observed.format,
        rate: observed.rate,
        channels: observed.channels,
        running: observed.running,
        generation: observed.generation,
    }
}
