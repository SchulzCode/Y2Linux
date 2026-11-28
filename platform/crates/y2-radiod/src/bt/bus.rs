//! The system bus: one private connection, the signals the controller listens to, every call sent
//! asynchronously with a deadline of its own, and the dispatch of whatever arrives (signals, the
//! pairing agent's questions, call answers).
// SPDX-License-Identifier: GPL-2.0-only
use super::{Controller, Record};
use dbus::{
    arg::{AppendAll, PropMap, Variant},
    blocking::Proxy,
    channel::{BusType, Channel},
    message::MessageType,
    Message,
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// What an asynchronous call was for.
#[derive(Clone, Debug)]
pub enum Kind {
    Power(bool),
    Connect(String),
    /// A Disconnect that cancels a stuck connect (recovery), not the owner's.
    Cancel(String),
    Disconnect(String),
    Pair(String),
    Discovery(bool),
    Select {
        address: String,
        codec: String,
    },
    Volume,
    Remove(String),
    /// A call whose answer changes nothing (CancelPairing).
    Quiet,
    /// The A2DP profile for a connected headset without a stream.
    Profile,
}

pub struct Call {
    pub kind: Kind,
    pub deadline: Instant,
}

/// A call's answer: Ok, or the D-Bus error's name and message (`timeout` when none came in time).
pub type Answer = Result<(), (String, String)>;

pub struct Bus {
    pub channel: Channel,
    pub calls: HashMap<u32, Call>,
    pub agent_owner: Option<String>,
}

const MATCHES: [&str; 6] = [
    "type='signal',sender='org.freedesktop.DBus',member='NameOwnerChanged'",
    "type='signal',sender='org.bluez',interface='org.freedesktop.DBus.ObjectManager'",
    concat!(
        "type='signal',sender='org.bluez',",
        "interface='org.freedesktop.DBus.Properties',member='PropertiesChanged'"
    ),
    "type='signal',sender='org.bluez',interface='org.bluez.Device1',member='Disconnected'",
    "type='signal',sender='org.bluealsa',interface='org.freedesktop.DBus.ObjectManager'",
    concat!(
        "type='signal',sender='org.bluealsa',",
        "interface='org.freedesktop.DBus.Properties',member='PropertiesChanged'"
    ),
];

/// Tests: the private bus a fake BlueZ serves (libdbus reads the system bus address only once per
/// process, so each scenario names its own here).
#[cfg(test)]
pub static TEST_BUS: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn system() -> Result<Channel, dbus::Error> {
    #[cfg(test)]
    if let Some(address) = TEST_BUS.lock().ok().and_then(|bus| bus.clone()) {
        let mut channel = Channel::open_private(&address)?;
        channel.register()?;
        return Ok(channel);
    }
    Channel::get_private(BusType::System)
}

impl Bus {
    fn open() -> Result<Bus, String> {
        let mut channel =
            system().map_err(|e| e.message().unwrap_or("system D-Bus unavailable").to_owned())?;
        channel.set_watch_enabled(true);
        let proxy = Proxy::new(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            Duration::from_secs(2),
            &channel,
        );
        for rule in MATCHES {
            proxy
                .method_call::<(), _, _, _>("org.freedesktop.DBus", "AddMatch", (rule,))
                .map_err(|e| e.name().unwrap_or("AddMatch").to_owned())?;
        }
        Ok(Bus {
            channel,
            calls: HashMap::new(),
            agent_owner: None,
        })
    }

    pub fn pollfd(&self) -> libc::pollfd {
        let watch = self.channel.watch();
        let mut events = 0;
        if watch.read {
            events |= libc::POLLIN;
        }
        if watch.write {
            events |= libc::POLLOUT;
        }
        libc::pollfd {
            fd: watch.fd,
            events,
            revents: 0,
        }
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.calls.values().map(|call| call.deadline).min()
    }

    /// Send a method call; its answer comes back through [`Controller::answer`].
    pub fn call<A: AppendAll>(
        &mut self,
        target: (&str, &str, &str, &str),
        args: A,
        kind: Kind,
        within: Duration,
    ) -> bool {
        let (destination, path, interface, method) = target;
        let Ok(mut message) = Message::new_method_call(destination, path, interface, method) else {
            return false;
        };
        message.append_all(args);
        let Ok(serial) = self.channel.send(message) else {
            return false;
        };
        self.channel.flush();
        self.calls.insert(
            serial,
            Call {
                kind,
                deadline: Instant::now() + within,
            },
        );
        true
    }

    /// Set a property asynchronously.
    pub fn set<T: dbus::arg::Arg + dbus::arg::Append>(
        &mut self,
        target: (&str, &str, &str, &str),
        value: T,
        kind: Kind,
        within: Duration,
    ) -> bool {
        let (destination, path, interface, name) = target;
        self.call(
            (destination, path, "org.freedesktop.DBus.Properties", "Set"),
            (interface, name, Variant(value)),
            kind,
            within,
        )
    }

    /// A short synchronous property write (Pairable, Trusted): an answer within 3 s.
    pub fn set_now<T: dbus::arg::Arg + dbus::arg::Append>(
        &self,
        target: (&str, &str, &str, &str),
        value: T,
    ) -> Result<(), String> {
        use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
        let (destination, path, interface, name) = target;
        Proxy::new(destination, path, Duration::from_secs(3), &self.channel)
            .set(interface, name, value)
            .map_err(|e| e.name().unwrap_or("no answer").to_owned())
    }

    pub fn send(&self, message: Message) {
        let _ = self.channel.send(message);
        self.channel.flush();
    }
}

impl Controller {
    /// Connect to the system bus when there is no connection (every 2 s until it works).
    pub(super) fn connect_bus(&mut self, now: Instant) {
        if self.bus.is_some() || now < self.bus_retry {
            return;
        }
        match Bus::open() {
            Ok(bus) => {
                self.bus = Some(bus);
                self.dirty = true;
                self.journal.event("bus_connected", serde_json::json!({}));
            }
            Err(error) => {
                self.bus_retry = now + Duration::from_secs(2);
                self.journal
                    .event("bus_unavailable", serde_json::json!({"error": error}));
            }
        }
    }

    /// Read everything that arrived and hand each message to its handler: how many there were.
    pub(super) fn drain_bus(&mut self) -> usize {
        let Some(bus) = &self.bus else {
            return 0;
        };
        if bus.channel.read_write(Some(Duration::ZERO)).is_err() {
            self.lost_bus();
            return 1;
        }
        let mut messages = Vec::new();
        while let Some(message) = self.bus.as_ref().and_then(|b| b.channel.pop_message()) {
            messages.push(message);
            if messages.len() >= 256 {
                break;
            }
        }
        let count = messages.len();
        for message in messages {
            self.message(message);
        }
        count
    }

    fn lost_bus(&mut self) {
        let calls: Vec<Call> = self
            .bus
            .take()
            .map(|bus| bus.calls.into_values().collect())
            .unwrap_or_default();
        for call in calls {
            self.answer(call, Err(("bus_lost".into(), String::new())));
        }
        self.bus_retry = Instant::now() + Duration::from_secs(2);
        self.journal.event("bus_lost", serde_json::json!({}));
    }

    fn message(&mut self, message: Message) {
        match message.msg_type() {
            MessageType::Signal => self.signal(&message),
            MessageType::MethodCall => self.agent_call(message),
            MessageType::MethodReturn | MessageType::Error => self.reply(message),
        }
    }

    fn signal(&mut self, message: &Message) {
        let interface = message
            .interface()
            .map(|i| i.to_string())
            .unwrap_or_default();
        let member = message.member().map(|m| m.to_string()).unwrap_or_default();
        match (interface.as_str(), member.as_str()) {
            ("org.freedesktop.DBus", "NameOwnerChanged") => {
                if let Ok((name, _, new)) = message.read3::<String, String, String>() {
                    if name == "org.bluez" || name == "org.bluealsa" {
                        self.dirty = true;
                        self.journal.event(
                            "owner_changed",
                            serde_json::json!({"name": name, "present": !new.is_empty()}),
                        );
                    }
                }
            }
            ("org.bluez.Device1", "Disconnected") => {
                let path = message.path().map(|p| p.to_string()).unwrap_or_default();
                if let Ok((reason, text)) = message.read2::<String, String>() {
                    let quiescing = !matches!(self.sleep, super::Sleep::Awake);
                    let ended = super::links::ended(&reason, quiescing);
                    // BlueZ's device path ends in dev_AA_BB_…: the address (reports pseudonymise it).
                    let address = path.rsplit("/dev_").next().map(|a| a.replace('_', ":"));
                    self.journal.event(
                        "peer_disconnected",
                        serde_json::json!({
                            "address": address, "ended": ended, "bluez_reason": reason,
                            "message": text
                        }),
                    );
                    self.reasons.insert(path, ended);
                    self.dirty = true;
                }
            }
            ("org.freedesktop.DBus.Properties", "PropertiesChanged") => {
                if relevant(message) {
                    self.dirty = true;
                }
            }
            ("org.freedesktop.DBus.ObjectManager", _) => self.dirty = true,
            _ => {}
        }
    }

    fn reply(&mut self, mut message: Message) {
        let Some(serial) = message.get_reply_serial() else {
            return;
        };
        let Some(call) = self.bus.as_mut().and_then(|bus| bus.calls.remove(&serial)) else {
            return;
        };
        let answer = match message.as_result() {
            Ok(_) => Ok(()),
            Err(error) => Err((
                error
                    .name()
                    .unwrap_or("org.freedesktop.DBus.Error.Failed")
                    .to_owned(),
                error.message().unwrap_or_default().to_owned(),
            )),
        };
        self.dirty = true;
        self.answer(call, answer);
    }

    /// Calls whose deadline passed: answered as a timeout (BlueZ may still be working on them).
    pub(super) fn expire_calls(&mut self, now: Instant) {
        let Some(bus) = self.bus.as_mut() else {
            return;
        };
        let late: Vec<u32> = bus
            .calls
            .iter()
            .filter(|(_, call)| call.deadline <= now)
            .map(|(serial, _)| *serial)
            .collect();
        let calls: Vec<Call> = late
            .into_iter()
            .filter_map(|serial| bus.calls.remove(&serial))
            .collect();
        for call in calls {
            self.answer(call, Err(("timeout".into(), String::new())));
        }
    }

    /// Whether a call of this kind is in flight.
    pub(super) fn in_flight(&self, matches: impl Fn(&Kind) -> bool) -> bool {
        self.bus
            .as_ref()
            .is_some_and(|bus| bus.calls.values().any(|call| matches(&call.kind)))
    }

    pub(super) fn record(&mut self, address: &str) -> &mut Record {
        self.records.entry(address.to_owned()).or_default()
    }
}

/// Property changes worth a new look (not RSSI or ManufacturerData while scanning).
fn relevant(message: &Message) -> bool {
    let Ok((interface, changed, invalidated)) = message.read3::<String, PropMap, Vec<String>>()
    else {
        return true;
    };
    let keys: &[&str] = match interface.as_str() {
        "org.bluez.Adapter1" => &["Powered", "Discovering", "Alias"],
        "org.bluez.Device1" => &[
            "Alias",
            "UUIDs",
            "Paired",
            "Bonded",
            "Trusted",
            "Blocked",
            "Connected",
            "ServicesResolved",
        ],
        "org.bluealsa.PCM1" => &[
            "Device",
            "Transport",
            "Mode",
            "Codec",
            "Format",
            "Rate",
            "Channels",
            "Running",
            "Sequence",
        ],
        "org.bluez.MediaTransport1" => &["Volume", "State"],
        _ => &[],
    };
    keys.iter()
        .any(|key| changed.contains_key(*key) || invalidated.iter().any(|v| v == key))
}
