//! What BlueZ and BlueALSA show, read whole from their object managers after any relevant signal:
//! the adapter, the devices, their A2DP transports and BlueALSA's PCMs, each PCM with a generation
//! that changes whenever its stream changes.
// SPDX-License-Identifier: GPL-2.0-only
use dbus::{
    arg::PropMap,
    blocking::{
        stdintf::org_freedesktop_dbus::{ObjectManager, Properties},
        Proxy,
    },
    channel::Channel,
};
use std::{collections::BTreeMap, collections::HashMap, time::Duration};

/// The A2DP sink UUID (headphones and speakers).
pub const SINK: &str = "0000110b-0000-1000-8000-00805f9b34fb";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Adapter {
    pub path: String,
    pub powered: bool,
    pub discovering: bool,
    /// The name other devices list the player by (BlueZ's Alias: the owner's or main.conf's).
    pub alias: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dev {
    pub path: String,
    pub address: String,
    pub name: String,
    pub paired: bool,
    pub bonded: bool,
    pub trusted: bool,
    pub blocked: bool,
    pub connected: bool,
    pub resolved: bool,
    pub audio: bool,
    pub transport: Option<String>,
    pub volume: Option<u8>,
}

impl Dev {
    /// A durable, trusted audio bond: the only kind the daemon reconnects on its own.
    pub fn reconnectable(&self) -> bool {
        self.audio && self.paired && self.bonded && self.trusted && !self.blocked
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PcmObs {
    pub object: String,
    pub device: String,
    pub transport: String,
    pub mode: String,
    pub codec: Option<String>,
    pub format: Option<u16>,
    pub rate: Option<u32>,
    pub channels: Option<u8>,
    pub running: Option<bool>,
    pub sequence: Option<u64>,
    pub generation: u64,
}

impl PcmObs {
    /// The stream a player opens for this device (A2DP source, the PCM's sink side).
    pub fn playback_for(&self, device: &str) -> bool {
        self.device == device && self.mode == "sink" && self.transport == "A2DP-source"
    }

    /// What makes a stream the same stream (running state aside).
    fn identity(
        &self,
    ) -> (
        String,
        String,
        String,
        Option<String>,
        Option<u16>,
        Option<u32>,
    ) {
        (
            self.device.clone(),
            self.transport.clone(),
            self.mode.clone(),
            self.codec.clone(),
            self.format,
            self.rate,
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct World {
    pub bluez: Option<String>,
    pub bluealsa: Option<String>,
    pub adapter: Option<Adapter>,
    /// By address.
    pub devices: BTreeMap<String, Dev>,
    pub pcms: Vec<PcmObs>,
}

impl World {
    pub fn pcm_for(&self, address: &str) -> Option<&PcmObs> {
        let device = self.devices.get(address)?;
        self.pcms.iter().find(|pcm| pcm.playback_for(&device.path))
    }

    /// A2DP is set up: BlueZ resolved the services, or BlueALSA has the stream. BlueZ looks up no
    /// services for a known headset that connects by itself, so its ServicesResolved stays false.
    pub fn set_up(&self, dev: &Dev) -> bool {
        dev.resolved || self.pcm_for(&dev.address).is_some()
    }

    pub fn powered(&self) -> bool {
        self.adapter.as_ref().is_some_and(|a| a.powered)
    }

    /// Connected audio devices.
    pub fn connected_audio(&self) -> impl Iterator<Item = &Dev> {
        self.devices.values().filter(|d| d.connected && d.audio)
    }
}

/// Keeps each PCM's generation while its stream stays the same; a new BlueALSA owner, a reconnect
/// (BlueALSA's connection sequence) or a changed codec/rate/format makes a new one. Generations
/// only grow within a daemon run and start from the boot clock, so a restarted daemon never repeats
/// one.
pub struct Generations {
    next: u64,
    owner: Option<String>,
    known: HashMap<String, (PcmObs, u64)>,
}

impl Generations {
    pub fn new(seed: u64) -> Generations {
        Generations {
            next: seed.max(1),
            owner: None,
            known: HashMap::new(),
        }
    }

    pub fn assign(&mut self, owner: &Option<String>, pcms: &mut [PcmObs]) {
        if &self.owner != owner {
            self.owner = owner.clone();
            self.known.clear();
        }
        self.known
            .retain(|object, _| pcms.iter().any(|pcm| &pcm.object == object));
        for pcm in pcms.iter_mut() {
            let same = self.known.get(&pcm.object).is_some_and(|(old, _)| {
                old.identity() == pcm.identity() && old.sequence == pcm.sequence
            });
            if same {
                pcm.generation = self.known[&pcm.object].1;
            } else {
                self.next += 1;
                pcm.generation = self.next;
                self.known
                    .insert(pcm.object.clone(), (pcm.clone(), self.next));
            }
        }
    }
}

type Objects = HashMap<dbus::Path<'static>, HashMap<String, PropMap>>;

fn text(p: &PropMap, key: &str) -> String {
    p.get(key)
        .and_then(|v| v.0.as_str())
        .unwrap_or_default()
        .into()
}

fn flag(p: &PropMap, key: &str) -> bool {
    p.get(key).and_then(|v| v.0.as_i64()).unwrap_or(0) != 0
}

fn number<T: TryFrom<u64>>(p: &PropMap, key: &str) -> Option<T> {
    p.get(key)
        .and_then(|v| v.0.as_u64())
        .and_then(|n| T::try_from(n).ok())
}

/// The unique name that owns a well-known name, if any.
pub fn owner(channel: &Channel, name: &str) -> Option<String> {
    Proxy::new(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        Duration::from_secs(1),
        channel,
    )
    .method_call("org.freedesktop.DBus", "GetNameOwner", (name,))
    .map(|(owner,): (String,)| owner)
    .ok()
}

/// Read the whole world. Err when BlueZ is there but did not answer in time (the caller keeps what
/// it knew and counts the miss).
pub fn observe(channel: &Channel, generations: &mut Generations) -> Result<World, String> {
    let mut world = World {
        bluez: owner(channel, "org.bluez"),
        bluealsa: owner(channel, "org.bluealsa"),
        ..World::default()
    };
    if let Some(bluez) = world.bluez.clone() {
        let objects = Proxy::new(bluez.as_str(), "/", Duration::from_secs(2), channel)
            .get_managed_objects()
            .map_err(|e| e.name().unwrap_or("org.bluez: no answer").to_owned())?;
        bluez_objects(&mut world, objects);
    }
    if let Some(bluealsa) = world.bluealsa.clone() {
        let objects = Proxy::new(
            bluealsa.as_str(),
            "/org/bluealsa",
            Duration::from_secs(1),
            channel,
        )
        .get_managed_objects()
        .unwrap_or_default();
        world.pcms = pcm_objects(objects);
    }
    generations.assign(&world.bluealsa, &mut world.pcms);
    Ok(world)
}

fn bluez_objects(world: &mut World, objects: Objects) {
    let mut volumes: HashMap<String, (String, u8)> = HashMap::new();
    for (path, interfaces) in &objects {
        if let Some(p) = interfaces.get("org.bluez.MediaTransport1") {
            if let Some(volume) = number::<u8>(p, "Volume").filter(|v| *v <= 127) {
                volumes.insert(text(p, "Device"), (path.to_string(), volume));
            }
        }
        if let Some(p) = interfaces.get("org.bluez.Adapter1") {
            if world.adapter.is_none() || &**path == "/org/bluez/hci0" {
                world.adapter = Some(Adapter {
                    path: path.to_string(),
                    powered: flag(p, "Powered"),
                    discovering: flag(p, "Discovering"),
                    alias: text(p, "Alias"),
                });
            }
        }
    }
    for (path, interfaces) in &objects {
        let Some(p) = interfaces.get("org.bluez.Device1") else {
            continue;
        };
        let address = text(p, "Address").to_ascii_uppercase();
        if !crate::desired::valid_address(&address) {
            continue;
        }
        let audio = p
            .get("UUIDs")
            .and_then(|v| v.0.as_iter())
            .is_some_and(|mut uuids| {
                uuids.any(|u| u.as_str().is_some_and(|s| s.eq_ignore_ascii_case(SINK)))
            });
        let path = path.to_string();
        let (transport, volume) = match volumes.remove(&path) {
            Some((transport, volume)) => (Some(transport), Some(volume)),
            None => (None, None),
        };
        world.devices.insert(
            address.clone(),
            Dev {
                name: text(p, "Alias"),
                paired: flag(p, "Paired"),
                bonded: flag(p, "Bonded"),
                trusted: flag(p, "Trusted"),
                blocked: flag(p, "Blocked"),
                connected: flag(p, "Connected"),
                resolved: flag(p, "ServicesResolved"),
                audio,
                transport,
                volume,
                path,
                address,
            },
        );
    }
}

fn pcm_objects(objects: Objects) -> Vec<PcmObs> {
    let mut pcms: Vec<PcmObs> = objects
        .into_iter()
        .filter_map(|(path, mut interfaces)| {
            let p = interfaces.remove("org.bluealsa.PCM1")?;
            let codec = text(&p, "Codec");
            Some(PcmObs {
                object: path.to_string(),
                device: text(&p, "Device"),
                transport: text(&p, "Transport"),
                mode: text(&p, "Mode"),
                codec: (!codec.is_empty()).then_some(codec),
                format: number(&p, "Format"),
                rate: number(&p, "Rate"),
                channels: number(&p, "Channels"),
                running: p.get("Running").and_then(|v| v.0.as_i64()).map(|v| v != 0),
                sequence: p.get("Sequence").and_then(|v| v.0.as_u64()),
                generation: 0,
            })
        })
        .collect();
    pcms.sort_by(|a, b| a.object.cmp(&b.object));
    pcms.truncate(16);
    pcms
}

/// One property read (a short answer; used where the async path would gain nothing).
pub fn get<T: for<'b> dbus::arg::Get<'b> + 'static>(
    channel: &Channel,
    owner: &str,
    path: &str,
    interface: &str,
    name: &str,
) -> Result<T, String> {
    Proxy::new(owner, path, Duration::from_secs(2), channel)
        .get(interface, name)
        .map_err(|e| e.name().unwrap_or("no answer").to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(object: &str, codec: &str, sequence: u64) -> PcmObs {
        PcmObs {
            object: object.into(),
            device: "/org/bluez/hci0/dev_AA".into(),
            transport: "A2DP-source".into(),
            mode: "sink".into(),
            codec: Some(codec.into()),
            rate: Some(44_100),
            sequence: Some(sequence),
            ..PcmObs::default()
        }
    }

    #[test]
    fn a_stream_keeps_its_generation_until_it_changes() {
        let owner = Some(":1.7".to_string());
        let mut generations = Generations::new(100);
        let mut first = vec![pcm("/a", "AAC", 1)];
        generations.assign(&owner, &mut first);
        let mut again = vec![pcm("/a", "AAC", 1)];
        again[0].running = Some(true);
        generations.assign(&owner, &mut again);
        assert_eq!(first[0].generation, again[0].generation);
        let mut codec = vec![pcm("/a", "LDAC", 1)];
        generations.assign(&owner, &mut codec);
        assert!(codec[0].generation > again[0].generation);
        let mut reconnect = vec![pcm("/a", "LDAC", 2)];
        generations.assign(&owner, &mut reconnect);
        assert!(reconnect[0].generation > codec[0].generation);
        let mut restarted = vec![pcm("/a", "LDAC", 2)];
        generations.assign(&Some(":1.9".into()), &mut restarted);
        assert!(restarted[0].generation > reconnect[0].generation);
    }
}
