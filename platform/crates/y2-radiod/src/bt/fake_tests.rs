//! The controller against a fake BlueZ and BlueALSA on a private bus (`bus::TEST_BUS`; the scenarios
//! run one after another in one test). Each scenario is
//! a field failure the old stack mishandled: a stuck page (F65, #92), headphones switched off, a
//! remote end, the owner's own disconnect, a codec the headphones refuse, a stream that never came,
//! a codec change that ends the link (#94), a car that pairs from its own side (#116); and the
//! owner's name for the player (Y2Reborn #217).
use super::*;
use crate::arbiter::Event;
use dbus::{
    arg::{PropMap, RefArg, Variant},
    blocking::Connection,
    channel::{Channel, MatchingReceiver, Sender},
    message::MatchRule,
    Message, Path as DbusPath,
};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use y2p_contract::radiod::{Ended, Link, Radio, Request, Service};

const ADAPTER: &str = "/org/bluez/hci0";
const DEVICE: &str = "/org/bluez/hci0/dev_12_34_56_78_90_AB";
const ADDRESS: &str = "12:34:56:78:90:AB";
const PCM: &str = "/org/bluealsa/hci0/dev_12_34_56_78_90_AB/a2dpsrc/sink";

/// How the fake behaves.
#[derive(Default, Clone)]
struct Fake {
    powered: bool,
    connected: bool,
    connects: u32,
    selects: Vec<String>,
    codec: String,
    /// The first ConnectProfile answers InProgress (a page racing the headset, F65).
    in_progress_first: bool,
    /// Every ConnectProfile answers "Host is down" (headphones switched off).
    host_down: bool,
    /// The headphones offer AAC too, and refuse it when selected.
    refuse_aac: bool,
    /// BlueALSA shows no PCM for the connected headset.
    no_pcm: bool,
    /// The headphones connected by themselves: BlueZ (5.87) looks up no services for a known
    /// headset that connects to us, so ServicesResolved stays false (wake, owner's player).
    arrived: bool,
    /// This many GetCodecs calls list no codec (BlueZ has not asked the headset yet).
    late_codecs: u32,
    /// The headphones offer AAC too (and accept it).
    offers_aac: bool,
    /// The codec every connect negotiates (BlueZ picks BlueALSA's first endpoint, AAC); empty keeps
    /// the last one.
    negotiates: String,
    /// A change within this long of a connect ends the link with EIO: BlueZ still opens the stream
    /// and the change cancels that (#94).
    fragile_for: Duration,
    /// This many changes end the link whenever they come.
    breaks: u32,
    /// When the last connect was answered, and how long after it each change came.
    connected_at: Option<Instant>,
    select_after: Vec<Duration>,
    /// A change takes BlueALSA's stream away for this long (it is set up again for the new codec).
    select_gap: Duration,
    gap_until: Option<Instant>,
    /// The device is not paired yet (a car, #116); `paired`/`trusted` once BlueZ bonded/trusted it.
    stranger: bool,
    paired: bool,
    trusted: bool,
    /// The adapter's Discoverable and Pairable as last written.
    discoverable: bool,
    pairable: bool,
    /// Who registered the pairing agent, and the agent's answer to AuthorizeService.
    agent: Option<String>,
    authorized: Option<bool>,
    /// The owner's name for the adapter (BlueZ's stored Alias); none: main.conf's Name.
    alias: Option<String>,
}

/// main.conf's `Name`.
const SYSTEM_NAME: &str = "Y2Linux";

impl Fake {
    fn alias(&self) -> String {
        self.alias.clone().unwrap_or_else(|| SYSTEM_NAME.into())
    }
}

fn variant<T: RefArg + 'static>(value: T) -> Variant<Box<dyn RefArg>> {
    Variant(Box::new(value))
}

type Objects = HashMap<DbusPath<'static>, HashMap<String, PropMap>>;

fn bluez_objects(fake: &Fake) -> Objects {
    let mut adapter = PropMap::new();
    adapter.insert("Powered".into(), variant(fake.powered));
    adapter.insert("Discovering".into(), variant(false));
    adapter.insert("Alias".into(), variant(fake.alias()));
    let mut device = PropMap::new();
    device.insert("Address".into(), variant(ADDRESS.to_string()));
    device.insert("Alias".into(), variant("Buds".to_string()));
    let paired = !fake.stranger || fake.paired;
    device.insert("Paired".into(), variant(paired));
    device.insert("Bonded".into(), variant(paired));
    device.insert("Trusted".into(), variant(!fake.stranger || fake.trusted));
    device.insert("Blocked".into(), variant(false));
    device.insert("Connected".into(), variant(fake.connected));
    device.insert(
        "ServicesResolved".into(),
        variant(fake.connected && !fake.arrived),
    );
    device.insert(
        "UUIDs".into(),
        variant(vec![super::world::SINK.to_string()]),
    );
    let mut objects = Objects::new();
    objects.insert(
        DbusPath::from(ADAPTER),
        HashMap::from([("org.bluez.Adapter1".to_string(), adapter)]),
    );
    objects.insert(
        DbusPath::from(DEVICE),
        HashMap::from([("org.bluez.Device1".to_string(), device)]),
    );
    objects
}

