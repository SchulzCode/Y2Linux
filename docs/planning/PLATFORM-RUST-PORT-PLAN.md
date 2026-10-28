# Platform service: port from Python to Rust, then remove Python from the player

Status: **plan, not started** (2026-10-28). Owner's goal: everything that runs on the player is Rust (Reborn), C
(kernel, early boot, installer) or shell (init scripts); **no Python in the image**. Host-side tooling (release,
build, validation, the test harness) may stay Python.

This plan is written for an agent picking the work up cold. Read it fully, then
[AGENTS.md](../../AGENTS.md), [CURRENT_PLATFORM_STATE](../CURRENT_PLATFORM_STATE.md) and the top entry of
[NEXT-SESSION-HANDOFF](NEXT-SESSION-HANDOFF.md) before changing code.

## 1. Why

Measured on the owner's player (Beta 8.44 test build 165, 37 min awake on the charger, 2026-10-28):

| | Python platform (4 daemons) | Reborn (Rust) |
| --- | --- | --- |
| RSS | service 20.3 + connectivity 15.3 + power 12.8 + ssh 11.5 ≈ 60 MB (shared pages counted per process) | 27.5 MB |
| CPU time | 67 s (power-daemon 38 s, service-daemon 23 s) | 28 s |
| Image | Python 3.15 ≈ 22 MB (`usr/lib/python3.15` 17 MB, `libpython3.15.so` 4.9 MB) + 12 334 lines of source | 8.6 MB binary |
| One CLI call | `python3 -I -c pass` 0.15 s; `y2-platform sleep status` 0.52 s | — |

Every CLI call is a new Python process: Reborn's polling (`status --reborn --pss` every 4–15 s), every
`wpa-event`/`dhcp-event`/`ntp-event`, every boot-stage journal write (two per shutdown, ≈0.5 s each, see the F39 trail).
Awake CPU costs battery; the 22 MB and the RAM come back only when Python is gone entirely.

Also wanted: one language with Reborn and **shared contract types**. Reborn and the platform exchange JSON whose
field and reason strings are matched by hand on both sides; F52 ("not_authenticated" read as a wrong password) was
such a mismatch. A shared `serde` crate makes the compiler check it.

## 2. What exists (inventory, 2026-10-28)

**Source:** `tools/platform/y2_platform/` (43 modules, 12 334 lines), entry `tools/platform/main.py`, helpers
`product-sleep.py`, `suspend-record.py`, scripts `y2-platform`, `y2-health`, `y2-power-audit` (Python),
`y2-power-command`, `y2-status`, `y2-suspend-record`. Installed to `/usr/lib/y2-platform` + `/usr/sbin`. Biggest
modules: report 1420, media 902, observe 758, update 695, product_sleep 571, internal_media 571, radiofw 568,
network 550, usb 483, power 458, cli 430, maintenance 423, bench 396, service 354, battery 337.

**Daemons** (started by `S03y2-platform` / `platform-supervise` / `connectivity`): `power-daemon` (battery, charger,
sleep, maintenance wakes, shutdown coordinator), `service-daemon` (media/SD admission, updates, reports, internal
storage, time), `connectivity-daemon` (Wi-Fi/BT supervision, radio policy, F52/F53 heal), `ssh-daemon`.

**CLI subcommands** (`cli.py`): status, health, capabilities, boot-evidence, boot-stage, cpu-idle-policy,
bluetooth-power, sleep {request,status,prepare}, power-daemon, service-daemon, connectivity-daemon, usb-mode,
codec-runtime, codec-settings, wifi-power-policy, ssh-daemon, transfer, reset, export-diagnostics, report {…},
export-state, firmware, update {check,apply,cancel,rollback,stage,status,…}, time-bootstrap, time, time-set,
time-remember, rtc-alarm, ntp-event, wifi-event, dhcp-event, network-check, shutdown, media {…}, internal-media {…},
bench-storage, space, bench-library, collect.

