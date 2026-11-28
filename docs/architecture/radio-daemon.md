# The radio daemon (`y2-radiod`)

Since Beta 9.3 one process owns Bluetooth and both radios' power. It replaced five owners that coordinated through
lock and intent files (Reborn's D-Bus worker, `y2-platform bluetooth-power`, `y2-bt-reconnect`, the sleep helper's
radio steps, the kernel's own recovery). Why and how it was planned:
[BLUETOOTH-REIMPLEMENTATION-PLAN.md](../planning/BLUETOOTH-REIMPLEMENTATION-PLAN.md).

## Pieces

| Piece | Source | Job |
|---|---|---|
| Arbiter (main thread) | `platform/crates/y2-radiod/src/arbiter.rs` | Wi-Fi power (per-device rfkill, never the type, F37), the wake order (Wi-Fi waits up to 12 s for the headset connected before the sleep), the codec ceiling while Wi-Fi is on (`/run/y2/bt-codec-ceiling`), Wi-Fi power save following Bluetooth streaming, the published state |
| Bluetooth controller (one thread) | `src/bt/` | BlueZ and BlueALSA over the system D-Bus; every long call asynchronous with its own deadline |
| Socket server | `src/server.rs` | `/run/y2/radiod.sock`, root only, one JSON object per line (`y2p_contract::radiod`) |
| `y2-radiod ctl` | `src/ctl.rs` | what `y2-radio` (boot, sleep, shutdown) asks; without the daemon it switches the radios directly |

## Contract

Requests (`Request`) describe a wish, never a step, and are answered at once (`Reply`); only a sleep's power-down is
answered when it is done. Subscribers get every new `State`; `/run/y2/radiod-state.json` holds the latest. Reborn
keeps one `Status` built from it (`Y2Reborn/crates/reborn-platform/src/bluetooth.rs`); a connected headset that is not
`Ready` yet is `settling` and not playable. Before opening BlueALSA's stream Reborn takes a `Stream` lease; no codec
changes while one is open.

## Bluetooth controller

- **Episodes** (`bt/links.rs`): Bluetooth on or a wake: 3 s for headphones to come back on their own, then three
  tries (3 s, 8 s apart); the owner's tap: at once, three tries; a lost link (timeout, local, unknown): after 2 s,
  three tries; a remote end (switched off): one look after 4 s; the owner's disconnect or a sleep: none. A spent
  episode parks the headset until it connects by itself, a wake, Bluetooth on, or a tap.
- **Stuck connects**: InProgress for 12 s, connected without a stream for 8 s, or no answer within 35 s, is
  cancelled (a Disconnect ends BlueZ's page); a second cancel in one episode switches the adapter off and on. A link
  counts as set up when BlueZ resolved its services *or* BlueALSA has its stream: BlueZ looks up no services for a
  known headset that connects by itself, so its `ServicesResolved` stays false for that whole connection. The 8 s
  count from when the set-up went missing (`Record::unset_since`), not from the connect, and not while a codec
  change runs: a change briefly removes BlueALSA's stream, and counting from the connect cancelled such a headset at
  once (owner's AirPods, 2026-11-04).
- **Recovery ladder** (`bt/ladder.rs`): cancel page → power cycle → BlueALSA restart → BlueZ restart, three each in
  ten minutes; then the service is Failed until the owner switches Bluetooth off and on (which restarts BlueALSA and
  BlueZ and starts every budget over).
- **Supervision** (`bt/supervise.rs`): bluetoothd (its init script) and bluealsad (started here with the build's
  codec arguments, `--keep-alive=0`, `--io-rt-priority=1` (its encoder threads run before the player's other work)
  and `--a2dp-force-audio-cd`: 44.1 kHz for every headset and codec, 48 kHz only
  where a headset refuses it) start again when they end, five times in ten minutes. Since 9.3.16 Reborn owns the
  stream's life like a phone's mixer: while the owner plays, its PCM stays open and is written with music or silence
  (never dropped, drained or prepared again inside BlueALSA: an underrun there is a stop and restart in a started
  stream, after which AirPods stay silent, F69/Y2Reborn #99); a skip, seek or song end changes only what is written,
  a pause writes silence for 3 s and then closes the PCM (AVDTP Suspend), so a Play after that starts a fresh stream.
  With no gap left in a started stream, BlueALSA patches 0007 (RTP timestamp over a gap), 0008 (refill faster than
  real time after a gap) and 0005 (learned start rate) were dropped: BlueALSA paces as upstream does. Builds 210–219 kept the stream started for 900 s after a
  pause: a Suspend sent 10 s into an idle stream had gone unanswered one time in three. But headphones are never
  left in a started stream without packets by a phone, and AirPods Pro went silent after every pause or resume gap
  longer than their buffer inside such a stream (packets ran on at 47/s, btmon 2026-11-04; the same as F43 in
  Beta 6) and stopped answering the baseband 2 s into a pause (supervision timeout 20 s later). BlueALSA patches
  0007 and 0008 came from that design and went with it (9.3.16).
- **Quick drops** (`bt/links.rs` `quick_drop`, `bt/reconcile.rs`): a link that ends within 10 s of coming up three
  times in a row parks the headset with "Couldn't stay connected … pair them again" (journal `reconnect_given_up`);
  a headset that had lost our key reconnected 26 times in 2 minutes (install 319dab53, BlueZ `Permission denied`),
  each drop starting a new episode. Bluetooth on, a wake or the owner's tap give it its tries again. Journal events
  carry the device address (reports pseudonymise it).
- **Codecs** (`bt/codec.rs`, `bt/settle.rs`): the policy Reborn had (Auto: AAC, aptX, SBC; LDAC and aptX HD only
  when chosen; history of refusals). SelectCodec only when the negotiated codec differs, only while no stream is
  open, at most three per connection; an unanswered one leaves the headset usable. Auto keeps the AAC, aptX or SBC a
  headset connected with (it only moves off LDAC/aptX HD); a chosen codec is applied once our Connect has answered and
  the link has been up 3 s (`SWITCH_QUIET`): sent earlier it cancels BlueZ's AVDTP Open and ends the link, which
  looped ≈40 times in Y2Reborn #94. A change that failed or ended a link is not sent to that headset again until the
  owner picks a codec again (`Codecs::kept`, survives reconnects). A stream whose codec list lacks the
  codec it runs waits up to 3 s for the list (BlueZ asks a headset that connected by itself only after the stream). New codec settings
  (`codec-settings`) restart BlueALSA once nothing plays (`ReloadCodecs`).
- **Pairing agent** (`bt/agent.rs`): `DisplayYesNo`; the owner's yes/no, refused after 60 s; Pairable for 90 s.
  A search (`Scan`) also makes the player Discoverable and Pairable for 120 s (BlueZ's own timeouts end
  both if the daemon stops), so a device that pairs from its own side finds it: a car's "add device"
  list (#116). Its number is the owner's question; after the owner's yes, that device's
  `AuthorizeService` is answered without a second question, and once BlueZ bonded it the daemon
  trusts it, hides the player again and uses it for audio like headphones paired here
  (`adopt_welcomed`). Bluetooth off, a sleep or a pairing of our own end the visibility early.
- **The player's name** (`bt/requests.rs` `rename`, Y2Reborn #217): `DeviceName` sets the adapter's `Alias`, the
  name cars, phones and headphones list the player by (trimmed, at most `radiod::NAME_MAX_BYTES`, no control
  characters; empty puts back main.conf's `Name=Y2Linux`). BlueZ keeps it in the adapter's `settings` file, so it
  outlives restarts and system updates; the full and Bluetooth-bond resets clear it. `State.bluetooth.name` shows
  the Alias while the adapter is there. A device paired before a rename may keep showing the name it learned.

## Records

| Record | Writer | Readers |
|---|---|---|
| `/data/bluetooth/enabled`, `/data/network/enabled` | daemon (owner's switches) | daemon, sleep helper |
| `/data/bluetooth/preferred-audio` | daemon (the trusted headset that connected last) | daemon |
| `/data/bluetooth/radiod.json` | daemon (codec choice, devices that connect only when asked, #78) | daemon, error reports |
| `/data/bluetooth/codec-history.json` | daemon | daemon |
| `/data/system/platform/bluetooth-journal.jsonl` | daemon (state changes, calls, disconnect reasons, recovery) | error reports |
| `/run/y2/radiod-state.json` | daemon | service daemon (coexistence, Wi-Fi power save), error reports |
| `/data/bluetooth/bluez/<adapter>/settings` `Alias=` | BlueZ, when the daemon sets the player's name | BlueZ, error reports (to replace the name) |

`status` asks the daemon on its socket for `bluetooth.reconnect`. BlueZ's and BlueALSA's objects and the encoder
statistics in `status` still come from `y2-bt-observe` (read-only).

## The stream under a weak link or a late client (2026-11-10)

Measured on a player with AirPods Pro (btmon): on the desk the stream is exact (a packet every 23.2 ms, no
underrun, the pacing clock within 11 ppm); with the player moved away the link stalled up to 2.88 s, and
BlueALSA then sent everything it had missed as fast as the link took it (6.1 s of audio in 3 s). Headphones
answer that with a dropout and fast playback (Y2Reborn #87), and the player sees nothing, since the
back-pressure only reaches the client. Three BlueALSA and platform rules follow:

- **Bounded catch-up** (`buildroot/patches/bluez-alsa/0009`, `src/shared/y2-catchup.h`): at most 200 ms of
  audio is owed after a stall, sent at no more than 4/3 of real time; the rest is forgotten (the music
  continues from where it stopped). On time, or late by ordinary jitter, nothing changes. Each such stall
  is one warning in the system log (`Y2 stall: stream N ms behind real time`; bluealsad runs with
  `--syslog` since 9.3.34), which `tools/reports/summary.py` counts.
- **Silence while a client is late** (`0010`, `src/y2-filler.h`): once a client has played, an A2DP source
  stream that gets no audio for 100 ms is fed silence until audio returns, the client drains or closes. A
  started stream therefore always carries packets, whatever the player's own audio thread does.
- **Bluetooth keeps off the Wi-Fi channel** (`y2-bt-afh`, service daemon `daemon/channels.rs`): while Wi-Fi is
  connected on 2.4 GHz the controller is told that network's channels (AFH host channel classification) and
  told again every 30 s; `clear` when Wi-Fi leaves. On the player the link left all 23 channels of the
  router's within 2 s (before: 66-70 of 79 in use, 4 of the router's left out).

Reborn's audio thread runs at the same real-time priority as BlueALSA's encoder threads, and a Bluetooth
start waits 8 s (BlueZ gives headphones 6 s to answer a stream start).
