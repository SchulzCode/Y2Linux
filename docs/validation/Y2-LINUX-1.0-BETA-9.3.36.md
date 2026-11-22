# Y2Linux 1.0 Beta 9.3.36: less work for the processor while music plays with the screen off

Release `1.0.0-beta.9.3.36`, Y2Reborn `1.0.0-beta.9.3.36`, kernel `6.18.56-y2linux-1.0.0-beta.9.3.36`, update
sequence 17400, root `2026.08-platform-v1.292`. Published as GitHub release `baseline-269` (build 269). Previous
release: [Beta 9.3.35](Y2-LINUX-1.0-BETA-9.3.35.md).

This report is written for people who use the player. A short technical appendix is at the end.

## In one paragraph

Most listening happens with the screen off. jonasevcik looked at what keeps the processor busy then and proposed
three changes ([Y2Reborn #176](https://github.com/SchulzCode/Y2Reborn/issues/176)), with the code. All three are in
this release. On the developer's player the processor now wakes about half as often during dark playback, and three
of its four cores switch off while the music plays. Nothing changes in what you hear or in any setting.

## What changed

- **The processor changes its speed less often.** It used to re-decide its speed almost every millisecond, which
  meant about a hundred speed changes a second during playback. It now decides every 25 ms. When you use the
  player, it still speeds up at once.
- **The music decoder waits calmly when it is ahead.** While the audio queue was full, the decoder checked it 200
  times a second. It now checks about once per block of audio, and the queue still holds plenty of music in reserve.
- **Three cores switch off during playback with the screen off.** Before, any music kept all four cores on. Now,
  with the screen dark, ordinary playback runs on one core. Waking the screen, pressing a button, a library scan,
  a file transfer, the heavy-playback mode and real load bring the other cores back at once, as before. Deep idle is
  still not used while music plays. If music ever stutters on one core, the player keeps all cores on for the next
  five minutes.

## How it was tested

- **Tests**: the platform's full check, Reborn's tests and the tests of the release images passed. New tests: the
  25 ms setting (also when deep idle is not admitted; nothing written without the setting), the decoder wait, the
  five-minute hold after a stutter, and the parking rules for playback (Python host tests of the kernel decision).
- **On the developer's player** (build 269, an MP3 on the speaker, Wi-Fi on, connected to USB, screen off, each
  window about 5 minutes, one run each):

  | | Beta 9.3.35 | 9.3.36, cores kept on | 9.3.36 |
  | --- | --- | --- | --- |
  | Cores online | 4 | 4 | 1 |
  | Context switches per second | 912 | 489 | 429 |
  | Interrupts per second | 821 | 486 | 379 |
  | Timer interrupts per second | 433 | 264 | 235 |
  | Speed changes per second | 117 | 28 | 28 |
  | Stutters (audio underruns) | 0 | 0 | 0 |

  With one core: skipping, seeking and changing the volume kept playing without a stutter; waking the screen
  brought all four cores back at once, and they switched off again about 100 s after the screen went dark.
- **jonasevcik's own runs** (Bluetooth AAC, on battery): 1,960 → 1,066 context switches a second and a battery
  voltage that fell more slowly with each change (see the issue).
- **Not tested**: a battery or current measurement on the developer's player (the Y2 has no current sensor),
  Bluetooth headphones on one core on the developer's player, LDAC, internet radio, high-resolution files, and
  films (films keep the screen on, so all cores stay on).

## Updating

Settings → System → Software Update → **Update Now**. Your music, playlists, settings, Wi-Fi networks and Bluetooth
pairings stay. **Restore Previous System** (Settings → System → About, tap Version seven times, then Diagnostics) goes
back to the version you had.

## Known issues

- If music stutters with the screen off since this version, please send an error report (Settings → System → Error
  Reports) right after it happens.
- Everything listed for [Beta 9.3.34](Y2-LINUX-1.0-BETA-9.3.34.md#known-issues) still applies.

## Technical appendix

- 25 ms: `y2-platform cpu-idle-policy start` writes `25000` to
  `/sys/devices/system/cpu/cpufreq/schedutil/rate_limit_us` once per boot (also when DORMANT is skipped) and records
  the readback as `rate_limit_us` in `/run/y2/cpu-idle-policy.json` (`commands/cpu_idle.rs`; proposed as a boot
  script, moved into the existing boot policy).
- Decoder wait: Reborn `playback/stream.rs` `queue_poll_interval` (one block, clamped 5–25 ms).
- Parking: `kernel/platform/workload.c` treats a live `PlaybackNormal` lease as soft for the parking coordinator
  while the backlight is dark (renewals no longer restore; the DORMANT idle blocker is unchanged);
  `system-idle-policy.h` allows 800 ‰ time above 598 MHz under that lease (load ≤ 400 mcores and the burst guard
  still decide). Run-time switch `/sys/module/workload/parameters/soft_playback` (`Y`; `0` keeps the old rule).
- Stutter hold: Reborn `reborn_platform::workload::XrunGuard` publishes `PlaybackHeavy` for 300 s after the
  `audio_xruns` counter moves.
- Contributor commits kept with their author (Linux `6e08db6a`, Reborn `2955c94`, `bd0adda`); Linux `04b9d6e0` for
  the boot policy.
- Images built from Linux `ccc5d6fd`, Reborn `d2642bf`.