**Callers on the player:** Reborn `crates/reborn-platform/src/client.rs` (`/usr/sbin/y2-platform` with
capabilities, codec-settings, export-diagnostics, export-state, health, internal-media setup, media repair,
report {enable,disable,new-id,save,send,status}, sleep request, status --reborn --pss, status wifi, time-set, update
{apply,cancel,check,rollback}); `storage.rs` → `/usr/sbin/y2-media`; `power.rs`. Overlay scripts
(`buildroot/board/y2/production-overlay`): `etc/init.d/S02y2-media`, `S03y2-platform`, `S03y2-time`,
`S05y2-cpu-idle`, `usr/libexec/y2/{platform-supervise,connectivity,wpa-event,dhcp-event,ntp-event}`,
`usr/sbin/{y2-media,y2-radio,y2-suspend}`. initramfs: `/sbin/y2-platform-start` and the platform contract marker
`/etc/y2linux/platform-contract` = `y2-platform-v1`.

**State contract** (files other parts read; keep paths and shapes): `/run/y2/*.json` (power.json, network.json,
media.json, media-mount.json, storage.json, wifi-events.json, application-ready.json, boot-stages.jsonl, …),
`/data/system/platform/*` (boot.json, power-journal.jsonl, previous-boot-evidence.json, sd-admissions.json,
sleep-diagnostics.jsonl, battery-ui.json, wifi-journal.jsonl, reports/…), `/data/updates/*` (state.json, …),
pmsg/kmsg trail lines (`shutdown begin …`, `platform …`), versions `platform_api_version` 1,
`feature_contract_version` 2, `capabilities.json`. The error-report scrubber/uploader, the report Worker and the
review tooling (`tools/reports`, `UserReports/REVIEWED.md` analyses) parse these: field names are an API.

**Python elsewhere on the player:** none found besides the platform (`grep -rlI '^#!.*python'` in the target finds
only `y2-power-audit`). Buildroot: `BR2_PACKAGE_PYTHON3=y` (+ `_PY_ONLY`, `_SQLITE`, `_SSL`, `_ZLIB`) in
`buildroot/configs/y2_production_defconfig`. Verify nothing else selects it before removal (§5 phase 6).

**Tests:** 46 test modules import `y2_platform` (e.g. test_platform_power_contract, test_battery_soc,
test_product_auto_sleep, test_platform_update, test_error_reports, test_connectivity_supervisor,
test_wifi_failure_evidence, test_platform_evidence, test_sleep_product, test_internal_media,
test_platform_storage_tools, …; ≈28 700 lines of Python tests overall). Most build a fake root tree and call
functions through `ctx` (`common.py`: `ctx.path`, `ctx.read`, `ctx.json`, `ctx.command`, atomic JSON writes).

## 3. Principles

1. **Strangler, not big bang.** One Rust multicall binary becomes `/usr/sbin/y2-platform` early. Subcommands it
   implements run in Rust; the rest `exec` the Python entry (`python3 -I /usr/lib/y2-platform/main.py …`) with the
   same arguments. Move command by command, daemon by daemon. Python is removed only in the last phase.
2. **The contract does not change.** Same CLI arguments, exit codes, stdout JSON, file paths, file shapes (field
   names, types, enum strings), timeouts, lock files and lock protocol (flock on the same paths), atomic-write
   semantics (temp + fsync + rename, no randomness at boot: see `test_platform_records_do_not_need_the_random_pool`),
   pmsg/kmsg line texts. Reborn and the report tooling must notice nothing. Contract changes, if ever wanted, are a
   separate step after parity.
3. **Python and Rust coexist safely.** During the transition a Python daemon and a Rust CLI may touch the same state:
   identical locking and write order are mandatory, not optional.
4. **Port behaviour, including the fixes.** Every F-number fix in the Python code (comments name them: F13, F23,
   F34, F36, F38, F39, F46–F57 …) is behaviour to keep. Port the comment with the code. Read the ledger row
   (`UserReports/REVIEWED.md`, private) when a comment names one.
5. **Differential testing is the safety net.** For each ported piece, run Python and Rust on the same fake root
   trees and compare outputs and resulting files exactly (normalise only timestamps that are inputs).
6. **Hardware gates.** Power, sleep, battery, charging, update and shutdown pieces each need a test build on the
   owner's player and an overnight run before the next phase (AGENTS.md: broader qualification for architecture
   changes; repeat the roadmap gap audit at milestone boundaries).
