# Error reports: opt-in reports from players to the maintainer

Status 2026-10-18: **R2 (collector, scrubber, triggers, queue, CLI) built and host-tested; nothing uploads yet.** Owner: "an error logger/reporter for users that
they can opt in. It should send me everything about the error and device, no personal information, over Wi-Fi
HTTPS", then "go with the Cloudflare Worker … get as much information as possible … not only kernel level, also
everything that happens in Reborn". Audit row ERROR-REPORTS in [roadmap-gap-audit](roadmap-gap-audit.md).

## Decisions (owner, 2026-10-18)

| Topic | Decision |
| --- | --- |
| Receiver | **Cloudflare Worker + R2** at `reports.schulzcode.dev` (the domain is already on Cloudflare). The home server was checked and is well maintained, but it is home/Tailscale-only today, and opening Nginx Proxy Manager to the internet would expose every proxy host (none has an access list) and put the home IP into the firmware. |
| Content | **Scrubbed raw logs**: everything the player records (kernel, platform, recovery/update, Reborn), hardware IDs and live state, with personal data replaced (below). |
| Sending | **Automatic + manual**: an error queues a report that uploads in the background over Wi-Fi; Settings has *Send Report Now*. Only after the user opted in. |
| Linking | **Random install ID**, made on the player (never derived from a serial), shown in Settings and resettable. |
| Asking | One question after the update, then the switch in Settings (see *Further decisions*). |

## What a report contains

One gzip-compressed tar per report, `report.json` (index, identity, trigger, section sizes and scrub counts) plus one
file per section. Budget 8 MiB compressed (typical: 1–3 MiB); a section that does not fit is cut from the oldest end
and the cut is recorded.

### Identity and trigger

- Report schema, report ID (random), install ID, trigger class and detail (see *When*), wall time (UTC) and uptime,
  boot ID, boot count, UTC offset (not the zone name).
- Release, update sequence, Y2Linux and Reborn commits, kernel and root filesystem versions, build ID; update state
  (`/data/updates/state.json`: state, sequence, previous sequence, timings, failure).

### Hardware

