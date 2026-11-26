# Y2Linux 1.0 Beta 9.3.41: cleaner quiet listening, Storage by kind, a smoother Home and eleven fixes

Release `1.0.0-beta.9.3.41`, Y2Reborn `1.0.0-beta.9.3.41`, kernel `6.18.56-y2linux-1.0.0-beta.9.3.41`, update
sequence 19300, root `2026.08-platform-v1.311`. Published as GitHub release `baseline-288` (build 288; test builds
284–287 came before it on the developer's player). Previous release: [Beta 9.3.40](Y2-LINUX-1.0-BETA-9.3.40.md).

This report is written for people who use the player. A short technical appendix is at the end.

## In one paragraph

Wired headphones sound cleaner at low volume: the volume is now set in the DAC chip instead of being taken out of
the music first, and audio that has to become 16 bits gets dither. Settings → Library → Storage shows what fills
each volume. Home moves more calmly and scrolls when it has many places. Eleven bugs found by a contributor's review
are fixed, among them the radio's elapsed time, the SD card pausing songs that are not on it, and Wi-Fi names with
accents. Songs tagged with several artists are scrobbled to Last.fm under the first one. All five changes are by
jonasevcik except the Last.fm fix.

## What changed

- **Cleaner sound at low volume on wired headphones**
  ([PR #207](https://github.com/SchulzCode/Y2Reborn/pull/207), jonasevcik): the player used to lower the volume by
  scaling the already 16-bit music and rounding it again, which costs about one bit for every 6 dB (about 11 bits
  left at volume 50). Now the DAC chip's own volume (0.5 dB steps) does it and the music reaches the chip as decoded.
  Measured by the contributor on a 1 kHz tone at volume 50: distortion and noise −48 dB before, −78 dB after. The
  balance moves to the chip too. Because the chip's steps are 0.5 dB and the player's 0.6 dB, the volume is rounded
  down (never louder) and one wheel step is 1.0 or 1.5 dB. The speaker, Bluetooth and the wheel click sound keep
  working as before. Should the chip ever refuse a volume, the player drops the few milliseconds already queued
  before going back to its own volume, so nothing plays at full level.
- **Dither where 16 bits lose information** (same PR): 24-bit files, MP3/AAC, resampled audio and audio changed by
  EQ, ReplayGain or speed get triangular dither on the way to 16 bits instead of plain rounding. A 16-bit file at
  44.1 kHz with nothing changing it still reaches the DAC bit for bit.
- **Storage by kind** ([PR #206](https://github.com/SchulzCode/Y2Reborn/pull/206), jonasevcik): Settings → Library →
  Storage splits each bar into Music, Audiobooks, Videos and Other, with size, count and play time per kind and a
  "used of" line. The numbers come from the library, so nothing is scanned again; a removed card shows "No SD card".
- **A smoother Home** ([PR #182](https://github.com/SchulzCode/Y2Reborn/pull/182), jonasevcik): choosing a place
  moves no other row; the chosen name grows into place while the marker slides to it. With more than six places
  (up to eleven with every Music list pinned) the menu scrolls and the next place peeks half-faded at the bottom.
  **Back on Home returns to the first place.**

## Fixed

- **Last.fm and several artists** ([Y2Linux #149](https://github.com/SchulzCode/Y2Linux/issues/149)): a song tagged
  "Artist A;Artist B" was scrobbled as one artist called "Artist A;Artist B". Last.fm keeps one artist per play, so
  it now gets the first one (the album artist the same way). "feat." tags and names with commas go as they are.
- Eleven fixes from a review of the code ([PR #205](https://github.com/SchulzCode/Y2Reborn/pull/205), jonasevcik):
  - **The elapsed time runs** for radio stations and files of unknown length instead of staying at 0:00.
  - **A song still starting** when the SD card changed, Stop was pressed or an SD repair began is given up cleanly;
    before, the screen could say Playing with no sound, or every action answered "Audio output is still starting".
  - **Taking out the SD card** pauses only a song from the card; Internal Storage songs and stations play on.
  - **The sleep timer** fades and restores the volume the output really uses (Bluetooth headphones that hold their
    own volume got the wrong level back).
  - **Films**: pictures without a time are shown instead of dropped; subtitles past line 4096 are kept (up to
    16384); sound and picture count from the same start in films whose clock does not start at 0.
  - **Wi-Fi**: networks with accented or non-English names are shown and joined by their real name (they appeared
    as `Caf\xc3\xa9`); hidden networks are no longer listed. Name and password limits count bytes, as the network
    does, and a name keeps its spaces.
  - **Lists Wrap Around: Everywhere** (Settings → Customize): Up from the first song under a heading wraps.
  - **Cover Flow** no longer crashes when its list gets shorter (a card taken out, a rescan).

## How it was tested

- **Tests**: the platform's full check, Reborn's tests and the 582 tests of the release images passed. New tests:
  bit-exact sound at reduced volume with a stand-in DAC, the chip's volume following the wheel and balance, the
  fallback when it refuses (queued sound dropped first), dither only where a conversion loses information, the
  Storage sums and layout, the Home layout and Back, each of the eleven fixes where a test could be written, and the
  Last.fm artist.
- **On the developer's player**, each change as its own test build (284–287), then this release:
  - DAC volume (headphones simulated): the chip's volume followed the wheel exactly (volume 39 → −37 dB, 50 → −30,
    90 → −6, 100 → 0, 1 → −81), a change made while paused took effect on resume, Stop gave the chip its earlier
    volume back, the speaker kept its own −3 dB; no underruns; a 96 kHz 24-bit file played (about 21 % of one core
    with a 10-band EQ).
  - Storage: the sums matched the library (5 313 songs) and the free space; the card taken out and put back.
  - Home: eight places scrolled at about 60 pictures a second; nothing is drawn while Home is idle.
  - PR #205: an MP3 starts at 0.02 s and seeks land; Stop right after Play; an Internal Storage song kept playing
    while the card went; two films after a seek with no pictures dropped and sound and picture within 0.05 s; the
    radio's elapsed time ran.
- **Not tested on a player**: listening with real headphones (the volume is in the chip, so please report clicks or
  steps you hear), a network with a non-English name, the Last.fm change (it would scrobble to a real account), the
  sleep timer with Bluetooth headphones, Wrap Around from a heading.

## Updating

Settings → System → Software Update → **Update Now**. Your music, playlists, settings, Wi-Fi networks and Bluetooth
pairings stay. **Restore Previous System** (Settings → System → About, tap Version seven times, then Diagnostics) goes
back to the version you had.

## Known issues

- Song positions now count from the very start of the file: a song resumed from a position saved before the update
  may start a few hundredths of a second later.
- A song from the SD card ends with an error, not paused, when the card is taken out while it plays; the card can
  take up to about a minute to be seen again after it is put back.
- Videos and sound can drift apart over Bluetooth on some players
  ([Y2Linux #145](https://github.com/SchulzCode/Y2Linux/issues/145)). Videos larger than 1920 × 1080 are too much for
  the player.
- Everything listed for [Beta 9.3.34](Y2-LINUX-1.0-BETA-9.3.34.md#known-issues) still applies.

## Technical appendix

- PR #207: `AudioSink::set_hardware_gain`/`applies_volume`, `rb_sink_set_master` (Master per channel, rounded toward
  quieter), `playback/dac_volume.rs` wraps wired sinks that report `hp_level_live`; `VolumeRamp` holds unity while
  the output applies the volume. Follow-up `b9cbe36`: a refused Master write discards the ring before the level's
  own Master comes back. Dither: `init_output_swr` with `SWR_DITHER_TRIANGULAR_HIGHPASS` when `output_dithers`.
- PR #206: `storage_use.rs` (sums per source, cached by library identity), `views_storage.rs`.
- PR #182: `HOME_ROWS` 6, 44 px rows, `Moving::List`, grow-in by `Ui::ease`; `Ui::back` on Home calls `home`.
- PR #205: commits `5f900f0`…`4344a3d`; `media.c` positions relative to `AVFormatContext.start_time`; fixture
  `film-h264-offset.mkv`.
- #149: `scrobble::lead_artist` (first non-empty `;` part) for artist and album artist.
- Images built from Linux `13d88839`, Reborn `db9fbb6`.