fn bluealsa_objects(fake: &Fake) -> Objects {
    let mut objects = Objects::new();
    let gap = fake.gap_until.is_some_and(|at| Instant::now() < at);
    if fake.connected && !fake.no_pcm && !gap {
        let mut pcm = PropMap::new();
        pcm.insert("Device".into(), variant(DbusPath::from(DEVICE)));
        pcm.insert("Transport".into(), variant("A2DP-source".to_string()));
        pcm.insert("Mode".into(), variant("sink".to_string()));
        pcm.insert("Codec".into(), variant(fake.codec.clone()));
        pcm.insert("Format".into(), variant(0x8210u16));
        pcm.insert("Rate".into(), variant(44_100u32));
        pcm.insert("Channels".into(), variant(2u8));
        pcm.insert("Running".into(), variant(false));
        pcm.insert("Sequence".into(), variant(fake.connects));
        objects.insert(
            DbusPath::from(PCM),
            HashMap::from([("org.bluealsa.PCM1".to_string(), pcm)]),
        );
    }
    objects
}

/// BlueALSA adds or removes a PCM object (a codec change replaces the stream).
fn pcm_event(added: bool) -> Message {
    let member = if added {
        "InterfacesAdded"
    } else {
        "InterfacesRemoved"
    };
    Message::new_signal(
        "/org/bluealsa",
        "org.freedesktop.DBus.ObjectManager",
        member,
    )
    .expect("signal")
    .append1(DbusPath::from(PCM))
}

fn changed(path: &str, interface: &str) -> Message {
    Message::new_signal(path, "org.freedesktop.DBus.Properties", "PropertiesChanged")
        .expect("signal")
        .append3(
            interface,
            PropMap::new(),
            vec!["Powered".to_string(), "Connected".into()],
        )
}

fn codecs(fake: &Fake) -> HashMap<String, PropMap> {
    let stereo = || {
        let mut p = PropMap::new();
        p.insert("Channels".into(), variant(vec![1u8, 2u8]));
        p.insert("Rates".into(), variant(vec![44_100u32, 48_000u32]));
        p
    };
    let mut all = HashMap::from([("SBC".to_string(), stereo())]);
    if fake.refuse_aac || fake.offers_aac {
        all.insert("AAC".into(), stereo());
    }
    all
}

