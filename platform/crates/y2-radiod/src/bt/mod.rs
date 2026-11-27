//! The Bluetooth controller: one thread that alone powers the adapter, connects, pairs, picks
//! codecs, keeps BlueZ and BlueALSA running and recovers when something sticks. It waits on the
//! bus, its order pipe and the daemons' pidfds; every pass reads what changed, carries out the
//! owner's wishes one step at a time (every long call is asynchronous, with its own deadline) and
//! publishes its state when it changed.
// SPDX-License-Identifier: GPL-2.0-only
mod agent;
mod answers;
mod bus;
mod codec;
mod ladder;
mod links;
mod publish;
mod reconcile;
mod requests;
mod settle;
mod supervise;
mod world;

#[cfg(test)]
mod fake_tests;

use crate::{
    arbiter::Event,
    desired::Desired,
    journal::Journal,
    server::Client,
    wake::{Wake, Waker},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    sync::mpsc::{channel, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};
use y2p_contract::radiod::{Bluetooth, Ended, Failure, Request, Scan};
use y2p_core::Root;

/// An order for the controller: a client's request and where to answer.
pub enum Order {
    Request(Request, Client),
}

/// How the arbiter reaches the controller.
pub struct Handle {
    orders: Option<Sender<Order>>,
    waker: Waker,
}

impl Handle {
    pub fn send(&self, request: Request, client: Client) {
        let sent = self
            .orders
            .as_ref()
            .is_some_and(|orders| orders.send(Order::Request(request, client)).is_ok());
        if sent {
            self.waker.wake();
        }
    }
}

/// Without a handle the controller ends: its order queue closes and it is woken to see that.
impl Drop for Handle {
    fn drop(&mut self) {
        self.orders = None;
        self.waker.wake();
    }
}

/// Start the controller with the owner's wishes as the daemon read them.
pub fn spawn(
    root: &Root,
    journal: Journal,
    events: Sender<Event>,
    desired: Desired,
) -> io::Result<Handle> {
    let (orders, received) = channel();
    let wake = Wake::new()?;
    let waker = wake.waker();
    let controller = Controller::new(root, journal, events, received, wake, desired);
    thread::Builder::new()
        .name("radiod-bluetooth".into())
        .spawn(move || controller.run())?;
    Ok(Handle {
        orders: Some(orders),
        waker,
    })
}

/// Where a sleep is.
#[derive(Default)]
enum Sleep {
    #[default]
    Awake,
    /// Taking Bluetooth down; answer these when it is.
    Going {
        clients: Vec<Client>,
        until: Instant,
    },
    Down,
}

/// What the controller knows about one device beyond what BlueZ shows.
#[derive(Default)]
struct Record {
    episode: Option<links::Episode>,
    /// A connect call in flight (or BlueZ said it already connects): since when.
    connecting: Option<Instant>,
    /// BlueZ answered InProgress: it connects on its own; give it [`links::STUCK`].
    in_progress: bool,
    /// Every try failed: wait for something new.
    parked: bool,
    ended: Option<Ended>,
    /// Connected since.
    connected_since: Option<Instant>,
    /// Connected but not set up (no resolved services and no BlueALSA PCM) since: a set-up that
    /// does not finish is cancelled. Counted from when the set-up went missing, not from the
    /// connect: a codec change briefly removes the PCM of a headset that connected by itself
    /// (never resolved), and counting from the connect cancelled it at once (owner's AirPods,
    /// 2026-11-04 10:49:30).
    unset_since: Option<Instant>,
    /// Connected and resolved, but no BlueALSA PCM yet: since when, and whether the A2DP profile
    /// was asked for once already.
    pcm_missing_since: Option<Instant>,
    profile_asked: bool,
    /// Cancels in this episode (a second one escalates).
    cancels: u32,
    /// Links in a row that dropped right after coming up (`links::QUICK_DROPS` park the headset).
    quick_drops: u32,
    /// Reconnects made because BlueALSA listed the stream with more than two channels
    /// (`settle::WIDE_RECONNECTS` at most); a stereo stream starts the count again.
    wide_reconnects: u32,
    /// When a reconnect for that is due (the link is set up first).
    wide_at: Option<Instant>,
    /// That reconnect's disconnect is under way: it ends the link as `Ended::Local`.
    stereo_reconnect: bool,
}

struct Power {
    in_flight: bool,
    failures: u32,
    next: Instant,
    /// A recovery switches the adapter off and on: the off half is done when this is set.
    cycle: Option<Instant>,
}

struct Discovery {
    until: Instant,
    seen: BTreeSet<String>,
    /// StartDiscovery was sent (once the adapter is on).
    started: bool,
}

pub struct Controller {
    root: Root,
    journal: Journal,
    events: Sender<Event>,
    orders: Receiver<Order>,
    wake: Wake,
    desired: Desired,
    bus: Option<bus::Bus>,
    bus_retry: Instant,
    world: world::World,
    generations: world::Generations,
    dirty: bool,
    observed_at: Instant,
    observe_misses: u32,
    records: BTreeMap<String, Record>,
    /// The headset the owner asked for by a tap this run (else the preferred one).
    asked: Option<String>,
    /// Disconnected by the owner: no automatic connect until asked again.
    held: BTreeSet<String>,
    /// BlueZ's disconnect reasons by device path, until the observation sees the link gone.
    reasons: BTreeMap<String, Ended>,
    /// The device whose audio stream the player holds open.
    /// The owner's codec settings changed: BlueALSA starts again once no stream is open.
    reload_codecs: bool,
    /// Streams the player holds open, per device (a probe and a sink may overlap).
    streams: BTreeMap<String, u32>,
    sleep: Sleep,
    /// Set when a sleep begins: whether no headset was connected then. The adapter's next power-on
    /// (the wake) looks for the headset once instead of three times (`links::Trigger::Wake`).
    asleep_alone: Option<bool>,
    power: Power,
    discovery: Option<Discovery>,
    scan: Scan,
    pairing: agent::Agent,
    codec: settle::Codecs,
    ladder: ladder::Ladder,
    supervise: supervise::Supervise,
    failure: Option<Failure>,
    published: Option<Bluetooth>,
    sequence: u64,
}

impl Controller {
    fn new(
        root: &Root,
        journal: Journal,
        events: Sender<Event>,
        orders: Receiver<Order>,
        wake: Wake,
        desired: Desired,
    ) -> Controller {
        let now = Instant::now();
        let seed = y2p_core::clock::boottime_ns() / 1000;
        Controller {
            root: root.clone(),
            supervise: supervise::Supervise::new(root, journal.clone()),
            journal,
            events,
            orders,
            wake,
            desired,
            bus: None,
            bus_retry: now,
            world: world::World::default(),
            generations: world::Generations::new(seed),
            dirty: true,
            observed_at: now,
            observe_misses: 0,
            records: BTreeMap::new(),
            asked: None,
            held: BTreeSet::new(),
            reasons: BTreeMap::new(),
            streams: BTreeMap::new(),
            reload_codecs: false,
            sleep: Sleep::Awake,
            asleep_alone: None,
            power: Power {
                in_flight: false,
                failures: 0,
                next: now,
                cycle: None,
            },
            discovery: None,
            scan: Scan::Idle,
            pairing: agent::Agent::default(),
            codec: settle::Codecs::default(),
            ladder: ladder::Ladder::default(),
            failure: None,
            published: None,
            sequence: 0,
        }
    }

    fn run(mut self) {
        self.journal.event(
            "bluetooth_start",
            serde_json::json!({"wanted": self.desired.bluetooth}),
        );
        // The first look sees the adapter come on: a power-on for the preferred headset.
        loop {
            let now = Instant::now();
            self.supervise.check(now);
            self.connect_bus(now);
            self.drain_bus();
            if !self.drain_orders() {
                return;
            }
            self.observe(now);
            self.expire(Instant::now());
            self.reconcile(Instant::now());
            self.publish();
            self.wait();
        }
    }

    /// Carry out every waiting order; false when the arbiter is gone.
    fn drain_orders(&mut self) -> bool {
        loop {
            match self.orders.try_recv() {
                Ok(Order::Request(request, client)) => self.request(request, client),
                Err(std::sync::mpsc::TryRecvError::Empty) => return true,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return false,
            }
        }
    }

    /// Wait for the bus, an order, a daemon's exit or the next deadline.
    fn wait(&mut self) {
        // A synchronous call this pass (an observation, a property) may have queued messages that
        // the bus descriptor no longer announces: handle them before sleeping.
        if self.drain_bus() > 0 || self.dirty {
            return;
        }
        let now = Instant::now();
        let until = self.next_deadline(now);
        let mut fds = vec![libc::pollfd {
            fd: self.wake.fd(),
            events: libc::POLLIN,
            revents: 0,
        }];
        if let Some(bus) = &self.bus {
            fds.push(bus.pollfd());
        }
        fds.extend(self.supervise.pollfds());
        let left = until.saturating_duration_since(now);
        let _ = y2p_core::sys::poll(&mut fds, left, y2p_core::sys::Rounding::Up);
        self.wake.drain();
        if fds.iter().skip(1).any(|fd| fd.revents != 0) {
            // The bus or a pidfd: the next pass looks.
            self.supervise.note_ready(&fds);
        }
    }

    /// The earliest time something is due (at most a minute: the observation backstop).
    fn next_deadline(&self, now: Instant) -> Instant {
        let mut until = now + Duration::from_secs(60);
        if self.dirty {
            return now;
        }
        // Only what lies ahead: whatever was due has had its turn in this pass, and a past time that
        // could not be acted on (a power retry time while the power is right, an episode while the
        // adapter is off, a try while its page runs) must not turn the wait into a spin.
        let mut consider = |at: Option<Instant>| {
            if let Some(at) = at.filter(|at| *at > now) {
                until = until.min(at);
            }
        };
        consider(self.bus.as_ref().and_then(bus::Bus::next_deadline));
        if self.bus.is_none() {
            consider(Some(self.bus_retry));
        }
        consider(Some(self.observed_at + Duration::from_secs(60)));
        for record in self.records.values() {
            consider(record.episode.as_ref().map(|e| e.next));
            consider(record.connecting.map(|at| at + links::STUCK));
            consider(record.unset_since.map(|at| at + links::HALF_CONNECTED));
            consider(record.pcm_missing_since.map(|at| at + settle::PCM_WAIT));
            consider(record.wide_at);
        }
        consider(self.discovery.as_ref().map(|d| d.until));
        consider(self.pairing.deadline());
        consider((!self.power.in_flight).then_some(self.power.next));
        consider(self.power.cycle.map(|at| at + Duration::from_secs(2)));
        if let Sleep::Going { until: at, .. } = &self.sleep {
            consider(Some(*at));
        }
        consider(self.supervise.next());
        consider(self.codec.next());
        until.max(now)
    }

    /// The headset to keep connected: the one the owner tapped, else the preferred one when it
    /// connects on its own.
    fn target(&self) -> Option<String> {
        if let Some(asked) = &self.asked {
            return Some(asked.clone());
        }
        let preferred = self.desired.preferred.clone()?;
        (self.desired.auto_connect(&preferred) && !self.held.contains(&preferred))
            .then_some(preferred)
    }

    /// Start an episode for the headset to keep connected.
    fn trigger_preferred(&mut self, trigger: links::Trigger) {
        let Some(target) = self.target() else {
            return;
        };
        self.trigger(&target, trigger);
    }

    fn trigger(&mut self, address: &str, trigger: links::Trigger) {
        let now = Instant::now();
        let record = self.records.entry(address.to_owned()).or_default();
        record.parked = false;
        record.cancels = 0;
        // Something new (Bluetooth on, a wake, the owner's tap): the headset gets its tries again.
        if !matches!(trigger, links::Trigger::Lost(_)) {
            record.quick_drops = 0;
        }
        record.episode = links::Episode::start(trigger, now);
        self.journal.event(
            "reconnect_episode",
            serde_json::json!({
                "trigger": format!("{trigger:?}"), "started": record.episode.is_some()
            }),
        );
    }

    /// Something went wrong that the owner should see (each one once: it carries the next
    /// sequence).
    fn fail(&mut self, reason: &str, message: &str, address: Option<&str>) {
        self.journal.event(
            "failure",
            serde_json::json!({"reason": reason, "message": message}),
        );
        self.failure = Some(Failure {
            reason: reason.into(),
            message: message.into(),
            address: address.map(str::to_owned),
            sequence: self.sequence + 1,
        });
    }
}
