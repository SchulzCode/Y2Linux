//! A connected headset becomes ready: its BlueALSA PCM must appear (the A2DP profile is asked for
//! once, then BlueALSA restarts), and its codec is set to the owner's choice before the player
//! opens it, never while a stream is open, and only when it differs from what runs. A codec change
//! that gets no answer leaves the headset as it is and usable (the old "outcome unknown" marker
//! blocked all playback, F25).
//!
//! A change waits until the link is set up (our Connect answered, [`SWITCH_QUIET`] since it came
//! up): sent while BlueZ still opens the stream, it cancels that open and the link ends; the
//! reconnect then met the same change, ≈40 times in a row (Y2Reborn #94, F66). A change that ended
//! a link or failed is not sent again to that headset until the owner picks a codec again.
//!
//! Headphones that connect by themselves can get a BlueALSA stream with 8 channels (a 7.1 map)
//! where the same headphones connected from here get 2. The player cannot open or route that
//! stream, yet it is "connected" on the screen and the sound stays on the speaker. Selecting a
//! codec on it does not repair it (the stream then runs behind real time and plays one ear
//! garbled, measured with AAC and with SBC), a fresh connection from this side does: the headset
//! is disconnected once and connected again ([`Controller::stereo_reconnect`]).
// SPDX-License-Identifier: GPL-2.0-only
use super::{bus::Kind, codec, ladder::Rung, world::SINK, Controller};
use dbus::{
    arg::{PropMap, Variant},
    blocking::{stdintf::org_freedesktop_dbus::Properties, Proxy},
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    time::{Duration, Instant},
};
use y2p_contract::radiod::{Codec, Eligibility};

/// A resolved headset without a PCM this long: ask for A2DP, then restart BlueALSA.
pub const PCM_WAIT: Duration = Duration::from_secs(8);
/// Reconnects for a stream that lists more than two channels, in a row: a headset that gets the
/// same stream again keeps it (nothing else is tried; the output then stays where it is).
pub const WIDE_RECONNECTS: u32 = 1;
const SELECT_DEADLINE: Duration = Duration::from_secs(8);
const SELECT_BUDGET: Duration = Duration::from_secs(12);
const SELECTIONS: u32 = 3;
/// How long a stream may list codecs without the one it runs: BlueZ asks a headset that connected
/// by itself for its codecs only after the stream is there (≈20 ms on AirPods Pro).
const CAPABILITIES_WAIT: Duration = Duration::from_secs(3);
const CAPABILITIES_RETRY: Duration = Duration::from_millis(250);
/// A codec change waits this long after the link came up (9.2.16 changed ≈2.5 s after Connect
/// answered, and those headsets kept their link).
pub const SWITCH_QUIET: Duration = Duration::from_secs(3);
/// While our Connect is still unanswered: look again this often (its answer also wakes the loop).
const CONNECT_POLL: Duration = Duration::from_millis(500);

/// One headset's codec decision for one stream generation and choice.
pub struct Run {
    pub generation: u64,
    pub preference: String,
    pub eligibility: BTreeMap<String, Eligibility>,
    pub candidates: Vec<String>,
    /// Codecs both sides run at 44.1 kHz (else 48 kHz is asked for).
    pub at_44k: BTreeSet<String>,
    pub tried: Vec<String>,
    pub state: &'static str,
    pub reason: Option<String>,
    pub started: Instant,
    /// "Learning": decide again then.
    pub retry_at: Option<Instant>,
    /// "Pending" until the link is set up: look again then.
    pub wait: Option<Instant>,
}

#[derive(Default)]
pub struct Codecs {
    pub runs: BTreeMap<String, Run>,
    /// Codecs this headset refused during this connection.
    pub refused: BTreeMap<String, BTreeSet<String>>,
    /// Selections sent during this connection (at most [`SELECTIONS`]: a headset that always
    /// renegotiates something else must not be asked forever).
    pub selections: BTreeMap<String, u32>,
    /// Headsets whose link a codec change ended or that failed one, with the choice it was for:
    /// they keep what they negotiate until the owner picks a codec again. Kept across reconnects
    /// (everything else here belongs to one connection).
    pub kept: BTreeMap<String, String>,
}

impl Codecs {
    pub fn next(&self) -> Option<Instant> {
        self.runs
            .values()
            .flat_map(|run| [run.retry_at, run.wait])
            .flatten()
            .min()
    }

    /// The link ended: this connection's decisions go; a change it ended during is remembered.
    pub fn ended(&mut self, address: &str) {
        if let Some(run) = self.runs.remove(address) {
            if run.state == "Selecting" {
                self.kept.insert(address.to_owned(), run.preference);
            }
        }
        self.refused.remove(address);
        self.selections.remove(address);
    }