/// Answer one method call as BlueZ or BlueALSA would.
fn answer(fake: &Mutex<Fake>, message: &Message, connection: &Connection) -> Message {
    let member = message.member().map(|m| m.to_string()).unwrap_or_default();
    let path = message.path().map(|p| p.to_string()).unwrap_or_default();
    let mut fake = fake.lock().expect("fake");
    match member.as_str() {
        "GetManagedObjects" if path == "/" => message.return_with_args((bluez_objects(&fake),)),
        "GetManagedObjects" => message.return_with_args((bluealsa_objects(&fake),)),
        "Set" => {
            let (_, name, value): (String, String, Variant<Box<dyn RefArg>>) =
                message.read3().expect("Set arguments");
            let on = value.0.as_u64().unwrap_or(0) != 0;
            match name.as_str() {
                "Powered" => {
                    fake.powered = on;
                    if !on {
                        fake.connected = false;
                    }
                    let _ = connection.send(changed(ADAPTER, "org.bluez.Adapter1"));
                }
                "Discoverable" => fake.discoverable = on,
                "Pairable" => fake.pairable = on,
                "Trusted" => fake.trusted = on,
                "Alias" => {
                    // BlueZ: an empty Alias goes back to the system name.
                    let alias = value.0.as_str().unwrap_or_default().to_owned();
                    fake.alias = (!alias.is_empty()).then_some(alias);
                    let _ = connection.send(changed(ADAPTER, "org.bluez.Adapter1"));
                }
                _ => {}
            }
            message.method_return()
        }
        "Get" if path.starts_with("/org/bluez") => {
            let (_, name): (String, String) = message.read2().expect("Get arguments");
            match name.as_str() {
                "PairableTimeout" => message.return_with_args((variant(0u32),)),
                "Pairable" => message.return_with_args((variant(fake.pairable),)),
                "Alias" => message.return_with_args((variant(fake.alias()),)),
                _ => message.return_with_args((variant(!fake.stranger || fake.paired),)),
            }
        }
        "RegisterAgent" => {
            fake.agent = message.sender().map(|s| s.to_string());
            message.method_return()
        }
        "ConnectProfile" => {
            fake.connects += 1;
            if fake.in_progress_first && fake.connects == 1 {
                return message.error(&"org.bluez.Error.InProgress".into(), c"In Progress");
            }
            if fake.host_down {
                return message.error(&"org.bluez.Error.Failed".into(), c"Host is down");
            }
            fake.connected = true;
            fake.arrived = false;
            fake.connected_at = Some(Instant::now());
            if !fake.negotiates.is_empty() {
                fake.codec = fake.negotiates.clone();
            }
            let _ = connection.send(changed(DEVICE, "org.bluez.Device1"));
            message.method_return()
        }
        "Disconnect" => {
            fake.connected = false;
            let _ = connection.send(changed(DEVICE, "org.bluez.Device1"));
            message.method_return()
        }
        "Get" => {
            let mut runtime = vec!["A2DP-source:SBC".to_string()];
            if fake.refuse_aac || fake.offers_aac {
                runtime.push("A2DP-source:AAC".into());
            }
            message.return_with_args((variant(runtime),))
        }
        "GetCodecs" if fake.late_codecs > 0 => {
            fake.late_codecs -= 1;
            message.return_with_args((HashMap::<String, PropMap>::new(),))
        }
        "GetCodecs" => message.return_with_args((codecs(&fake),)),
        "SelectCodec" => {
            let codec: String = message.read1().expect("codec");
            fake.selects.push(codec.clone());
            let after = fake.connected_at.map_or(Duration::MAX, |at| at.elapsed());
            fake.select_after.push(after);
            if fake.breaks > 0 || after < fake.fragile_for {
                fake.breaks = fake.breaks.saturating_sub(1);
                fake.connected = false;
                let _ = connection.send(
                    Message::new_signal(DEVICE, "org.bluez.Device1", "Disconnected")
                        .expect("signal")
                        .append2("org.bluez.Reason.Remote", "Connection terminated"),
                );
                let _ = connection.send(changed(DEVICE, "org.bluez.Device1"));
                return message.error(
                    &"org.freedesktop.DBus.Error.Failed".into(),
                    c"Input/output error",
                );
            }
            if codec == "AAC" && fake.refuse_aac {
                return message.error(
                    &"org.freedesktop.DBus.Error.NotSupported".into(),
                    c"Not supported",
                );
            }
            fake.codec = codec;
            if !fake.select_gap.is_zero() {
                fake.gap_until = Some(Instant::now() + fake.select_gap);
                let _ = connection.send(pcm_event(false));
            }
            message.method_return()
        }
        _ => message.method_return(),
    }
}

/// What a scenario can make the fake do on its own.
enum Control {
    /// The headphones end the link with this BlueZ reason.
    Drop(&'static str),
    /// The headphones connect by themselves.
    Arrive,
    /// A car pairs from its side: BlueZ asks the agent to confirm its number.
    Pair,
    /// BlueZ bonded the car.
    Bond,
    /// The car opens its audio: BlueZ asks the agent (from another thread, as BlueZ waits).
    Authorize(String),
}

fn act(fake: &Arc<Mutex<Fake>>, control: Control, connection: &Connection) {
    let agent = || {
        fake.lock()
            .expect("fake")
            .agent
            .clone()
            .expect("agent registered")
    };
    match control {
        Control::Pair => {
            let question = Message::new_method_call(
                agent(),
                super::agent::PATH,
                "org.bluez.Agent1",
                "RequestConfirmation",
            )
            .expect("call")
            .append2(DbusPath::from(DEVICE), 123_456u32);
            let _ = connection.send(question);
            return;
        }
        Control::Bond => fake.lock().expect("fake").paired = true,
        Control::Authorize(bus) => {
            let (fake, agent) = (fake.clone(), agent());
            std::thread::spawn(move || {
                let mut channel = Channel::open_private(&bus).expect("bus");
                channel.register().expect("register");
                let asking = Connection::from(channel);
                let answer: Result<(), dbus::Error> = asking
                    .with_proxy(agent, super::agent::PATH, Duration::from_secs(3))
                    .method_call(
                        "org.bluez.Agent1",
                        "AuthorizeService",
                        (DbusPath::from(DEVICE), super::world::SINK),
                    );
                fake.lock().expect("fake").authorized = Some(answer.is_ok());
            });
            return;
        }
        Control::Drop(reason) => {
            fake.lock().expect("fake").connected = false;
            let signal = Message::new_signal(DEVICE, "org.bluez.Device1", "Disconnected")
                .expect("signal")
                .append2(reason, "Connection terminated");
            let _ = connection.send(signal);
        }
        Control::Arrive => {
            let mut fake = fake.lock().expect("fake");
            fake.connected = true;
            fake.arrived = true;
        }
    }
    let _ = connection.send(changed(DEVICE, "org.bluez.Device1"));
}

struct Bus {
    daemon: Child,
    address: String,
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

fn private_bus() -> Option<Bus> {
    let mut daemon = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    let mut address = String::new();
    BufReader::new(daemon.stdout.take()?)
        .read_line(&mut address)
        .ok()?;
    Some(Bus {
        daemon,
        address: address.trim().to_owned(),
    })
}

/// One scenario's world: a bus, the fake on it, the controller, its fixture root.
struct Harness {
    _bus: Bus,
    fake: Arc<Mutex<Fake>>,
    stop: Arc<AtomicBool>,
    control: mpsc::Sender<Control>,
    server: Option<std::thread::JoinHandle<()>>,
    events: mpsc::Receiver<Event>,
    handle: Handle,
    dir: std::path::PathBuf,
    started: Instant,
}

impl Harness {
    fn start(name: &str, fake: Fake) -> Option<Harness> {
        Harness::choosing(name, fake, "Auto")
    }