7. **Small and boring Rust.** No async runtime unless a daemon really needs it (the Python daemons are loops with
   sleeps and subprocesses; mirror that with threads/poll). Dependencies: `serde`, `serde_json`, `libc` (or `rustix`),
   maybe `rusqlite` (only if a ported command needs SQLite). Keep using the `curl` binary for HTTPS as Reborn does
   (`radio.rs`, `lastfm.rs`): no TLS stack in the binary. Release profile `opt-level = "s"`, LTO, `panic = "abort"`
   only if no code relies on unwinding.

## 4. Layout and build

- New Cargo workspace in Y2Linux: `platform/` with
  - `y2p-contract` — serde types for every JSON the platform writes or prints (status, network, power, media,
    update state, sleep, report status …), enum reason strings as Rust enums with `#[serde(rename = …)]`.
  - `y2p-core` — root-prefixed paths (`Y2_ROOT` / `--root` for tests, like `ctx.path`), atomic JSON, command runner
    with deadlines and D-state-aware timeout reporting (port `common.command`), flock helpers, kmsg/pmsg trail,
    monotonic/boottime clocks.
  - `y2-platform` — the multicall binary (one crate per area is fine: power, service, connectivity, media, update,
    report, time, usb, ssh).
- Toolchain: the same pinned Rust as Reborn (`rust-toolchain.toml` 1.99); vendored crates (`cargo vendor`) as Reborn
  does; a Buildroot package `buildroot/package/y2-platform` modelled on `buildroot/package/reborn`.
- **Contract sharing with Reborn:** Reborn should depend on `y2p-contract` instead of its hand-written structs in
  `reborn-platform`. Options: (a) Reborn vendors `y2p-contract` from Y2Linux at a pinned commit, (b) the crate lives
  in Y2Reborn and Y2Linux vendors it. Pick one in phase 1 and write it down; release tooling
  (`tools/production/release.py`) must keep both repos' commits consistent.
- Install: Rust binary at `/usr/sbin/y2-platform`; Python moves to `/usr/lib/y2-platform/main.py` only (the
  fallback). `capabilities.json` and the contract marker unchanged.

## 5. Phases

Each phase ends with: host tests green (`tools/development/host-tests.sh`), Reborn tests green, a test build
(`release.py build --label N`, see docs/BUILDING.md §7) installed on the owner's player by staged OTA and
Acknowledged, the phase's device checks, a handoff entry. Publish only when the owner says so.

### Phase 0 — Baseline and contract inventory (no product change)
- Measure on the player: RSS and CPU of each daemon over 1 h awake idle and 1 h playing; awake current (charger
  register or battery delta); CLI latency for every command Reborn calls; boot stages (`platform_start` →
  `services_started` → `application_ready`); overnight drain. Write them into this plan (§7 table).
- Generate a machine-readable contract inventory: for every subcommand its arguments, exit codes, stdout schema,
  files read/written, locks, timeouts, subprocesses. Start from `cli.py` and the tests; record it as
  `platform/CONTRACT.md` (or JSON) and keep it updated.
- Build the **differential harness**: a host test helper that runs a subcommand under Python and under a host build
  of the Rust binary against the same fake root (copy of the fixture tree), then diffs stdout, exit code and the
  resulting tree.

### Phase 1 — Skeleton and dispatcher
- Workspace, `y2p-core`, `y2p-contract` (start with the types of `status`, `health`, `capabilities`, boot stages),
  Buildroot package, multicall binary that implements nothing yet and execs Python for every subcommand.
- Device check: everything behaves exactly as before (one boot, sleep, update restart).

### Phase 2 — Hot short-lived commands (largest CPU win, low risk)
- `status` (incl. `--reborn --pss`, `wifi`), `health`, `capabilities`, `boot-stage`, `boot-evidence`,
  `time-remember`, `time`, `sleep status`, `wifi-event`, `dhcp-event`, `ntp-event`, `network-check`,
  `codec-runtime`, `cpu-idle-policy`.
- Device checks: Reborn screens that show platform state (Settings, About, Wi-Fi, Storage), shutdown trail timing,
  Wi-Fi connect/reconnect events, CPU before/after.