    /// The device is gone from BlueZ (forgotten): nothing about it stays.
    pub fn forget(&mut self, address: &str) {
        self.ended(address);
        self.kept.remove(address);
    }

    pub fn selecting(&self, address: &str) -> bool {
        self.runs
            .get(address)
            .is_some_and(|run| matches!(run.state, "Selecting" | "Pending" | "Learning"))
    }

    /// The decision shown for this headset.
    pub fn shown(&self, address: &str, device: &str) -> Option<Codec> {
        let run = self.runs.get(address)?;
        Some(Codec {
            device: device.to_owned(),
            generation: run.generation,
            preference: run.preference.clone(),
            eligibility: run.eligibility.clone(),
            // "Pending" and "Learning" are this module's own steps before a selection starts.
            state: if matches!(run.state, "Pending" | "Learning") {
                "Selecting"
            } else {
                run.state
            }
            .into(),
            reason: run.reason.clone(),
        })
    }
}

impl Controller {
    /// Bring one connected, set-up audio device to Ready.
    pub(super) fn settle(&mut self, address: &str, now: Instant) {
        let Some(pcm) = self.world.pcm_for(address).cloned() else {
            self.pcm_missing(address, now);
            return;
        };
        let record = self.record(address);
        record.pcm_missing_since = None;
        record.profile_asked = false;
        if self.stereo_reconnect(address, &pcm, now) {
            return;
        }
        let preference = self.desired.policy.preference.clone();
        // The owner picked a codec while this link runs: apply it, even to a headset kept before.
        let changed = self
            .codec
            .runs
            .get(address)
            .is_some_and(|run| run.preference != preference);
        if changed {
            self.codec.kept.remove(address);
        }
        let fresh = self.codec.runs.get(address).is_none_or(|run| {
            run.generation != pcm.generation
                || run.preference != preference
                || run.retry_at.is_some_and(|at| now >= at)
        });
        if fresh
            && !self.in_flight(|k| matches!(k, Kind::Select { address: a, .. } if a == address))
        {
            let run = self.decide(address, &pcm, &preference, changed, now);
            self.codec.runs.insert(address.to_owned(), run);
        }
        let open = self.streams.contains_key(address);
        let waits = self.switch_waits(address, now);
        let sent = self.codec.selections.get(address).copied().unwrap_or(0);
        let Some(run) = self.codec.runs.get_mut(address) else {
            return;
        };
        // A change the open stream held back is made once it closed.
        if !matches!(run.state, "Pending" | "Deferred") {
            return;
        }
        if open {
            run.state = "Deferred";
            run.reason = Some("stream_open".into());
            return;
        }
        run.wait = waits;
        if waits.is_some() {
            return;
        }
        let current = pcm.codec.clone().unwrap_or_default();
        let next = run
            .candidates
            .iter()
            .find(|c| !run.tried.contains(c))
            .cloned();
        match next {
            Some(codec)
                if codec != current
                    && sent < SELECTIONS
                    && now.duration_since(run.started) < SELECT_BUDGET =>
            {
                run.tried.push(codec.clone());
                run.state = "Selecting";
                let rate = if run.at_44k.contains(&codec) {
                    44_100
                } else {
                    48_000
                };
                self.codec.selections.insert(address.to_owned(), sent + 1);
                self.select(address, &pcm, &codec, rate);
            }
            _ => run.state = "Settled",
        }
    }

    /// The stream lists more than two channels (headphones that connected by themselves): end the
    /// link and connect again from here, once. Only for a headset this daemon keeps connected (it
    /// reconnects it itself: episode `Lost(Local)`), not while a
    /// stream is open, and not before the link is set up. Whether a reconnect was started.
    fn stereo_reconnect(
        &mut self,
        address: &str,
        pcm: &super::world::PcmObs,
        now: Instant,
    ) -> bool {
        let wide = not_stereo(pcm);
        let wait = if wide {
            self.switch_waits(address, now)
        } else {
            None
        };
        let keeps = self.target().as_deref() == Some(address);
        let open = self.streams.contains_key(address);
        let record = self.record(address);
        record.wide_at = None;
        if !wide {
            record.wide_reconnects = 0;
            return false;
        }
        if record.wide_reconnects >= WIDE_RECONNECTS || !keeps || open {
            return false;
        }
        if let Some(at) = wait {
            record.wide_at = Some(at);
            return true;
        }
        record.wide_reconnects += 1;
        let reconnects = record.wide_reconnects;
        self.journal.event(
            "stereo_reconnect",
            serde_json::json!({
                "address": address, "channels": pcm.channels, "codec": pcm.codec,
                "attempt": reconnects
            }),
        );
        self.record(address).stereo_reconnect = true;
        self.disconnect(address, Kind::Cancel(address.to_owned()));
        true
    }

