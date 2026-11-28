//! The radio daemon (`y2-radiod`): the one owner of Bluetooth and of the shared radio core's power.
//! Clients (Reborn, `y2-radio`, the sleep helper, the Wi-Fi recovery) say what they want with a
//! [`Request`]; the daemon answers each with a [`Reply`] and, to subscribers, sends every new [`State`].
//! One JSON object per line in both directions ([`Message`] from the daemon).
//!
//! Requests describe a wanted state, never a step: a Connect while the daemon pages another headset
//! replaces that wish instead of being refused as busy (docs/planning/BLUETOOTH-REIMPLEMENTATION-PLAN.md).
// SPDX-License-Identifier: MIT
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `State::schema`.
pub const SCHEMA: &str = "org.y2linux.radiod/v1";

/// The longest name the owner can give the player ([`Request::DeviceName`]), in UTF-8 bytes: car
/// screens and phone lists cut longer names short.
pub const NAME_MAX_BYTES: usize = 32;

/// The two radios of the shared core.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Radio {
    Wifi,
    Bluetooth,
}

/// What a client asks for. Addresses are Bluetooth device addresses (`AA:BB:CC:DD:EE:FF`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Send the current state now and every new one after it on this connection.
    Subscribe,
    /// Answer with the current state once.
    State,
    /// Bluetooth on or off; `persist` keeps it across restarts (the owner's switch).
    BluetoothPower { on: bool, persist: bool },
    /// Look for new devices for a while (`on`), or stop looking.
    Scan { on: bool },
    /// Pair with a device found by a scan, then connect it.
    Pair { address: String },
    /// Use this device for audio: connect it now and reconnect it when it comes back.
    Connect { address: String },
    /// Stop using this device until it is asked for again.
    Disconnect { address: String },
    /// Remove the pairing.
    Forget { address: String },
    /// The answer to the pairing question shown (`State::bluetooth.pairing`).
    Confirm { accept: bool },
    /// The owner's codec choice for every headset: "Auto", "SBC", "SBC-XQ", "AAC", "aptX", "aptX-HD",
    /// "LDAC". Applied when a headset connects, or at the next stop of its stream.
    CodecPreference { preference: String },
    /// The name cars, phones and headphones list this player by (BlueZ keeps it across restarts).
    /// Empty: back to the standard name. At most [`NAME_MAX_BYTES`], no control characters.
    DeviceName { name: String },
    /// The headphones' own volume (AVRCP absolute volume, 0..=127).
    Volume { address: String, volume: u8 },
    /// Whether this paired device connects on its own (Y2Reborn #78: a TV that is always on).
    AutoConnect { address: String, on: bool },
    /// The player opened (`open`) or closed its stream to this headset: no codec change while open.
    Stream { address: String, open: bool },
    /// Restart BlueALSA and BlueZ now (what switching Bluetooth off and on does after a failure).
    RestartBluetooth,
    /// The owner's codec settings changed (`codec-settings`): BlueALSA starts again with them as
    /// soon as no stream is open (headphones reconnect by themselves), no player restart.
    ReloadCodecs,
    /// Wi-Fi on or off; `persist` keeps it across restarts.
    WifiPower { on: bool, persist: bool },
    /// Wi-Fi off and on once (the Wi-Fi recovery's last step before a core reset).
    WifiRestart,
    /// Sleep: take this radio down and answer when it is down.
    Quiesce { radio: Radio },
    /// Wake: bring this radio back as the owner left it.
    Restore { radio: Radio },
}

/// The answer to one request.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    pub ok: bool,
    /// Why not (`ok` false), as a stable reason the client can act on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Reply {
    pub fn done() -> Reply {
        Reply {
            ok: true,
            reason: None,
        }
    }
    pub fn refused(reason: &str) -> Reply {
        Reply {
            ok: false,
            reason: Some(reason.into()),
        }
    }
}

/// One line from the daemon.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Message {
    Reply(Reply),
    State(Box<State>),
}

/// Everything the daemon knows, published whole after each change.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub schema: String,
    /// Grows with every published state of this daemon run.
    pub sequence: u64,
    pub bluetooth: Bluetooth,
    pub wifi: WifiPower,
}

/// Where the Bluetooth service is on its way up, or why it is not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Service {
    /// BlueZ or BlueALSA is not there yet.
    #[default]
    Starting,
    /// The adapter answers; devices can be used.
    Ready,
    /// A recovery step runs (`Bluetooth::recovery` says which).
    Recovering,
    /// Every recovery step failed: switching Bluetooth off and on tries again.
    Failed,
    /// Asleep (the sleep helper took it down).
    Quiesced,
}