### Phase 3 — Small daemons and remaining simple commands
- `ssh-daemon`, `connectivity-daemon` (network.py, connectivity.py, radio_policy, bt_control, bluetooth, the F52/F53
  heal and wifi-journal), `usb-mode`, `wifi-power-policy`, `bluetooth-power`, `codec-settings`, `time-set`,
  `time-bootstrap`, `rtc-alarm`, `space`, `transfer`.
- Device checks: Wi-Fi join/wrong password/heal, Bluetooth on/off/reconnect, USB modes (MTP/ECM), SSH.

### Phase 4 — Service daemon, media, update, reports
- `service-daemon` (service.py, maintenance.py), `media` + `y2-media` (SD admission, F49 background verify,
  repair, write windows), `internal-media`, `update` (check/stage/apply/cancel/rollback; works with the C
  `y2-update-core`), `report` (scrubber, save, send via curl, schedule), `export-diagnostics`, `export-state`,
  `firmware` (radiofw), `collect`, `reset`.
- Device checks: SD insert/remove/repair/large card, Internal Storage setup on a wiped data image (owner decides),
  a full OTA update + rollback, report save and send, firmware import from system.img.

### Phase 5 — Power daemon (last, most sensitive)
- `power-daemon`, `sleep request/prepare`, product sleep, sleep diagnostics, power journal, suspend record,
  battery/SOC (F36, F54, F55), charger state (F38, F42, F46), maintenance wakes, `shutdown` coordinator (F39/F50
  watchdog, long-press reset, per-step pmsg trail), `y2-power-audit`, `product-sleep.py`, `suspend-record.py`.
- Device checks: 10 sleep/wake cycles, charger plug/unplug during sleep, low battery to power-off, power off on a
  charger (charging screen), update restart, **two overnight runs** with drain compared to the Phase 0 baseline.

### Phase 6 — Remove Python
- Remove the dispatcher fallback, `/usr/lib/y2-platform`, Python scripts, `BR2_PACKAGE_PYTHON3*` from the production
  defconfig; confirm with `make … show-depends`/the legal-info manifest that nothing else pulls Python; firmware scan
  and package validation still pass; image size and RAM measured (§7).
- Host tests: the differential harness has no Python side any more; convert the remaining white-box Python tests
  that only tested internals into Rust unit tests, keep black-box tests (they drive the Rust binary via `--root`).
- Device: full qualification as for a release candidate (AGENTS.md), then the owner decides on publishing.

## 6. Risks and how to handle them

| Risk | Handling |
| --- | --- |
| Re-introducing fixed bugs (sleep, battery, charging, updates) | F-comments ported with code; differential tests; power last; overnight gates |
| A field or reason string changes and Reborn/report tooling misreads it | `y2p-contract` shared with Reborn; golden JSON fixtures from the Python side |
| Python daemon and Rust CLI race on shared files | same flock paths and write order; a test that runs both concurrently on one tree |
| Faster code exposes timing races (things that relied on 0.5 s startups) | watch for it in device checks; the F39 shutdown and F47/F48 sleep paths are timing-sensitive |
| Update/recovery breakage | Phase 4 tests full OTA + rollback on the player before publishing; the recovery image is C/shell and not touched |
| Slower debugging (no copying a `.py` onto the player) | keep `--root`, verbose logging and the trail lines; host builds for quick repro |
| Binary size | one multicall binary, `opt-level = "s"`, LTO, curl subprocess instead of TLS crates |

## 7. Success criteria (fill in Phase 0 and Phase 6)

| Metric | Phase 0 (Python) | Target | Phase 6 (Rust) |
| --- | --- | --- | --- |
| Platform RSS (all daemons) | ≈60 MB | < 15 MB | |
| Platform CPU, 1 h awake idle | (measure) | ≥ 5× less | |
| `y2-platform status --reborn --pss` latency | ≈0.5 s | < 50 ms | |
| Root image size | 130 MB tree | −22 MB or more | |
| Awake idle current | (measure) | lower | |
| Overnight drain | ≈2 mA (161, estimate) | not worse | |
| Host tests / container suite | green | green | |

## 8. Out of scope / later
- Host tooling in Python (release, build, validation, report fetch) stays.
- Plugin system (separate plan): the Rust platform makes a shared plugin SDK possible.
- Contract version changes (`platform_api_version` 2) only after parity is reached.
