# Y2Linux 1.0 Beta 8.22 (Baseline 121)

Status: **built, installed and tested on the owner's player; publication at the owner's word.** Release
`1.0.0-beta.8.22`, Reborn `1.0.0-beta.8.22`, kernel `6.18.56-y2linux-1.0.0-beta.8.22`, update sequence 8603 (test
builds 8600–8602 found the issues below on the owner's player), root `2026.08-platform-v1.152`, Linux `908eff7`,
Reborn `e196033`. Built with `tools/production/release.py` (111 s, resumed workspace, update based on `baseline-120`):
container suite **866 OK**, host suite 893 with two load-flaky report tests that pass alone (45/45), Reborn
workspace tests, clippy and fmt clean; all seal checks PASS, no MediaTek firmware in any image. Milestone audit
BETA-8.22 in [roadmap-gap-audit](../planning/roadmap-gap-audit.md).

| Image | SHA-256 |
| --- | --- |
| BOOTIMG | `c537ab8898a87beb32b8cdd6359630f07d2c9ddcda23492e43347bd7f7f01946` |
| Y2ROOT | `03ed2e1f33f63735a819f4fa3f98b2f4cd3e69fbb24d7cc72f7b681de9e2462b` |
| RECOVERY | `1f6f96311392e90019f6d543354176ae62fb82107473dced53fb0bbf451cd9a4` |
| Y2DATA (no SSH key) | `296f5a6e13ae5b16c69d474d566cf39c6161d4abd06efafd3d20dcbf29ff7f7a` |

## What changed since Beta 8.21

All fourteen uploaded reports pending on 2026-10-22 (8.10–8.20, nine players) were reviewed; findings F23–F31 in
the review ledger, plus a user's message (F32) and GitHub #53.

| Finding | Change | Evidence |
| --- | --- | --- |
| F23: an update rolled back — the new system's first boots died before the system logger (d0337717, 8.16→8.17, "new system did not confirm health"; 5f8082dd needed two boots), both GC9503V; no trace survived | **Evidence:** init, every rcS script and the platform's boot stages are written to pstore's message buffer (`/dev/pmsg0`, 128 KiB of the existing 1 MiB ramoops region, `CONFIG_PSTORE_PMSG`); the RAM console alone held only critical lines (`loglevel=3`, 41 bytes). Each update health boot keeps the previous boot's trail, console and oops record plus this boot's battery voltage, LK boot reason, data check and boot-check record in `/data/updates/health/` (newest 4; reports collect them). Until health is confirmed the console takes every kernel message and a CPU stall panics into a ramoops record and a restart; confirmed health restores both. **Fix candidate:** the recovery stopped neither its UI (≈9 frames/s) nor the backlight before `reboot -f`; it now does both, as the normal restart. Cause still unproven | Linux `f977ab5`, `0d0712c`; `tests/test_production_handover.py`, `tests/test_platform_update.py` |
| F24: headphones refused with ALSA -16 1–3 s after a wake | Reborn's sink open takes a shared `activity.lock`; the sleep helper held it exclusively through the ≈1.6 s radio restore. It now releases it as soon as the screen is back (other operation locks stay). Reborn retries a busy output for 2 s (100–250 ms steps) inside the 5-s open deadline and names the holder (`holders`: PCM owner, lease holders) | Linux `6053d9c`, Reborn `ef1acbc` |
| F25: Bluetooth playback refused "codec outcome unknown" until a restart | The uncertain-codec marker records PCM, device and address and clears once the headphones disconnect, reconnect (new PCM sequence) or BlueALSA restarts, only while nobody holds the PCM; plain wording | Reborn `ef1acbc` |
| F26: one kernel WARN disabled sleep (and deep idle) for the boot | A WARN-only taint (512) is allowed; every other taint bit still refuses | Linux `6053d9c` |
| F27: `WARN _cfg80211_unregister_wdev` when Wi-Fi powers down while connected | `wlanRemove` reports a live link gone before unregistering | Linux `6053d9c`, `9cd6e8b` |
| F28: Pair refused while the reconnect daemon pages | Pair busy-retries and posts `connect_pending` like Connect | Reborn `ef1acbc` |
| F29: quick Next logged every queued track as unreadable | The pre-open stops on cancellation without logging | Reborn `ef1acbc` |
| F30: per-frame scanout lines filled 27–99 % of the system log | Logged once per layout or when the address did not land | Linux `6053d9c` (overlay 0014) |
| F31: Wi-Fi failure evidence lost at the next connect | `wifi-failure.json` keeps wpa_supplicant's reason code and a `last_failure` block | Linux `6053d9c` |
| F32 (user message, 8.17): "Downloading Update… Keep Wi-Fi on." for a day, Wi-Fi toggles and restarts no help | A download writes a durable `Staging` record first; a download that dies without its handler left it, the page had neither Cancel nor Check for Updates, and a failed background check wrote it back. A `Staging`/`Checking` record without a live stage-lock holder now reads as `download_interrupted` ("Update Didn't Finish", Check for Updates); a failed check never restores it | Linux `f977ab5` |
| #53: no way to set the clock | Settings → Display → Date & Time (year, month, day, time; 12/24 h as the zone), `y2-platform time-set`: range-checked, never TLS-trusted, kept durably (next boot starts from it, labelled `manual`), saved hourly and at shutdown. Refused while this boot's clock is network time (the page then shows the clock only); a running time server is restarted after a manual set | Linux `bdd171a`, `a98df07`, `18ccf60`; Reborn `e9a5295`, `e196033` |

## Device results (owner's player, test builds 8600–8603)

| Check | Result |
| --- | --- |
| OTA 8500→8600, →8601… →8603 | each Acknowledged on health boot 1 (install ≈17 s) |
| F23 health evidence (8603 path) | `/data/updates/health/<time>-boot1-previous-boot.txt`: battery, `boot_reason=4`, data check clean, boot-check record, the recovery boot's trail (`init initramfs/recovery/recovery_install`) and console (`reboot: Restarting system`); after confirmation `printk 3 4 1 7`, `panic 0`, `panic_on_rcu_stall 0` |
| pmsg trail of a normal boot (software restart) | next boot read every init stage, all 24 rcS scripts with times, `platform_start` … `shutdown_complete` |
| F30 | 1 scanout line per boot (was ≈9 per second) |
| F32 | a `Staging` record without a download → `Failed / download_interrupted`; original record restored afterwards |
| #53 | set +2 h before network time: applied, `manual`, not TLS-ready, durable; out of range refused (`date_out_of_range`); with network time refused (`network_time_active`); a fresh ntpd stepped back to network time within 21 s; shutdown saved the clock |
| F27 | Wi-Fi off while Online and on again: no WARN, taint 0, Online after 12 s |
| Playback regression | 12/12 (cold start, 50 quick Next/Previous, pauses, 48/96 kHz incl. from a powered-down codec), `y2_hp/xtal` ready every power-up |
| F29 | 0 `next_track_open_failed` after 50 quick skips |

Found and fixed on the player during this release (why the sequence is 8603): the initramfs links few BusyBox applets
(`date`, `head`, `ls`, `sort`, `rm` went missing: empty file name, empty trail), printk's tab-separated levels made the
saved settings unreadable JSON (not restored after confirmation), and a running busybox ntpd did not step a 2-h
manual jump for minutes while its callbacks relabelled the clock as network time.

## Open

- Not checked on a player: F24 after a real sleep (product sleep refuses with USB attached, SSH needs USB), F25, F26,
  F28 and F31 in their failure cases (host tests only), the Date & Time page by hand (preview renders only).
- F23's cause stays open: the next failing health boot's file names where it stopped. The recovery display change is
  a candidate, not a proven fix.
- #52/#54 (8.21) confirmations from the reporters; car AVRCP metadata, long-book seek, #42, #45, #47 unchanged.