    /// The owner chose this codec before the player started.
    fn choosing(name: &str, fake: Fake, preference: &str) -> Option<Harness> {
        let bus = private_bus()?;
        if let Ok(mut test_bus) = super::bus::TEST_BUS.lock() {
            *test_bus = Some(bus.address.clone());
        }
        let fake = Arc::new(Mutex::new(Fake {
            codec: if fake.codec.is_empty() {
                "SBC".into()
            } else {
                fake.codec.clone()
            },
            ..fake
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (control, controls) = mpsc::channel();
        let (ready, served) = mpsc::channel();
        let server = {
            let (address, fake, stop) = (bus.address.clone(), fake.clone(), stop.clone());
            std::thread::spawn(move || serve(address, fake, stop, ready, controls))
        };
        served.recv_timeout(Duration::from_secs(5)).ok()?;
        let dir = std::env::temp_dir().join(format!("y2-radiod-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("data/bluetooth")).ok()?;
        std::fs::create_dir_all(dir.join("etc/y2linux")).ok()?;
        std::fs::write(dir.join("data/bluetooth/enabled"), "1\n").ok()?;
        std::fs::write(dir.join("data/bluetooth/preferred-audio"), ADDRESS).ok()?;
        std::fs::write(
            dir.join("data/bluetooth/radiod.json"),
            format!(r#"{{"schema":1,"preference":"{preference}","manual":[]}}"#),
        )
        .ok()?;
        let approved =
            r#"{"compiled_locally":true,"distribution_approved":true,"platform_qualified":true}"#;
        std::fs::write(
            dir.join("etc/y2linux/bluetooth-codecs.json"),
            format!(r#"{{"schema":1,"codecs":{{"SBC":{approved},"AAC":{approved}}}}}"#),
        )
        .ok()?;
        let root = Root::new(&dir);
        let (sender, events) = mpsc::channel();
        let journal = crate::journal::Journal::new(&root);
        let handle = spawn(&root, journal, sender, Desired::load(&root)).ok()?;
        Some(Harness {
            _bus: bus,
            fake,
            stop,
            control,
            server: Some(server),
            events,
            handle,
            dir,
            started: Instant::now(),
        })
    }

    /// The first published state that satisfies `wanted`, within `within`.
    fn until(&self, within: Duration, wanted: impl Fn(&Bluetooth) -> bool) -> Option<Bluetooth> {
        let end = Instant::now() + within;
        while Instant::now() < end {
            match self
                .events
                .recv_timeout(end.saturating_duration_since(Instant::now()))
            {
                Ok(Event::Bluetooth(state)) if wanted(&state) => return Some(*state),
                Ok(_) => {}
                Err(_) => return None,
            }
        }
        None
    }

    fn link(&self, within: Duration, link: Link) -> Option<Bluetooth> {
        self.until(within, |s| s.devices.iter().any(|d| d.link == link))
    }

    /// Ask the controller and read its answer.
    fn ask(&self, request: Request) -> String {
        let (client, peer) = std::os::unix::net::UnixStream::pair().expect("pair");
        self.handle
            .send(request, crate::server::Client::for_tests(client));
        let mut answer = String::new();
        peer.set_read_timeout(Some(Duration::from_secs(16)))
            .expect("timeout");
        let _ = BufReader::new(peer).read_line(&mut answer);
        answer
    }

    fn fake(&self) -> Fake {
        self.fake.lock().expect("fake").clone()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Serve BlueZ and BlueALSA on the bus until `stop`.
fn serve(
    address: String,
    fake: Arc<Mutex<Fake>>,
    stop: Arc<AtomicBool>,
    ready: mpsc::Sender<()>,
    controls: mpsc::Receiver<Control>,
) {
    let mut channel = Channel::open_private(&address).expect("bus");
    channel.register().expect("register");
    let connection = Connection::from(channel);
    for name in ["org.bluez", "org.bluealsa"] {
        connection
            .request_name(name, false, true, false)
            .expect("name");
    }
    let answering = fake.clone();
    connection.start_receive(
        MatchRule::new_method_call(),
        Box::new(move |message, connection| {
            let reply = answer(&answering, &message, connection);
            let _ = connection.send(reply);
            true
        }),
    );
    let _ = ready.send(());
    while !stop.load(Ordering::Acquire) {
        let _ = connection.process(Duration::from_millis(50));
        while let Ok(control) = controls.try_recv() {
            act(&fake, control, &connection);
        }
        // The stream is back after a change: BlueALSA announces it.
        let mut state = fake.lock().expect("fake");
        if state.gap_until.is_some_and(|at| Instant::now() >= at) {
            state.gap_until = None;
            let _ = connection.send(pcm_event(true));
        }
    }
}

/// This process's user + system CPU time in clock ticks (/proc/self/stat fields 14 and 15).
fn cpu_ticks() -> u64 {
    let stat = std::fs::read_to_string("/proc/self/stat").expect("stat");
    let fields: Vec<&str> = stat
        .rsplit_once(')')
        .map(|(_, rest)| rest.split_whitespace().collect())
        .unwrap_or_default();
    let tick = |index: usize| {
        fields
            .get(index)
            .and_then(|f| f.parse::<u64>().ok())
            .unwrap_or(0)
    };
    tick(11) + tick(12)
}

const LONG: Duration = Duration::from_secs(40);

/// F65/#92: power on, the adoption window, a page meeting InProgress is cancelled and retried, the
/// stream becomes Ready, the controller then idles, and a sleep takes the adapter down.
fn power_on_adoption_in_progress_and_sleep() {
    let Some(h) = Harness::start(
        "inprogress",
        Fake {
            in_progress_first: true,
            ..Fake::default()
        },
    ) else {
        return;
    };
    let ready = h.link(LONG, Link::Ready).expect("headset ready");
    assert!(ready.powered && ready.bluealsa);
    assert!(
        h.started.elapsed() >= links::ADOPTION,
        "no page before adoption"
    );
    assert!(h.fake().connects >= 2, "InProgress was retried");
    assert!(ready.pcms[0].generation > 0);
    let before = cpu_ticks();
    std::thread::sleep(Duration::from_secs(2));
    let spent = cpu_ticks() - before;
    assert!(spent < 20, "{spent} ticks of CPU in 2 s while idle");
    let answer = h.ask(Request::Quiesce {
        radio: Radio::Bluetooth,
    });
    assert!(answer.contains("\"ok\":true"), "{answer}");
    h.until(Duration::from_secs(5), |s| {
        s.service == Service::Quiesced && !s.powered
    })
    .expect("down");
}

/// Headphones switched off: three tries, then parked (no paging forever); when they come on and
/// connect by themselves they are Ready without a tap, and stay connected although BlueZ never
/// resolves their services (9.3.5 wake: cancelled as half connected after 8 s); their codecs, known
/// only after the stream, are waited for (9.3.6: nothing mutually usable).
fn switched_off_parks_then_returns_by_itself() {
    let Some(h) = Harness::start(
        "off",
        Fake {
            host_down: true,
            late_codecs: 2,
            ..Fake::default()
        },
    ) else {
        return;
    };
    h.link(LONG, Link::Parked).expect("parked");
    let tries = h.fake().connects;
    assert_eq!(tries, 3, "three tries for a power-on");
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(h.fake().connects, tries, "no paging while parked");
    let _ = h.control.send(Control::Arrive);
    let back = h
        .link(Duration::from_secs(10), Link::Ready)
        .expect("back by itself");
    let codec = back.codec.expect("codec decision");
    assert!(codec.eligibility["SBC"].mutually_usable, "{codec:?}");
    assert_eq!(codec.state, "Settled");
    std::thread::sleep(links::HALF_CONNECTED + Duration::from_secs(1));
    let after = h.fake();
    assert!(
        after.connected,
        "a stream without resolved services was cancelled"
    );
    assert_eq!(after.connects, tries, "no page for a headset that is back");
}

/// A remote end gets one look; the owner's own disconnect gets none and says so.
fn remote_end_one_look_and_owners_disconnect_none() {
    let Some(h) = Harness::start("remote", Fake::default()) else {
        return;
    };
    h.link(LONG, Link::Ready).expect("ready");
    let tries = h.fake().connects;
    let _ = h.control.send(Control::Drop("org.bluez.Reason.Remote"));
    let ended = h
        .until(Duration::from_secs(5), |s| {
            s.devices.iter().any(|d| d.ended == Some(Ended::Remote))
        })
        .expect("remote end seen");
    assert!(!ended.devices[0].connected);
    h.link(Duration::from_secs(10), Link::Ready)
        .expect("one look reconnected");
    assert_eq!(h.fake().connects, tries + 1);
    let answer = h.ask(Request::Disconnect {
        address: ADDRESS.into(),
    });
    assert!(answer.contains("\"ok\":true"), "{answer}");
    h.until(Duration::from_secs(5), |s| {
        s.devices.iter().any(|d| d.ended == Some(Ended::User))
    })
    .expect("owner's end");
    let tries = h.fake().connects;
    std::thread::sleep(Duration::from_secs(6));
    assert_eq!(
        h.fake().connects,
        tries,
        "no reconnect after the owner's disconnect"
    );
}

/// AAC chosen, the headphones refuse it: SBC stays, the stream is Ready, nothing blocks playback.
fn refused_codec_falls_back_and_stays_playable() {
    let Some(h) = Harness::choosing(
        "codec",
        Fake {
            refuse_aac: true,
            ..Fake::default()
        },
        "AAC",
    ) else {
        return;
    };
    let ready = h.link(LONG, Link::Ready).expect("ready");
    assert_eq!(h.fake().selects, ["AAC"], "AAC tried once");
    assert_eq!(ready.pcms[0].codec.as_deref(), Some("SBC"));
    let history =
        std::fs::read_to_string(h.dir.join("data/bluetooth/codec-history.json")).expect("history");
    assert!(history.contains("\"AAC\":1"), "{history}");
}

/// #94: SBC chosen, the headphones connect with AAC. The change waits until the link is set up
/// (sent at once it ends the link, and the reconnect met it again ≈40 times); Auto changes nothing.
fn codec_change_waits_for_the_link() {
    let aac = || Fake {
        offers_aac: true,
        negotiates: "AAC".into(),
        fragile_for: Duration::from_secs(2),
        ..Fake::default()
    };
    let Some(h) = Harness::choosing("switch", aac(), "SBC") else {
        return;
    };
    let ready = h.link(LONG, Link::Ready).expect("ready");
    let fake = h.fake();
    assert_eq!(fake.selects, ["SBC"], "one change");
    assert!(
        fake.select_after[0] >= settle::SWITCH_QUIET,
        "{:?}",
        fake.select_after
    );
    assert!(fake.connected && ready.devices[0].connected);
    drop(h);
    let Some(h) = Harness::start("auto", aac()) else {
        return;
    };
    let ready = h.link(LONG, Link::Ready).expect("ready");
    assert!(
        h.fake().selects.is_empty(),
        "Auto keeps the AAC it connected with"
    );
    assert_eq!(ready.pcms[0].codec.as_deref(), Some("AAC"));
}

/// A headset whose link every change ends: one change, then it keeps AAC through the reconnect
/// instead of dropping again and again (#94).
fn a_change_that_ends_the_link_is_not_repeated() {
    let Some(h) = Harness::choosing(
        "breaks",
        Fake {
            offers_aac: true,
            negotiates: "AAC".into(),
            breaks: u32::MAX,
            ..Fake::default()
        },
        "SBC",
    ) else {
        return;
    };
    h.until(LONG, |s| {
        s.devices.iter().any(|d| d.ended == Some(Ended::Remote))
    })
    .expect("the change ended the link");
    let back = h
        .until(Duration::from_secs(15), |s| {
            s.devices.iter().any(|d| d.link == Link::Ready) && !s.pcms.is_empty()
        })
        .expect("back and ready");
    assert_eq!(back.pcms[0].codec.as_deref(), Some("AAC"));
    std::thread::sleep(settle::SWITCH_QUIET + Duration::from_secs(2));
    let fake = h.fake();
    assert_eq!(fake.selects, ["SBC"], "not asked again");
    assert!(fake.connected, "the link stays");
}

/// AirPods that connected by themselves (services never resolved) get another codec from the owner:
/// the stream is briefly gone while it changes, and the link stays (it was cancelled at once as
/// half connected, owner's player 2026-11-04 10:49:30).
fn a_codec_change_keeps_a_headset_that_came_by_itself() {
    let Some(h) = Harness::start(
        "gap",
        Fake {
            host_down: true,
            offers_aac: true,
            select_gap: Duration::from_secs(2),
            ..Fake::default()
        },
    ) else {
        return;
    };
    h.link(LONG, Link::Parked).expect("parked");
    let _ = h.control.send(Control::Arrive);
    h.link(Duration::from_secs(10), Link::Ready)
        .expect("back by itself");
    std::thread::sleep(links::HALF_CONNECTED + Duration::from_secs(1));
    let answer = h.ask(Request::CodecPreference {
        preference: "AAC".into(),
    });
    assert!(answer.contains("\"ok\":true"), "{answer}");
    let changed = h
        .until(Duration::from_secs(10), |s| {
            s.pcms.first().and_then(|p| p.codec.as_deref()) == Some("AAC")
        })
        .expect("changed to AAC");
    assert!(changed.devices[0].connected);
    std::thread::sleep(Duration::from_secs(3));
    let fake = h.fake();
    assert!(fake.connected, "the link was cancelled during the change");
    assert_eq!(fake.selects, ["AAC"]);
}

/// Connected and resolved, but BlueALSA shows no stream: A2DP is asked for, then BlueALSA restarts.
fn missing_stream_asks_a2dp_then_restarts_bluealsa() {
    let Some(h) = Harness::start(
        "nopcm",
        Fake {
            no_pcm: true,
            ..Fake::default()
        },
    ) else {
        return;
    };
    h.link(LONG, Link::Settling).expect("settling");
    let recovered = h
        .until(LONG, |s| s.recovery.as_deref() == Some("bluealsa_restart"))
        .expect("BlueALSA restarted");
    assert!(recovered.devices[0].connected);
    assert!(h.fake().connects >= 2, "A2DP asked for once more first");
}

/// New codec settings with a headset connected: its link ends cleanly, BlueALSA starts again, the
/// headset is connected again at once (no half set-up link, 9.3.3 on the owner's player).
fn codec_reload_reconnects_cleanly() {
    let Some(h) = Harness::start("reload", Fake::default()) else {
        return;
    };
    h.link(LONG, Link::Ready).expect("ready");
    let tries = h.fake().connects;
    let answer = h.ask(Request::ReloadCodecs);
    assert!(answer.contains("\"ok\":true"), "{answer}");
    h.until(Duration::from_secs(5), |s| {
        s.devices.iter().any(|d| d.ended == Some(Ended::Local))
    })
    .expect("ended cleanly");
    let started = Instant::now();
    h.link(Duration::from_secs(10), Link::Ready).expect("back");
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(h.fake().connects, tries + 1);
}

/// A headset that drops every link within seconds (install 319dab53, 9.3.12: BlueZ `Permission
/// denied`, the headset had lost our key; 26 connects in 2 minutes): three quick drops park it with
/// a message to pair it again, and nothing pages it after that until the owner's tap.
fn quick_drops_park_the_headset() {
    let Some(h) = Harness::start("quick", Fake::default()) else {
        return;
    };
    for drop in 1..=links::QUICK_DROPS {
        h.until(LONG, |s| s.devices.iter().any(|d| d.connected))
            .expect("connected");
        let _ = h.control.send(Control::Drop("org.bluez.Reason.Local"));
        if drop < links::QUICK_DROPS {
            h.until(Duration::from_secs(5), |s| {
                s.devices.iter().all(|d| !d.connected)
            })
            .expect("dropped");
        }
    }
    let parked = h
        .link(Duration::from_secs(5), Link::Parked)
        .expect("parked after quick drops");
    let failure = parked.failure.expect("the owner is told");
    assert!(failure.message.contains("pair them again"), "{failure:?}");
    let tries = h.fake().connects;
    assert_eq!(tries, links::QUICK_DROPS);
    std::thread::sleep(Duration::from_secs(5));
    assert_eq!(h.fake().connects, tries, "no paging after giving up");
    let answer = h.ask(Request::Connect {
        address: ADDRESS.into(),
    });
    assert!(answer.contains("\"ok\":true"), "{answer}");
    h.link(LONG, Link::Ready)
        .expect("the owner's tap connects again");
}

/// The fake's state once `wanted` holds, within `within`.
fn fake_until(h: &Harness, within: Duration, wanted: impl Fn(&Fake) -> bool) -> Option<Fake> {
    let end = Instant::now() + within;
    while Instant::now() < end {
        let fake = h.fake();
        if wanted(&fake) {
            return Some(fake);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

/// #116: a car pairs from its own side. A search makes the player visible and pairable; the
/// car's number is the owner's question; after the owner's yes the car's audio needs no second
/// question; once bonded the car is trusted, the player hidden again and the car used for audio.
fn a_car_pairs_from_its_side() {
    let Some(h) = Harness::start(
        "car",
        Fake {
            stranger: true,
            ..Fake::default()
        },
    ) else {
        return;
    };
    h.until(LONG, |s| s.powered).expect("powered");
    fake_until(&h, Duration::from_secs(5), |f| f.agent.is_some()).expect("agent registered");
    assert!(!h.fake().discoverable, "hidden until the owner searches");
    let answer = h.ask(Request::Scan { on: true });
    assert!(answer.contains("\"ok\":true"), "{answer}");
    fake_until(&h, Duration::from_secs(5), |f| f.discoverable && f.pairable)
        .expect("visible and pairable while searching");
    let _ = h.control.send(Control::Pair);
    let asked = h
        .until(Duration::from_secs(5), |s| s.pairing.is_some())
        .expect("the car's number is asked");
    assert_eq!(asked.pairing.expect("question").display, "Confirm 123456");
    let answer = h.ask(Request::Confirm { accept: true });
    assert!(answer.contains("\"ok\":true"), "{answer}");
    let _ = h.control.send(Control::Authorize(h._bus.address.clone()));
    let authorized = fake_until(&h, Duration::from_secs(5), |f| f.authorized.is_some());
    assert_eq!(
        authorized.and_then(|f| f.authorized),
        Some(true),
        "the owner's yes covers the car's audio"
    );
    let _ = h.control.send(Control::Bond);
    let done = fake_until(&h, Duration::from_secs(5), |f| f.trusted && !f.discoverable)
        .expect("trusted and hidden again");
    assert!(!done.pairable, "Pairable put back");
    h.link(LONG, Link::Ready)
        .expect("the car is used for audio");
}

/// Y2Reborn #217: the owner names the player. BlueZ gets the name as the adapter's Alias and the
/// published state shows it; a name too long is refused and changes nothing; an empty one puts
/// main.conf's back.
fn the_owner_renames_the_player() {
    let Some(h) = Harness::start("rename", Fake::default()) else {
        return;
    };
    h.until(LONG, |s| s.name.as_deref() == Some(SYSTEM_NAME))
        .expect("the standard name is shown");
    let answer = h.ask(Request::DeviceName {
        name: "  Kitchen Y2 ".into(),
    });
    assert!(answer.contains("\"ok\":true"), "{answer}");
    assert_eq!(h.fake().alias.as_deref(), Some("Kitchen Y2"), "trimmed");
    h.until(Duration::from_secs(5), |s| {
        s.name.as_deref() == Some("Kitchen Y2")
    })
    .expect("the new name is published");
    let answer = h.ask(Request::DeviceName {
        name: "x".repeat(33),
    });
    assert!(answer.contains("device_name_too_long"), "{answer}");
    let answer = h.ask(Request::DeviceName {
        name: "Kitchen\nY2".into(),
    });
    assert!(answer.contains("device_name_invalid"), "{answer}");
    assert_eq!(h.fake().alias.as_deref(), Some("Kitchen Y2"), "kept");
    let answer = h.ask(Request::DeviceName { name: "".into() });
    assert!(answer.contains("\"ok\":true"), "{answer}");
    assert_eq!(h.fake().alias, None, "main.conf's name again");
    h.until(Duration::from_secs(5), |s| {
        s.name.as_deref() == Some(SYSTEM_NAME)
    })
    .expect("the standard name is published again");
}

#[test]
fn field_failures_against_a_fake_bluez() {
    if private_bus().is_none() {
        eprintln!("no dbus-daemon on this host: skipped");
        return;
    }
    power_on_adoption_in_progress_and_sleep();
    switched_off_parks_then_returns_by_itself();
    remote_end_one_look_and_owners_disconnect_none();
    refused_codec_falls_back_and_stays_playable();
    codec_change_waits_for_the_link();
    a_change_that_ends_the_link_is_not_repeated();
    a_codec_change_keeps_a_headset_that_came_by_itself();
    missing_stream_asks_a2dp_then_restarts_bluealsa();
    codec_reload_reconnects_cleanly();
    quick_drops_park_the_headset();
    a_car_pairs_from_its_side();
    the_owner_renames_the_player();
}
