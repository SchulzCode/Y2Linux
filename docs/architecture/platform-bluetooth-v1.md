# Bluetooth platform v1 contract

CONSYS/HCI, BlueZ 5.87 and BlueALSA 5.0.0 remain the control/transport/encoder
owners. Reborn retains FFmpeg decode/DSP and one final conversion. Pairing trust,
Rate/Format observation and transport generation invalidation were already fixed
by later Reborn source; the older codec audit does not supersede those fixes.

`y2-bt-observe` makes bounded, read-only native GIO D-Bus queries. It never activates
a radio or opens/selects a transport. It addresses unique daemon owners and checks
owners again before publishing; owner replacement invalidates the sample.
`y2-status bluetooth` normalizes adapter, paired/trusted/connected peer, manager
runtime codecs, exact negotiated codec and PCM Format/Rate/Channels. S24_LE means
24 valid bits in a 32-bit container. Selected/requested preferences are separate
from observation. Unsupported or incomplete PCM is degraded, never assumed S16.

BlueALSA GetCodecs reports a subset filtered by local enabled endpoints and
remote capabilities. It is labelled mutually_usable; raw remote_advertised remains
null. Neither live encoder bitrate nor radio packet loss is fabricated. PCM
connection Sequence, object and owner are exposed; clients opening PCM must also
consume ObjectManager removal and owner signals for per-open lifetime, as Reborn
already does. A snapshot cannot prove no removal occurred between snapshots.

The image's codec inventory is generated from the configured, built BlueALSA
source, with pinned source/configuration hashes. It fails packaging on an
unexpected optional encoder. Build, runtime, remote/mutual, distribution and
qualification states are separate. Current normal image: conformant SBC high
quality, no XQ or optional encoder. All integrated codec physical qualification
is still absent. Production Auto must not promote an unqualified codec.

Reborn exports /org/reborn/player and registers it with BlueZ Media1. A bounded
eight-command queue accepts calls from the current BlueZ unique owner only.
Play/Pause resolve idempotently against the current AppModel; Next/Previous and
PlayPause use existing Actions. Playback status and bounded metadata are a
projection of that model, not another player authority. PropertiesChanged reports
actual model changes; requesting Pause does not claim playback has already paused.
The registration is recreated when the BlueZ owner changes. No volume or gain
policy is introduced. Physical AVRCP and peer interoperability remain PHYSICAL_GATE.

API basis: pinned BlueALSA 5.0.0 docs/org.bluealsa.PCM1.7.rst and
src/bluealsa-dbus.c (GetCodecs filtering); pinned BlueZ 5.87 profiles/audio/media.c;
[BlueZ Media API](https://github.com/bluez/bluez/blob/master/doc/org.bluez.Media.rst).
Optional codec provenance/distribution and Auto/reconnect implementation are
tracked separately in the completion ledger; an audit is not a release approval.

Automatic Device connection remains solely owned by y2-bt-reconnect. One episode
allows at most two 10-second calls within a 25-second start window; successful
connected observation for 30 seconds resets a later link-loss episode. Same-boot
budget, explicit disconnect inhibition and unknown outstanding calls survive
helper restart. A local/client timeout inhibits further automatic requests.
A new explicit connect/power-on intent can re-arm; periodic polling cannot.
Reborn and y2-radio hold a common Linux flock through each user operation.
Pending intents inhibit recovery even if that client dies; only an observed
successful explicit operation re-arms. No pairing is automatic. Runtime records
are private, boot-bound and status rejects stale observations.

Linux lock interoperability is covered by an actual competing file-descriptor
lock test and the pinned Rust implementation's [Unix flock semantics](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock).
This policy is HOST_VALIDATED; headset disappearance, daemon restart, intentional
disconnect and reconnect after long absence remain owner qualification cases.
