# Y2Linux 1.0 Beta 9.3.40: longer battery life, End of Queue, chapter buttons and many small fixes

Release `1.0.0-beta.9.3.40`, Y2Reborn `1.0.0-beta.9.3.40`, kernel `6.18.56-y2linux-1.0.0-beta.9.3.40`, update
sequence 18800, root `2026.08-platform-v1.306`. Published as GitHub release `baseline-283` (build 283; test builds
276–282 came before it on the developer's player). Previous release: [Beta 9.3.39](Y2-LINUX-1.0-BETA-9.3.39.md).

This report is written for people who use the player. A short technical appendix is at the end.

## In one paragraph

The player uses less power: lying untouched it wakes up less and less often, the moving wave on Now Playing draws
at a calmer rate, and a new Battery Saver dims the screen when the battery is low. New are a sleep timer that stops at
the end of your queue, buttons for the next and previous audiobook chapter, a finer audiobook speed, and the screen
staying on while you read lyrics. Several fixes: a skin on the SD card is applied after a restart, the two new skins
show the names of pinned lists on Home, the Home Menu explains why Audiobooks or Videos are missing, update checks
say why they failed, and Cover Flow no longer glides back when you open it.

## What changed

- **Less power while the player lies untouched**: asleep, the player wakes every 30 minutes to check the battery.
  It now waits longer and longer the longer it sleeps (30 minutes for the first 2 hours, then 1, 2 and finally 4
  hours); every new sleep starts at 30 minutes again, and at 20 % or less it keeps checking every 30 minutes. On the developer's player 42.8 hours unplugged and asleep used about 1.6 mA, from
  99 % to 93 %. An earlier night on 9.3.39 measured about 9 mA; the wake-ups alone do not explain that difference,
  so the number is a good night, not a promise.
- **The moving wave draws at a calmer rate** ([Y2Reborn #197](https://github.com/SchulzCode/Y2Reborn/issues/197),
  measured by jonasevcik): while music plays with the screen on, the wave on Now Playing and Home was redrawn about 64
  times a second. Now 16, which roughly halves what the processor does then. Glides, slides and Cover Flow keep the
  full rate.
- **Battery Saver**: Settings → Display → Battery Saver: Off, At 15 % (the default) or On. While it is on, the screen
  is at most 30 % bright and turns off after at most 15 seconds, and animations rest. Your own settings come back when
  it turns off, also as soon as a charger is connected.
- **Sleep Timer: End of Queue** ([#175](https://github.com/SchulzCode/Y2Reborn/issues/175)): Now Playing → hold
  Select → Sleep Timer → **End of Queue** (End of Book in an audiobook). It pauses when the last song in Up Next ends,
  also with Repeat on, so the first song doesn't start again while you sleep.
- **Next Chapter and Previous Chapter buttons** ([#177](https://github.com/SchulzCode/Y2Reborn/issues/177)):
  Settings → Customize → Buttons. In an audiobook they move one chapter however long it is; with music they skip a
  song. They can also be used with the screen off.
- **Audiobook speed in fine steps** ([PR #198](https://github.com/SchulzCode/Y2Reborn/pull/198), thanks to
  AbeEstrada): in an audiobook, hold Select → Speed opens a slider from 0.5× to 2× in steps of 0.05×; Select keeps it,
  Back leaves it as it was.
- **The screen stays on while you read lyrics** ([Y2Linux #142](https://github.com/SchulzCode/Y2Linux/issues/142)):
  with Lyrics open and the music playing, the screen doesn't time out, as for a video. Paused, it does.
- **Sharper icons** ([PR #181](https://github.com/SchulzCode/Y2Reborn/pull/181), jonasevcik): the large music note
  for songs without a cover, the empty album cover and the Favorites heart are drawn sharp instead of stretched.

## Fixed

- **A skin on the SD card is applied after a restart**
  ([Y2Linux #143](https://github.com/SchulzCode/Y2Linux/issues/143)): a large SD card can take a few seconds before
  it is ready, and the player looked for the skin before that, showed Reborn's own look and kept your choice for
  later. Now the skin appears as soon as the card is ready.
- **Pixel Quest and Gilded Grove show "Artists", "Albums" and "Songs" on Home**
  ([#196](https://github.com/SchulzCode/Y2Reborn/issues/196)) instead of "…".
- **Home Menu says why a place is missing** ([#195](https://github.com/SchulzCode/Y2Reborn/issues/195)): Settings →
  Customize → Home Menu shows "No books yet" and "No videos yet" beside switches that are on while there is nothing
  for Home to show.
- **Update checks say why they failed** ([PR #179](https://github.com/SchulzCode/Y2Reborn/pull/179) and
  [Y2Linux PR #140](https://github.com/SchulzCode/Y2Linux/pull/140), jonasevcik): right after Wi-Fi connects the
  player waits up to 45 seconds for the internet time instead of failing, and a failed check names the reason
  instead of always "Couldn't reach the update server".
- **Cover Flow opens on the first album** ([PR #193](https://github.com/SchulzCode/Y2Reborn/pull/193),
  jonasevcik) instead of gliding back from where you left it.
- **An artist's albums load ahead** ([PR #180](https://github.com/SchulzCode/Y2Reborn/pull/180), jonasevcik): the
  covers beside the three on screen load before the wheel reaches them.
- **Building from source works again** ([#199](https://github.com/SchulzCode/Y2Reborn/issues/199)): the build used
  packages Alpine had replaced on its servers; it now uses Alpine 3.24.4 and the current packages.

## How it was tested

- **Tests**: the platform's full check, Reborn's tests and the 582 tests of the release images passed. New tests:
  the sleep timer pauses only when the last entry of the queue ends (not for a chapter inside it), the chapter
  buttons step chapters in a book and songs elsewhere, the speed slider stays open while the wheel turns, the wave
  draws at the calm rate, lyrics keep the screen on only while playing, the Home Menu notes, the backoff steps.
- **On the developer's player** (builds 276–283): the 42.8-hour night above, with the backoff wakes at 30, 30, 30,
  30, 60, 60, 120, 120, then 240 minutes; the wave at 16 pictures a second and about half the processor time while
  playing with the screen on; lyrics open and playing kept the screen on past a 60-second timeout, paused it turned
  off; a skin on the SD card applied 0.2 seconds after the card was ready when the card came late (on 9.3.39 never);
  1080p videos used the video chip.
- **Not tested on a player**: Battery Saver turning on at 15 %, End of Queue and the chapter buttons on the device
  (tests only), the speed slider by hand.

## Updating

Settings → System → Software Update → **Update Now**. Your music, playlists, settings, Wi-Fi networks and Bluetooth
pairings stay. **Restore Previous System** (Settings → System → About, tap Version seven times, then Diagnostics) goes
back to the version you had.

## Known issues

- Battery Saver turns on by itself at 15 % (Settings → Display → Battery Saver → Off if you don't want it).
- Videos and sound can drift apart over Bluetooth on some players
  ([Y2Linux #145](https://github.com/SchulzCode/Y2Linux/issues/145)); this version reports more about it in error
  reports. Videos larger than 1920 × 1080 are too much for the player.
- Everything listed for [Beta 9.3.34](Y2-LINUX-1.0-BETA-9.3.34.md#known-issues) still applies.

## Technical appendix

- Power plan 2026-11 ([plan](../planning/POWER-PLAN-2026-11.md)): maintenance backoff (`spm.maintenance_seconds`
  300–14400, `suspend/helper/backoff.rs`), 50 ms timer slack for background daemons, `energy.json` per state (written
  on boottime since `f075809e`), Battery Saver (Reborn `reborn_core::saver`), STP power saving built in but off
  (`stp_psm.enable=N`).
- Wave: `Ui::gliding` no longer counts `Motion::live`; `animating` does (34 ms frame gap).
- #143: `Runtime::retry_saved_skin` on a media source change. #142: `lyrics::keeps_screen_on`. #195:
  `pages::home_menu` notes; toggle rows draw their secondary text. #196: `build_skins.py` detail column by row width.
  #175: `SleepChoice::EndOfQueue`, `sleep_timer::ended_the_queue`. #177: `Command::NextChapter`/`PreviousChapter`.
  PR #198 merged with `ui_state::normalize` keeping the row-less `Modal::BookSpeed` open.
- #145 diagnostics: FFmpeg `y2vdec: … refused at <step>: <error>`, kernel `vdec` refusal lines, `/proc/buddyinfo` in
  reports, `reader_stopped` with `late_max_ms`, `late_avg_ms`, `sound_ms`, `wall_ms`.
- Build lock: `tools/build/inputs.lock.json` minirootfs 3.24.4 + 19 packages, `connectivity-host.lock.json` 3.
- Images built from Linux `5c12d83d`, Reborn `7f3fb0a`.