/// A device's link, as the daemon drives it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Link {
    /// Not connected and not wanted now.
    #[default]
    Idle,
    /// Wanted; waiting for it to connect on its own (after power-on or a wake) or for the next try.
    Waiting,
    /// The daemon is connecting it.
    Connecting,
    /// Connected; its audio stream or codec is being set up.
    Settling,
    /// Connected and its audio stream can be opened (`Device::generation`).
    Ready,
    /// Wanted, but every try failed: tried again when it connects on its own, at a wake or on a tap.
    Parked,
    /// Pairing.
    Pairing,
}

/// Why a device's last connection ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ended {
    /// The headset went out of range or stopped answering (link supervision timeout).
    Timeout,
    /// The headset ended it (switched off, another source).
    Remote,
    /// This player ended it without being asked (a failed stream set-up).
    Local,
    /// This player's sleep took Bluetooth down.
    Sleep,
    /// The owner disconnected it, switched Bluetooth off, or forgot it.
    User,
    /// No reason was given.
    Unknown,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Device {
    /// BlueZ's object (`/org/bluez/hci0/dev_…`).
    pub path: String,
    pub address: String,
    pub name: String,
    pub paired: bool,
    pub bonded: bool,
    pub trusted: bool,
    pub connected: bool,
    /// It offers A2DP audio (a sink).
    pub audio: bool,
    pub link: Link,
    /// Connects on its own when it comes back (Y2Reborn #78).
    pub auto_connect: bool,
    /// The headphones' own volume (0..=127), once they report one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<u8>,
    /// BlueZ's A2DP transport object of this device.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<Ended>,
}

/// A BlueALSA PCM.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Pcm {
    pub object: String,
    pub device: String,
    pub transport: String,
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running: Option<bool>,
    /// Changes whenever this PCM's stream changes (codec, rate, format, a reconnect, a BlueALSA
    /// restart): an open stream of another generation is stale.
    pub generation: u64,
}

/// Whether a codec can be used with the connected headset, and why.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Eligibility {
    pub compiled_locally: bool,
    pub runtime_enabled: bool,
    pub mutually_usable: bool,
    pub distribution_approved: bool,
    pub platform_qualified: bool,
    pub auto_eligible: bool,
    pub experimental_eligible: bool,
}

/// The codec choice for the connected headset.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Codec {
    pub device: String,
    pub generation: u64,
    /// The owner's choice ([`Request::CodecPreference`]).
    pub preference: String,
    pub eligibility: BTreeMap<String, Eligibility>,
    /// "Settled", "Selecting", "Deferred" (the stream is open), "Failed".
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// A pairing question for the owner.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Pairing {
    pub device: String,
    pub display: String,
}

/// Looking for devices.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Scan {
    #[default]
    Idle,
    Scanning,
    Complete {
        found: usize,
    },
    Failed {
        message: String,
    },
}

/// The last thing that went wrong, for the owner and for error reports.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Failure {
    /// A stable name (`connect_failed`, `pair_failed`, `bluealsa_restart`, …).
    pub reason: String,
    /// What to show.
    pub message: String,
    /// The device it was about, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    /// The `sequence` of the state that first carried it (a client shows each failure once).
    pub sequence: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Bluetooth {
    pub service: Service,
    /// The owner wants Bluetooth on.
    pub wanted: bool,
    /// The adapter is there.
    pub available: bool,
    pub powered: bool,
    pub discovering: bool,
    /// BlueALSA is running.
    pub bluealsa: bool,
    pub devices: Vec<Device>,
    pub pcms: Vec<Pcm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pairing: Option<Pairing>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec: Option<Codec>,
    /// The owner's codec choice.
    pub preference: String,
    /// The name other devices list this player by, while the adapter is there (an older daemon
    /// does not send it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub scan: Scan,
    /// The recovery step running or last run (`reconnect`, `cancel_page`, `power_cycle`,
    /// `bluealsa_restart`, `bluez_restart`, `core_reset`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<Failure>,
}

/// Wi-Fi's power as the daemon holds it (association stays wpa_supplicant's).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WifiPower {
    /// The owner (or a runtime request) wants it on.
    pub wanted: bool,
    /// The radio is unblocked.
    pub on: bool,
    /// Held off for now: a wake brings Bluetooth headphones back first.
    pub held: bool,
}

impl Bluetooth {
    /// The reconnect state as the `status` command has always named it (`bluetooth.reconnect.state`).
    pub fn reconnect_state(&self) -> &'static str {
        if self.service == Service::Starting {
            "Unavailable"
        } else if !self.powered {
            "Off"
        } else if self.devices.iter().any(|d| d.connected && d.audio) {
            "Connected"
        } else if self.devices.iter().any(|d| d.link == Link::Connecting) {
            "Connecting"
        } else if self.devices.iter().any(|d| d.link == Link::Parked) {
            "Exhausted"
        } else {
            "Waiting"
        }
    }
}

#[cfg(test)]
#[path = "radiod_tests.rs"]
mod radiod_tests;