- SoC and CPU revision, RAM, panel (`panel_kind`, LK's `lcm=` token and `panel=`), stock bootloader identity (SHA-256
  of LK and preloader regions: tells 3.2.0 from 3.2.1 units, #45/#46).
- eMMC: name, manfid, oemid, PRV, firmware, date, size, EXT_CSD feature bytes and life/EOL. **Not** the CID serial.
- SD card: manfid, oemid, name, date, size, bus mode. **Not** the CID serial.
- MT6323 CID, CS43131 revision, APT32F firmware, CONSYS chip/HVR/FVR, input devices, partition table.
- Battery and charger: voltage, current, the charger's raw readings (`baton_raw`, `isense_raw`, source), capacity,
  temperatures, thermal zones.

### Kernel

- The whole kernel ring buffer (`dmesg`, 1 MiB buffer), the persistent crash record of the previous boot
  (`/sys/fs/pstore/*`), `/data/logs/system.log` and `system.log.0`.
- Taint, warning count, `/proc` state: version, cmdline, meminfo, vmstat, loadavg, interrupts, softirqs, cpuinfo,
  pressure; suspend statistics and wakeup sources; cpuidle and cpufreq statistics (time in state); regulator and
  clock summaries (debugfs); mounts and space; ext4 error counts; process list with RSS and CPU time.
- Y2 driver status nodes already used by the platform: SPM, CIRQ, USB (`status`, `rxdiag`), MMC, connectivity
  (`/sys/devices/platform/18070000.connectivity/status`), panel parameters, codec registers (`regmap` of 1-0030),
  `y2_boot`.

### Platform (Y2Linux services)

- Health (`y2-platform health`), the existing allowlisted tester export (`diagnostics.py`), boot record
  (`boot.json`, `previous-boot-evidence.json`, `/run/y2/boot-stages.jsonl`, `boot-trace.jsonl`), the retained
  per-boot archives (`/data/system/platform/retained/*.tar`).
- Power: `power-journal.jsonl` and `sleep-diagnostics.jsonl` (last 1 MiB of each), `suspend-last.json`,
  `product-sleep-guard.json`, `power-features.json`, `rtc-policy.json`, `time-last.json`, `battery-ui.json`.
- Update and recovery: `/data/updates/*.json` (state, recovery-last, boot-check-last, download, last-check,
  restart), `/data/logs/recovery-last.log`, `recovery-display.txt`, `/run/y2/update-*.json`.
- Every runtime state file in `/run/y2` (`*.json`, `*.jsonl`, `*.log`: connectivity, wifi, dhcp, network,
  coexistence, codec runtime, media and storage, MTP events, USB transfer, time, readiness, ssh), scrubbed.

### Reborn

- All Reborn logs (`/data/reborn/logs/*.jsonl`: current, previous-1, previous-2, splash and startup, ≈2.1 MiB;
  every subsystem: playback, alsa, audio, power, wifi, bluetooth, scanner, graphics, startup, …).
- The last crash (`panic-last.json`) and the latest diagnostic bundles (`/data/reborn/diagnostics/*.tar.gz`).
- Live state from the control socket: `rebornctl status`, `health`, `metrics`, `audio`, `snapshot`, `events`.
- Settings from `session.json`, **projected**: every setting value, the queue length and position, the output kind,
  the current track's technical data (codec, rate, bits, channels, duration, ReplayGain present), never titles,
  artists, paths, playlists or account data. Last.fm: only signed-in yes/no and queue counts.
- Library statistics from `library.db`: counts per codec, sample rate, bit depth and source, scan errors by class,
  database size and schema version. No names.

## What is never sent, and how

Two layers, both in one module (`tools/platform/y2_platform/report.py`, `scrub()`), applied to every byte of every
section before it is written:

1. **Known secrets of this player**, collected at report time and replaced wherever they appear: saved Wi-Fi
   network names and passphrases, the current BSSID, Bluetooth device names and addresses (paired and seen), the
   Last.fm user name and session key, the SSH key comment, the hostname, the player's own MAC and IP addresses.
   Exact-match is robust where a pattern is not (an SSID can be any text).
2. **Patterns**: MAC addresses (keep the first three bytes, the maker; mask the rest), IPv4/IPv6 (keep the class:
   loopback, link-local, USB `10.42.0.x`, private, public), serial-like fields (CID, `serial`, `machine-id`), e-mail
   addresses, URLs (keep the host, drop path and query), paths below `/media` (keep the extension:
   `<media>/…/*.flac`).

Replacements are stable within one report (`<ssid#1>`, `<bt#2>`, `<mac 3c:22:fb:…#1>`), so a log still shows that
the same network or device came back. `report.json` counts the replacements per class.

Not collected at all: `library.db` contents, `scrobbles.jsonl`, `lastfm.json`, Wi-Fi configuration, Bluetooth bond
keys, SSH keys, `/data/system/dbus/machine-id`, entropy seed, artwork caches, any file below `/media`.

Tests: canary values (an SSID, a passphrase, a Bluetooth name, a Last.fm user, a MAC, an IP, a CID, a music path)
are planted in every source of a synthetic player tree, a report is built, and none may appear in it. The same runs
against a copy of real logs from the owner's player kept outside Git.

## When a report is made

Only with *Error Reports* on. At most one report per trigger class per boot, at most five a day; the queue holds three
(oldest dropped).

| Trigger | Detected by |
| --- | --- |
| Kernel crash on the previous boot | pstore record at boot |
| Kernel warning / taint | taint flags or warning count rising |
| Unclean restart, watchdog reset | `previous-boot-evidence.json` (no orderly shutdown, reset cause) |
| Reborn crash or restart by its supervisor | a new `panic-last.json`, supervisor restart count |
| A required Reborn worker or subsystem failed | Reborn `health` |
| Update failed or rolled back, recovery entered | `/data/updates/state.json`, `recovery-last.json` |
| Sleep failure (refused with an error, failed resume, stuck guard) | sleep diagnostics and power journal rows |
| Radio failure (Wi-Fi/Bluetooth boot or restore failed) | connectivity status, `wifi-failure.json` |
| Dark display (DSI register lag, no frame) | kernel log lines of the display driver |
| Storage error (ext4 errors, I/O errors, read-only remount) | ext4 counters, kernel log |
| Manual | *Send Report Now* (with an optional choice: Sound, Display, Battery, Bluetooth, Wi-Fi, Crash, Other) |

A trigger collects at once (the logs are fresh) and queues the report; uploading waits for Wi-Fi and runs at low
priority (the platform's NetworkTransfer workload class), never during an update and never below 15 % battery unless
charging.

## Transport

- `POST https://reports.schulzcode.dev/v1/report`, body = the tar.gz, headers: schema, install ID, report ID,
  SHA-256 of the body. TLS 1.2+ with certificate checks (the same urllib/OpenSSL path as updates), no redirects.
- A build-wide upload key in the header (it is public, inside the firmware; it only keeps random internet traffic
  out). The real protection is the Worker's limits.
- Retry with backoff (1 min … 6 h), give up after 3 days; the reply's report ID is shown in Settings.

## The Worker

- `POST /v1/report` only. Checks: method, path, upload key, `Content-Length` ≤ 8 MiB, gzip magic, schema version,
  install ID format, body SHA-256. Rate limits: 10 reports a day per install ID, 30 per IP (Workers KV counters).
- Stores `reports/YYYY-MM-DD/<install-id>/<report-id>.tar.gz` in a private R2 bucket with small metadata (schema,
  release, trigger, size). It stores no IP address and has no read endpoint.
- R2 lifecycle: delete after 180 days.
- The maintainer reads with an R2 API token (S3 API): `tools/reports/fetch.py` downloads new reports to
  `out/reports/` (or a scheduled pull to the home server).
- Code in `tools/reports/worker/` (JavaScript, `wrangler.toml`, unit tests with `node --test`); the owner deploys it
  with `wrangler deploy` in his Cloudflare account.

## Player settings and screens

Settings → About & Updates → **Error Reports**: the switch (off by default), *What is sent* (a short list and a link
to the privacy note), the install ID with *New ID*, *Send Report Now*, and the last report's state (queued, sent with
its ID, failed with the reason). Reborn talks to the platform through its existing client (`client::Reply`), like
updates. State: `/data/system/platform/reports.json` (switch, install ID, counters), queue in
`/data/system/platform/reports/`.

User documentation: `docs/ERROR-REPORTS.md` (what is sent, what never is, how to see and delete one's reports by
install ID: a request on GitHub with the ID).

## Steps

| Step | Content | Validation |
| --- | --- | --- |
| R1 | This plan, audit row | — |
| R2 | `report.py`: sources, sections, scrubber, budget, triggers, queue (platform) | Host tests with canaries, size bounds, every trigger, on a synthetic tree and on a private copy of real logs |
| R3 | Worker + `fetch.py` | `node --test`; owner deploys to `reports.schulzcode.dev` |
| R4 | Uploader and platform commands (`y2-platform report status|send|enable|disable|new-id`) | Host tests against a local HTTPS stub |
| R5 | Reborn screens and client calls | Reborn tests, preview screenshots |
| R6 | Device test with a test build (owner's OK): manual report, a provoked Reborn crash, an update failure from the test bench, upload to the deployed Worker, owner downloads | Device |
| R7 | Release, `docs/ERROR-REPORTS.md`, readme | Release checks |

## Further decisions (owner, 2026-10-18: "yes to all")

- Players ask once after updating to the release with error reports ("Help improve Y2Linux by sending error
  reports?" Yes / No); the switch in Settings stays.
- MAC addresses keep their maker part (first three bytes).
- R2 keeps reports 180 days.

## R2 results (2026-10-18)

- `tools/platform/y2_platform/report.py`, `y2-platform report status|enable|disable|new-id|send [--topic]|check`,
  `tests/test_error_reports.py` (in the release gate): planted canaries in every source (Wi-Fi, Bluetooth paired
  and unpaired, Last.fm, SSH, eMMC/CPU/machine serials, volume ID, queue titles and paths, IPs, e-mail, URL token,
  a file on the never-read list) never appear; removing any scrub layer fails the test (mutation-checked).
  Evidence kept: panic text, Reborn events, MAC makers, the USB network, text after a music file, session settings,
  library counts, hardware IDs. Budget cut, queue limits and every trigger tested. Host suite 813 OK.
- Known-secret matching is one prefix-tree regex (an alternation of 2000 titles took 4 s per 200 KB; now
  0.1–0.2 s per MB on the PC).
- **Real data, owner's player (read-only, `/tmp` only, removed afterwards):** a full report takes **33 s** (nice 10),
  5.7 MB raw → **≈390 KB** compressed, 162 sections, nothing cut. Harvested: 1 network, 5 Bluetooth names, the
  Last.fm account, 5 serials, 184 queue strings. None of the owner's Wi-Fi name, Last.fm name, headphone address,
  eMMC serial, home IPs, song/artist names, music paths or SD volume serial appears. Found and fixed on the way:
  `ctx.glob` paths were re-rooted (Bluetooth names never harvested), the 128 KiB `ctx.json` limit hid the 311-song
  session, the SD volume serial in Reborn's media source, an SSH key comment ("Y2Linux") blanking the product name.
- Remaining "@"/"luca" hits are FFmpeg's `[flac @ 0x…]` prefixes and the build host in the kernel/FFmpeg version
  strings (the maintainer's, part of the firmware).
- Next: R3 (Worker), R4 (uploader + periodic `check` in the platform service).

## Test build 96 on the owner's player (2026-10-18)

`1.0.0-beta.8.8`, sequence 6950, Linux `acec57c`, Reborn `ef9e805` (identity only); container suite 786 (the new
tests run there too). Installed by stage + apply, ≈16 s, Acknowledged 6950. `y2-platform report status` →
`enabled: null`; `enable` made an install ID and took the baseline (`check` then reported nothing, 0.7 s);
`send --topic other` built and queued a manual report in **37 s, 409 KB**, 163 sections, all 13 live commands OK;
the leak search on it found none of the owner's secrets. Afterwards `reports.json` and the queue were removed (state
back to "not asked") and `/data/ota-96` deleted. Not released.