    /// When a codec change may be sent: not while our Connect is unanswered, nor in the first
    /// [`SWITCH_QUIET`] of the link (None = now).
    fn switch_waits(&self, address: &str, now: Instant) -> Option<Instant> {
        let quiet = self
            .records
            .get(address)
            .and_then(|record| record.connected_since)
            .map(|at| at + SWITCH_QUIET)
            .filter(|at| *at > now);
        let connecting = self.in_flight(|k| matches!(k, Kind::Connect(a) if a == address));
        quiet.or_else(|| connecting.then(|| now + CONNECT_POLL))
    }

    /// What should run on this stream: settled at once when it already does, when Auto would run
    /// it anyway, or when a change already ended this headset's link (unless the owner just chose).
    fn decide(
        &mut self,
        address: &str,
        pcm: &super::world::PcmObs,
        preference: &str,
        changed: bool,
        now: Instant,
    ) -> Run {
        let mut run = Run {
            generation: pcm.generation,
            preference: preference.to_owned(),
            eligibility: BTreeMap::new(),
            candidates: Vec::new(),
            at_44k: BTreeSet::new(),
            tried: Vec::new(),
            state: "Settled",
            reason: None,
            started: now,
            retry_at: None,
            wait: None,
        };
        let (mutual, at_44k, runtime) = match self.capabilities(&pcm.object) {
            Ok(found) => found,
            Err(reason) => {
                run.reason = Some(reason);
                return run;
            }
        };
        if pcm
            .codec
            .as_deref()
            .is_some_and(|c| !mutual.iter().any(|m| m == c))
        {
            let first = self
                .codec
                .runs
                .get(address)
                .filter(|r| r.generation == pcm.generation && r.state == "Learning")
                .map_or(now, |r| r.started);
            if now.duration_since(first) < CAPABILITIES_WAIT {
                run.started = first;
                run.state = "Learning";
                run.reason = Some("capabilities_pending".into());
                run.retry_at = Some(now + CAPABILITIES_RETRY);
                return run;
            }
        }
        let inventory = codec::inventory(&self.root);
        let experimental = codec::experimental(&self.root, &inventory);
        run.eligibility = codec::eligibility(&inventory, &runtime, &mutual, experimental);
        run.at_44k = at_44k;
        let context = codec::context(&self.root, address, pcm.codec.as_deref());
        let refused = self.codec.refused.get(address).cloned().unwrap_or_default();
        run.candidates = codec::candidates(preference, &run.eligibility, &context)
            .into_iter()
            .filter(|c| !refused.contains(c))
            .collect();
        if let Some(observed) = pcm.codec.as_deref() {
            codec::remember(&self.root, address, Some(observed), None);
        }
        let current = pcm.codec.clone().unwrap_or_default();
        if !changed {
            if self
                .codec
                .kept
                .get(address)
                .is_some_and(|p| p == preference)
            {
                run.reason = Some("kept_after_failed_change".into());
                return run;
            }
            if codec::auto_keeps(preference, &current, &run.candidates) {
                return run;
            }
        }
        if run.candidates.first().is_some_and(|best| *best != current) {
            run.state = "Pending";
        }
        run
    }

    /// What both sides support (stereo at 44.1 or 48 kHz) and what BlueALSA runs.
    fn capabilities(&self, object: &str) -> Result<Capabilities, String> {
        let (Some(bus), Some(owner)) = (&self.bus, self.world.bluealsa.as_deref()) else {
            return Err("bluealsa_unavailable".into());
        };
        let proxy = Proxy::new(owner, object, Duration::from_millis(800), &bus.channel);
        let (codecs,): (HashMap<String, PropMap>,) = proxy
            .method_call("org.bluealsa.PCM1", "GetCodecs", ())
            .map_err(|_| "codec_capabilities_unanswered".to_string())?;
        let mutual = codecs
            .iter()
            .filter(|(_, p)| stereo_at_common_rate(p))
            .map(|(name, _)| name.clone())
            .collect();
        let at_44k = codecs
            .iter()
            .filter(|(_, p)| rates(p).contains(&44_100))
            .map(|(name, _)| name.clone())
            .collect();
        let manager = Proxy::new(
            owner,
            "/org/bluealsa",
            Duration::from_millis(800),
            &bus.channel,
        );
        let runtime: Vec<String> = manager
            .get("org.bluealsa.Manager1", "Codecs")
            .map_err(|_| "codec_runtime_unanswered".to_string())?;
        Ok((mutual, at_44k, codec::runtime_codecs(&runtime)))
    }

