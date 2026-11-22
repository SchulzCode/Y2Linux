# Y2Linux 1.0 Beta 9.3.37: even fewer wakeups while music plays with the screen off

Release `1.0.0-beta.9.3.37`, Y2Reborn `1.0.0-beta.9.3.37`, kernel `6.18.56-y2linux-1.0.0-beta.9.3.37`, update
sequence 17700, root `2026.08-platform-v1.295`. Published as GitHub release `baseline-272` (build 272; test builds
270 and 271 came before it on the developer's player). Previous release: [Beta 9.3.36](Y2-LINUX-1.0-BETA-9.3.36.md).

This report is written for people who use the player. A short technical appendix is at the end.

## In one paragraph

Beta 9.3.36 let three of the four cores switch off while music plays with the screen dark
([Y2Reborn #176](https://github.com/SchulzCode/Y2Reborn/issues/176)). This release follows up: the player was profiled
on wired output and on AirPods, and five places that woke the processor more often than needed were changed. With
the screen off, the processor now wakes about 70 % less often than on Beta 9.3.35. Nothing changes in what you hear
or in any setting.

## What changed

- **Reborn waits longer with the screen off.** While music plays in the dark, Reborn's main loop looks around twice a
  second instead of twenty times. Buttons, Bluetooth and the headphone controls still wake it at once; during the
  last seconds of a sleep timer it goes back to quick steps so the fade-out stays smooth.
- **The headphone output asks for music less often.** It now takes music in blocks of 23 ms instead of 12 ms, with
  the same amount kept in reserve, so volume and pause react as quickly as before.
- **The decoder hands over bigger pieces.** MP3, AAC and Opus came one tiny frame at a time (about 40 a second); the
  decoder now fills a whole block. The sound is the same, sample for sample.
- **The decoder rests while it is ahead.** It waits about one block when the queue is full, and a stop, skip or seek
  wakes it at once.
- **The output waits one whole period.** Its wait was a little shorter than one period of audio, so it woke twice
  per period. Now once.

## How it was tested

- **Tests**: the platform's full check, Reborn's tests and the tests of the release images passed. New tests: the
  main loop's wait in dark playback and near a sleep-timer fade, whole decoder blocks for MP3, AAC and Opus with
  positions in order, the decoder woken early from a full-queue wait, and the output's wait for one period.
- **On the developer's player** (build 272, screen off, one core, about 5 minutes per run, one run each, on USB):

  | | Beta 9.3.35 | Beta 9.3.36 | Beta 9.3.37 |
  | --- | --- | --- | --- |
  | Speaker: context switches per second | 912 | 429 | 274 |
  | Speaker: interrupts per second | 821 | 379 | 218 |
  | AirPods (AAC): audio thread wakeups per second | | 87 | 44 |
  | AirPods (AAC): decoder thread wakeups per second | | 57 | 25 |
  | AirPods (AAC): timer interrupts per second | | 272 | 198 |
  | Stutters (audio underruns) | 0 | 0 | 0 |

  Skipping, seeking, pausing, resuming, volume, wheel clicks while paused and waking the screen all worked with one
  core. A skip takes about half a second with the screen lit or dark, as before.
- **Not tested**: a battery or current measurement (the Y2 has no current sensor), LDAC and SBC, internet radio and
  films.

## Updating

Settings → System → Software Update → **Update Now**. Your music, playlists, settings, Wi-Fi networks and Bluetooth
pairings stay. **Restore Previous System** (Settings → System → About, tap Version seven times, then Diagnostics) goes
back to the version you had.

## Known issues

- If music stutters with the screen off, please send an error report (Settings → System → Error Reports) right
  after it happens.
- Everything listed for [Beta 9.3.34](Y2-LINUX-1.0-BETA-9.3.34.md#known-issues) still applies.

## Technical appendix

- Main loop: `main_loop_interval` returns `DARK_PLAYBACK_INTERVAL` (500 ms) while playing dark; `sleep_fade_near`
  caps the wait at 50 ms from one second before a sleep-timer fade.
- Wired PCM: 4 periods of 1024 frames in the same 4096-frame ring (`reborn-audio/native/audio.c`); Bluetooth stays
  8 × 1024.
- Decoder: `rb_media_read` fills the caller's 4096-frame block (`reborn-media/native/media.c`), position of the first
  sample, a decode error after a partial block returned by the next read (`deferred_error`).
- Full-queue wait: `queue_poll_interval` up to 100 ms with `thread::park_timeout`; `Playback::wake_decoder` unparks
  the decoder thread after every epoch store and decoder command.
- Output wait: `ready_wait_ms` = one period + 5 ms, 20–40 ms (`reborn-audio/src/native.rs`).
- Profiled and left as they are: the limiter (with the developer's AutoEq profile its worst-case peak is +3.4 dBFS,
  so skipping it would change the sound), the AAC encoder (no afterburner already), and the Bluetooth driver's
  per-packet work item (about 43 a second).
- Images built from Linux `bac18ae4`, Reborn `556f4c2`.
