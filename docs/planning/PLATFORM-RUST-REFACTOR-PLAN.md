# Platform Rust refactor: from ported Python to idiomatic Rust

Status: **in progress (started 2026-10-29 on the owner's word).** The owner chose to do it right away, before the
hardware checks on test build 170 ("can you just do the refactoring plan now and after you are finished install via
ota"); the result goes to the player as test build 171, and the hardware checks then cover both. If one fails, 170
(sequence 11150) is the comparison point.

**Progress (2026-10-29, stopped at the usage limit):** wave 1 is merged on `main` (2f5b5075): the argparse emulation
is replaced by `crates/y2-platform/src/cli.rs` (table-driven, `y2-platform __arguments` feeds platform-contract.py)
and one shared core exists (y2p-core `json::{Object, ObjectExt, dumps_sorted}` with serde_json `preserve_order`,
`py::PyValue`, `mounts`, `Root::boot_id`, `record::atomic_json`, `lock`, `volume`, `sys` libc wrappers, `Runner`/
`Clock` carried by `Root` with `Root::fixture(..).with_answers(..)`); goldens unchanged except the parser's
deliberate ones. The golden suite sorts JSON before comparing: key order/spacing must be checked by comparing
raw output against a baseline binary (wave 1 did this for all 1 063 cases). **Next: wave 2**, one agent per group
(observation, connectivity, devices, storage, service, reports, power, suspend): adopt the core, typed records,
tests from `scripted` to `Root` injection (then delete the thread-local and `#[cfg(test)]` seams), crash paths →
refusal records, split long modules, merge the duplicated Python text helpers (suspend_text, power_text,
observation util, reports pyre), per-tick work in the daemons, connectivity: same-lease renew no-op and the DNS
fixture argv. Then host suite, build 171, OTA. Not built or installed yet; the player runs 170.

**Owner decisions (2026-10-29, §7):** replace the argparse emulation with a small declarative parser (exact option
names and `--opt=value`, errors exit 2, help says y2-platform); every fake-traceback crash path becomes a refusal
record (exit 1, JSON `{"state": "Failed", "failure": …}`); **keep the exact JSON output format** (json.dumps
spacing, key order, Python float repr) — only the internal types are unified; a DHCP renew of the same lease
changes nothing (no flush and re-add).

Owner (2026-10-29): put the Rust-only build on the player, review what works and what not, then "another pass
regarding refactoring / code quality" because the code is "more like ported python and not really well written /
optimised rust". This plan is written for a session that picks this up cold.

## 0. Read first

1. [PLATFORM-RUST-PORT-HANDOFF.md](PLATFORM-RUST-PORT-HANDOFF.md): layout, build/test/install commands, rules
   (git identity, no push or publish without the owner, OTA-only installs).
2. [platform/CONTRACT.md](../../platform/CONTRACT.md): who calls what (Reborn, scripts, the platform itself), the
   fixed defects, the golden suite.
3. The top entry of [NEXT-SESSION-HANDOFF.md](NEXT-SESSION-HANDOFF.md) and the device results of the newest test
   build in [roadmap-gap-audit.md](roadmap-gap-audit.md).
4. AGENTS.md: this is an architecture change of the platform; repeat the roadmap/gap audit before starting and
   before a build that carries the result.

## 1. Where the code stands (2026-10-29, main after the Rust-only merge)

- `platform/`: Cargo workspace, Rust 1.99.0, 37 locked crates (vendored, offline). Crates: `y2p-core` (shared
  helpers), `y2p-contract` (schemas shared with Reborn, a stamped copy lives in Y2Reborn), `y2-platform` (the one
  binary, 3.7 MB on the ARM target, multicall for y2-status, y2-health, y2-suspend-record, y2-power-audit,
  /sbin/poweroff, /sbin/reboot).
- ≈51 000 lines of non-test Rust, ≈17 800 lines of test Rust, **608 Rust tests** (unit + integration), plus the
  golden command suite (tests/platform_goldens, run by tests/test_platform_differential.py against the Rust
  binary) and recordings (`crates/*/tests/recorded`, made while the Python platform still agreed).
- The Python platform (`tools/platform`) is deleted; the Rust code was ported **1:1 from it** (owner decision
  2026-10-28: port first, fix after). The port was proven equal on ≈400 command cases and ≈5 700 argument lists.
- Largest files: service/update.rs 1775, observation/snapshot.rs 1748, storage/media.rs 1347,
  storage/internal.rs 1254, argparse.rs 1242, reports/maintenance.rs 1212, observation/bench.rs 1167,
  suspend/audit.rs 1162, connectivity/wifi.rs 1134, reports/fields.rs 1057, power/battery.rs 1027.

Device numbers to beat or keep (build 169): 4 daemons 15.2 MB RSS / 4.6 MB PSS, 19.6 s CPU per hour awake idle
(power 9.1, service 8.1, ssh 2.4, connectivity 0.2), `status --reborn --pss` 0.48 s (mostly Reborn's own
`rebornctl metrics --json`), restart trail 0.46 s, boot to application_ready 36.8 s.

## 2. What makes it "ported Python" (the debt, with locations)

Counts are rough (grep over non-test files, 2026-10-29).

### 2.1 Emulation of Python itself (≈3 200 lines)
Written so the output stayed byte-identical to Python's, including where nobody depends on it:
- `y2p-core/src/py.rs` (int()/float() parsing, truthiness, `exact_int`, uuid4, Python's `round`), `os_text.rs`
  (os.fsdecode surrogates for non-UTF-8 argv/env), `PyRandom` in observation_support.rs (CPython's Mersenne
  Twister, for the same random bytes as Python).
- `y2-platform/src/argparse.rs` + `argparse/python.rs` + `argparse/unicode.rs` + `argparse.json`: a reimplementation
  of CPython 3.15 argparse (abbreviations, ambiguity, "maybe you meant", usage text naming `main.py`).
- `commands/reports/pyjson.rs`, `pyops.rs`, `pyre.rs` (Python json/str/re semantics for the scrubber),
  `commands/suspend/pyjson.rs`, `commands/service/python.rs` (realpath, `str()` of JSON, `json.loads(strict=False)`).
- `Failure::Crash` / `Raise::Crash` (≈60 sites): where Python would have died with a traceback, Rust prints a fake
  traceback and exits 1 after the same file changes. Each is a place where the real answer should be a proper
  refusal record.
- ≈350 comments that explain behaviour as "as Python did" instead of saying why.

### 2.2 Several implementations of the same thing
- **Five JSON value types**: `serde_json::Value`, `y2p_core::json::Ordered` (key order kept), `power_json::{Json,
  Object}`, `connectivity::wifi::Json`, `reports::pyjson::Py` (+ `reports_sqlite::Value`). Each group's agent made
  its own.
- **mountinfo parsers / Mount types**: boot.rs, observation/snapshot.rs, devices/storage.rs, reports/media.rs,
  storage/sys.rs, service/python.rs, connectivity_journal.rs; `mounts()` again in storage/media.rs and
  storage/internal.rs.
- **boot_id()** in 9 modules (some return `Value`, some `Option<String>`).
- **atomic_json** in y2p-core/record.rs, reply.rs, storage/sys.rs (`atomic_json_mode`), suspend/receipt.rs.
- **data volume** (verified /data) in service/storage.rs, reports/volume.rs, devices/transfer.rs.
- Locks: `y2p_core::lock`, reply.rs `lock`, storage/sys.rs `try_lock`/`lock_within`, and raw `libc::flock` in
  maintenance.rs, transfer.rs, suspend/helper.rs, plus `File::try_lock` in update.rs and suspend.

### 2.3 Untyped records
Records are `Map<String, Value>` navigated with `get(..).and_then(..)` chains and Python truthiness, not structs.
y2p-contract has the schema *names* but few types. Typed `serde` structs for the records (power.json,
media-mount.json, sd-write.json, download.json and the update journal, boot.json, network/dhcp/dns/wifi records,
suspend receipts, report state) would remove most `get`/`truthy` code and make field typos compile errors.

### 2.4 Structure, errors, idioms
- Very long functions mirroring the Python ones (update stage/apply, media lifecycle, internal setup, snapshot).
- `unwrap()` ≈330 and `expect(` ≈190 outside `_tests.rs` files (many in in-file test modules; audit the rest:
  a panic aborts, `panic = "abort"`), `.clone()` ≈430, `unsafe` ≈230 (libc calls: flock, statvfs64, ioctl,
  signals; wrap each family once in y2p-core with a safe API).
- Test seams: ≈75 files with `#[cfg(test)]` hooks (per-thread replacements for clocks, `command`, PM stores,
  `launch`, space, tokens, network waits, renames) that the Python tests used to patch. Replace with explicit
  injected interfaces (a `Clock`, a command `Runner`, a `System` for sysfs/PM writes) passed in a context.
- Configuration as environment for tests (`Y2_ROOT`, `Y2_FIXTURE_COMMANDS`, `Y2_FIXTURE_CALLS`,
  `Y2_DAEMON_TICKS`, `Y2_FIXTURE_TOKEN`, `Y2_ARGPARSE_ECHO`) read deep inside the code: gather in one context.

### 2.5 Efficiency (measure first; only where the player gains)
- The daemons poll: power-daemon samples BAT0 every second, service-daemon ticks every 3 s (dark: longer),
  connectivity waits on fds. Candidates: uevent/netlink for power_supply and block changes, timerfd deadlines
  instead of fixed ticks, fewer sysfs reads per tick. Target: idle CPU well below 19.6 s/h, and longer CPU-off
  stretches awake (deep idle is blocked by other things too: see the awake-idle memory/notes).
- `status --reborn --pss` spends its time in `rebornctl metrics --json` (Reborn side).
- Binary size 3.7 MB (opt-level "s", LTO): check what the regex/flate2/tar/argparse tables cost.

### 2.6 Leftovers of the Python era (small, do first)
- Fixture argv for the DNS lookup still spells the Python one-liner (`/usr/bin/python3 -I -c …getaddrinfo…`,
  connectivity/wifi.rs and its goldens/recordings); on the player the lookup is Rust `getaddrinfo` already.
- `Y2_PLATFORM_PYTHON` in tests/dispatcher.rs; usage texts say `main.py` (part of the CLI contract: help text
  only, no caller parses it — owner decision whether to rename to y2-platform).
- Development scripts that run Python **on the player** with the old platform modules can no longer work (no
  Python there since build 169): tools/development/power-session/{deepsleep-clean-session.py,
  deepsleep-idle-followup.py, deepsleep-product-session.py, power-press.sh, power-screen.py},
  qualify-cpu-fix02.py / qualify-cpu-fix03.py (`charger()`), tools/usb/qualify.py (remote status script),
  observe-platform-v1.py (pins a deleted file). Retire them or give the platform the commands they need.
- Docs that still name deleted tests or tools/platform paths: docs/release/Y2-COMMUNITY-BETA-FEATURE-AUDIT.md,
  Y2-COMMUNITY-BETA-FEATURES.json, docs/planning/CLAUDE-DEEP-SUSPEND-PROMPT.md, tools/reports/worker comment.

## 3. What must not change (the contract)

Before changing any output, check CONTRACT.md "Callers". Fixed points:
- **Reborn** (`reborn-platform/src/client.rs`, `storage.rs`, `power.rs`) parses stdout JSON and matches
  `failure`/`reason` strings by hand (F52 was such a mismatch); exit 0 vs non-zero decides success except for
  `health` (exit 1 is evidence). y2p-contract schema names are shared and stamped.
- **Shell scripts** grep compact JSON (`S43y2-time`: `"state":"Online"` in network.json;
  sftp-server: `"state": ?"Ready"`): compact separators of those records are contract.
- **initramfs** `/sbin/y2-platform-start` and the marker `y2-platform-v1`.
- Persistent file formats on /data (journals, receipts, boot.json, update journal, report state, power journal):
  readers are Reborn, the error-report scrubber/Worker, tools/reports and the UserReports analyses. A format change
  needs a migration or a version.
- The multicall names and the subcommand/argument forms callers use (CONTRACT.md tables).

Free to change (nobody depends on it, verify per case): JSON key order and spacing on stdout where only Reborn
reads it as JSON, Python float repr where no reader compares text, argparse corner cases (abbreviations, "maybe
you meant", exact error wording) beyond exit code 2, fake tracebacks (become refusal records), PyRandom streams.
Each such change shows up as a golden diff: review and re-record it in the same commit.

## 4. Safety net and how to use it

- `cd platform && cargo test --offline --locked && cargo clippy --offline --locked --all-targets` and
  `cargo check --offline --locked --target armv7-unknown-linux-gnueabihf` (the player is 32-bit: time_t/c_long,
  64-bit file calls; use `y2p_core::wide()`).
- Golden command suite: `tools/development/host-tests.sh tests.test_platform_differential …` (see the harness
  docstring for the re-record command). A pure refactor changes no golden and no recording.
- Recordings: `Y2_RECORD_GOLDENS=1 cargo test …` re-records `crates/*/tests/recorded`; only for an intended change,
  with the diff reviewed.
- Full host suite before a build: `tools/development/host-tests.sh`.
- Device: anything touching power, sleep, charging, storage, update or radios needs a test build and the owner's
  check (OTA install only; ask before anything that powers the player off or needs button presses).

## 5. Phases (each its own commits; never behaviour and structure in one commit)

1. **Leftovers** (§2.6) and an `unwrap/expect/unsafe` audit list (no changes yet).
2. **One core** in y2p-core: `Mount`/mountinfo, boot_id, atomic records (with mode and durability options), the
   lock API (exclusive, try, within-deadline, shared), the verified data volume, safe wrappers for the libc
   families. Replace the copies group by group; goldens unchanged.
3. **One JSON model**: serde_json with `preserve_order` (vendor indexmap) or `Ordered` everywhere; delete
   power_json::Json, wifi::Json, pyjson::Py where the order/float behaviour is not contract.
4. **Typed records** in y2p-contract (shared with Reborn where Reborn reads them; re-stamp with
   `release.py sync-contract`, commit Reborn with the identity env vars).
5. **Context and injection**: one `Context { root, clock, runner, system }` passed down; remove the `#[cfg(test)]`
   seams and env-var plumbing; tests construct a context.
6. **Python emulation out** where §3 allows: argparse → a small declarative parser keeping exit code 2 and the
   argument forms (owner decides on help text); Crash paths → refusal records (each a behaviour change: list them,
   owner reviews the list); py.rs/os_text/pyre/pyops reduced to what the contract needs.
7. **Split the long modules** along their responsibilities (update: source/download, staging, journal, apply;
   media: lifecycle, check, window, repair, format; snapshot: per section).
8. **Efficiency** (§2.5): measure on the player (plan §7 method in PLATFORM-RUST-PORT-PLAN.md: idle sampler 600 s,
   per-daemon CPU/RSS), then event-driven waits; a test build and an overnight drain to prove no regression.
9. **Docs**: CONTRACT.md as the Rust contract (drop "as Python" wording), comments say why, not "as Python did".

Each phase: entry note in roadmap-gap-audit.md when it reaches a test build; numbers before/after on the player.

## 6. Test quality follow-ups (from the 2026-10-29 test port)

The Python unit tests were ported by eight agents; where Rust had no hook for what Python patched they adapted:
- Real clocks instead of fake ones (power coordinator grace, report retry times, supplicant timeout duration,
  update network wait 0.05 s under test); some waits are real (Wi-Fi removal 2–3 s, wakeup_count 2 s, collect 3 s).
- Source-text pins instead of behaviour (shutdown order "panel dark before Reborn stops", transfer check order,
  reborn-last init order, service loop gating, media inventory identity, failure_output kwarg).
- Dropped sub-cases: cpu-idle `budget_readback` (-1 write ignored), PM store returning None, internal
  `music_root` assertions (function never ported), `configure(sbc_abr='on')` (not expressible in Rust types).
- Faults made with files instead of patches (0444 files, directories in place of files, symlinks to /dev/null);
  one rollback test runs only as non-root.
- `#[cfg(test)]` seams in suspend/helper.rs, service/update.rs + mod.rs, reports/maintenance.rs,
  storage/media.rs, internal.rs, observation/bench.rs, service/process.rs (`launching`), and a 0.2 s busy-lock
  retry in test builds (y2p_core::lock::settle, storage/sys.rs): a forked child shares a test's lock until exec.
Phase 5 (injection) should turn most of these back into direct tests.

## 7. Owner decisions to ask for (when the phase comes)

- Rename `main.py` in usage/help texts to `y2-platform`? (No caller parses help.)
- Replace the argparse emulation (abbreviations like `--hol` for `--holder` would stop working; no caller uses
  them)?
- Turn each fake-traceback crash path into a refusal record (list per command)?
- JSON spacing/order on stdout: keep `json.dumps` style or switch to compact everywhere (Reborn parses JSON;
  scripts grep only the records named in §3)?
- DHCP renew flushes and re-adds the same address (brief outage, Python behaviour kept): change to a no-op renew?