    fn select(&mut self, address: &str, pcm: &super::world::PcmObs, codec: &str, rate: u32) {
        let (Some(owner), Some(bus)) = (self.world.bluealsa.clone(), self.bus.as_mut()) else {
            return;
        };
        let mut properties = PropMap::new();
        properties.insert("Channels".into(), Variant(Box::new(2u8)));
        properties.insert("Rate".into(), Variant(Box::new(rate)));
        let kind = Kind::Select {
            address: address.to_owned(),
            codec: codec.to_owned(),
        };
        let target = (
            owner.as_str(),
            pcm.object.as_str(),
            "org.bluealsa.PCM1",
            "SelectCodec",
        );
        bus.call(target, (codec, properties), kind, SELECT_DEADLINE);
        self.journal.event(
            "codec_select",
            serde_json::json!({"address": address, "codec": codec, "from": pcm.codec}),
        );
    }

    /// The answer to a SelectCodec.
    pub(super) fn selected(&mut self, address: &str, codec: &str, answer: super::bus::Answer) {
        let Some(run) = self.codec.runs.get_mut(address) else {
            return;
        };
        match answer {
            Ok(()) => {
                run.state = "Settled";
                run.reason = None;
                codec::remember(&self.root, address, Some(codec), None);
            }
            Err((name, message)) if refused(&name, &message) => {
                run.state = "Pending";
                self.codec
                    .refused
                    .entry(address.to_owned())
                    .or_default()
                    .insert(codec.to_owned());
                codec::remember(&self.root, address, None, Some(codec));
            }
            Err((name, _)) => {
                // No answer or an unexpected one (BlueZ's EIO when the change broke the stream,
                // #94): leave the headset as it runs and do not ask it again. Nothing blocks playback.
                run.state = "Failed";
                run.reason = Some(format!("codec_change_{name}"));
                self.codec
                    .kept
                    .insert(address.to_owned(), run.preference.clone());
            }
        }
        self.journal.event(
            "codec_answer",
            serde_json::json!({
                "address": address, "codec": codec,
                "state": self.codec.runs.get(address).map(|r| r.state)
            }),
        );
    }

    /// Resolved without a PCM: A2DP once, then BlueALSA again.
    fn pcm_missing(&mut self, address: &str, now: Instant) {
        let Some(path) = self.world.devices.get(address).map(|d| d.path.clone()) else {
            return;
        };
        let record = self.record(address);
        let since = *record.pcm_missing_since.get_or_insert(now);
        if now.duration_since(since) < PCM_WAIT {
            return;
        }
        if !record.profile_asked {
            record.profile_asked = true;
            record.pcm_missing_since = Some(now);
            let Some(owner) = self.world.bluez.clone() else {
                return;
            };
            if let Some(bus) = self.bus.as_mut() {
                let target = (
                    owner.as_str(),
                    path.as_str(),
                    "org.bluez.Device1",
                    "ConnectProfile",
                );
                let kind = Kind::Profile;
                bus.call(target, (SINK,), kind, super::links::CONNECT_DEADLINE);
            }
            self.journal
                .event("a2dp_profile_asked", serde_json::json!({}));
            return;
        }
        record.pcm_missing_since = None;
        record.profile_asked = false;
        if let Some(Rung::BluealsaRestart | Rung::BluezRestart) =
            self.recover(Rung::BluealsaRestart, Some(address))
        {
            self.dirty = true;
        }
    }
}

/// Mutual codecs, those of them at 44.1 kHz, and BlueALSA's running codecs.
type Capabilities = (Vec<String>, BTreeSet<String>, Vec<String>);

fn numbers(p: &PropMap, key: &str) -> Vec<u64> {
    p.get(key)
        .and_then(|v| v.0.as_iter())
        .map(|values| values.filter_map(|v| v.as_u64()).collect())
        .unwrap_or_default()
}

fn rates(p: &PropMap) -> Vec<u64> {
    numbers(p, "Rates")
}

/// BlueALSA lists the stream with a channel count other than 2 (8 with a 7.1 map: AAC headphones
/// that connected by themselves, BlueALSA 5.0.0).
fn not_stereo(pcm: &super::world::PcmObs) -> bool {
    matches!(pcm.channels, Some(channels) if channels != 2)
}

/// Stereo at 44.1 or 48 kHz (what the player sends).
fn stereo_at_common_rate(p: &PropMap) -> bool {
    numbers(p, "Channels").contains(&2) && rates(p).iter().any(|r| [44_100, 48_000].contains(r))
}

/// SelectCodec refused this codec for this headset (try the next one).
fn refused(name: &str, message: &str) -> bool {
    matches!(
        name,
        "org.freedesktop.DBus.Error.NotSupported" | "org.freedesktop.DBus.Error.InvalidArgs"
    ) || (name == "org.freedesktop.DBus.Error.Failed"
        && matches!(
            message,
            "SEP codec not available" | "SEP codec not supported" | "No BlueZ SEP support"
        ))
}
