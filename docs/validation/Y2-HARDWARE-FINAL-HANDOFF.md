# Hardware Final handoff — 2026-09-27

**PAUSED at the owner's request to reduce token use and continue with another
model. Not complete. No final candidate was built, no flash occurred, nothing
was pushed.** Resume from this file rather than repeating the initial audit.
The owner subsequently confirmed: **“the fix works tho”** for the volume-key fix.

## Repositories and preserved work

Both repositories are on `hardware-final`, in their original directories:
`/home/luca/Dokumente/Code/Y2Linux` and adjacent `Y2Reborn`.
Implementation heads before this documentation commit: Linux `4a35658`,
Reborn `47c545fea16dd347dd5c0feaf5a498de4312c04d`.

Pre-existing Linux modifications remain unstaged in roadmap-gap-audit.md,
platform-v1-roadmap.md, PLATFORM-V1-PHYSICAL-QUALIFICATION.md and
PLATFORM-V1-TELEMETRY-01.md. Do not commit or erase them accidentally. Their original
patch is `out/hardware-final/preexisting-linux.patch`. Reborn's pre-existing asset
pack deletions and untracked replacement pack/audit/docs remain untouched;
`out/hardware-final/preexisting-reborn-status.txt` records them. No identity config
was changed. Reborn lacks local identity config; commits used environment values
from Y2Linux's existing configured Luca identity, matching prior Reborn history.

## Device at handoff

Original Hardware 02 restored and healthy, receipt21 plus final cleanup:

- Linux `76bc8229580ec8d101008c5bad47419f2c110eae`, Reborn
  `95747e0a36c7b27beb44b8cdd54feda1813f2f3a`, candidate.4.
- Same boot `b93cda16-4e08-4ead-a74a-c85f4cccaa11`; taint0, CPUs0–3,
  pm_test none, root/data/SD ext4 error counters0, quick health OK.
- Both MMC qualification caps restored to25MHz; actual24,999,971Hz,
  eMMC8-bit MMC high-speed and SD4-bit SD high-speed,3.3V.
- Original `/usr/bin/reborn` SHA256
  `8027f751c543b8548e6b6832994d7c0724a6276b068242f24a93beeae3f17938`.
  Temporary bind mount removed; original app restarted, stopped playback.
  The installed app therefore does **not** retain the volume repair after handoff.
- Original BlueALSA daemon/supervisors restored after AAC experiment. No stopped
  supervisor, temporary daemon, benchmark or campaign observer remains running.
- Temporary `/run/y2-final-app`, `/run/y2-final-codecs` and generated listening
  fixture under `/data/music/Y2-Hardware-Final-Qualification` removed. Library
  rescanned. No existing music deleted; ordinary test/library history persists.
- Intentional persistent change retained: `/data/system/platform/rtc-policy.json`
  enables existing NTP→RTC synchronization, grounded in successful receipt05 and
  earlier receipts50–52. No RTC retention/alarm claim.

USB SSH: pinned existing host key, `root@10.42.0.1`, owner identity file
`/home/luca/.ssh/y2linux_ed25519`. Never print key bytes or bypass host checking.
Owner has AirPods Pro2 and wired headphones, **no USB meter**. Wired listening
was offered but not started before the stop request. No reboot/suspend/module
load occurred in this campaign segment.

## Physical evidence and limits

Raw/private directory: `out/hardware-final/20260927T122702Z-hardware02/`.
`out/hardware-final/active-run` points there; capture.py records command/stdout/
stderr/host UTC/exit status. SHA256SUMS seals completed receipts. Raw logs contain
identifiers/private network information; do not publish them wholesale.

- `00`,`01`: exact identity, broad census, CID/CSD, MMC error stats, power/IIO,
  timer_list, interfaces/services. EXT_CSD still needs confirming through debugfs.
- `02`,`03`: eMMC and SD13/25/50MHz cap sweep, guarded direct sequential/random
  readback plus buffered/fsync/fdatasync/directory/metadata/SQLite workloads pass.
  Actual13MHz cap is about12.5MHz with this inherited clock source; report actual
  separately. First50MHz large-block eMMC read/write33.08/26.44MB/s; SD19.19/17.95.
- `17`: three additional complete direct-readback repeats at50MHz per medium pass.
  Some overlapped product audio testing: stable-load evidence, not isolated maxima.
  No source default-speed promotion yet; source and device default remain25MHz.
- `18`: 1k and10k library+scan pass (5.90s/62.67s). **20k intentionally terminated
  for handoff; workload FAILED/command_failed despite outer capture rc0.** The
  clock was restored to25MHz during its unfinished run; never certify it as a
  50MHz result. `22` records termination. Rerun matched isolated library cases.
- `06`: USB TCP RX38.35–38.79Mb/s with704–713 retransmits, TX46.31–46.57Mb/s
  with0. DMA IRQ count stays6 despite large transfers: ECM actually falls back
  to PIO. Wi-Fi with AAC gives18.98–24.07Mb/s into Y2 and31.89–33.47Mb/s out.
  These are coexistence samples, not unloaded Wi-Fi ceiling or endurance.
- `07`: idle app voluntary context switches/s: control98,audio99,Bluetooth51,
  main65. Fixes below await post-fix idle A/B; no measured power-saving claim.
- `05`: checked NTP then RTC write/read/tick pass. Prior RTC date was2022.
- `08`–`11`: AirPods SBC stereoS16/44.1; owner clean both ears and working stem
  Play/Pause. Private D-Bus trace captures actual MPRIS Play/Pause from BlueZ;
  metadata changes present. Next/Previous not qualified. Initial resume failed
  because the old selected track path was unavailable; dedicated fixture fixed
  the test setup, not a source change.
- `12`: 100 one-millisecond sleeps/core, medians1.10–1.113ms;340.48s continuity
  across low-counter rollover, raw/monotonic difference about1ppm; RTC340s.
  All four highres/nohz flags and idle_sleeps observed. Not external long drift.
- `15`: temporary ARM FDK-AAC/BlueALSA5 bundle negotiates AAC,S16 stereo48k;
  owner **“clean no problems”**, short playback/coexistence, no recorded XRUN or
  decode error. Volume presses still restarted old app during this experiment.
  Original daemon restored automatically. Not optional-codec endurance acceptance.
- `16`:4→3→2→1→2→3→4 cores with per-core checked hash loads/idle and clockevents
  pass; same boot, SPM broken0, taint0 and clean filesystems. No automatic policy.
- `19`,`20d`: temporary fixed ARM Reborn. Automated20/0/30/10/25 volume changes
  preserve playing state, generation2 and start count; owner confirms keys work.
  `20` failed only on diagnostic build label “development”; `20b` hit a transient
  not-yet-ready A2DP peer; `20c` reconnect and `20d` pass. Preserve failed attempts.
- `04`: passive voltage/die-temperature trace under labelled workloads. Intentionally
  stopped rc143 for handoff. Peak observed CPU67.5°C/PMIC~60.3°C; no pack temperature
  or net battery current inference. Start future stress cool with bounded stops.

## Committed implementation

Linux:

- `e91fb1e`: power daemon deadline-based waiting replaces4Hz polling.
- `bd48c13`: DMA allocation/programming/abort/failure counters (programmed bytes
  explicitly not delivered bytes).
- `7db74bc`: pinned private FDK-AAC2.0.3, libfreeaptx0.2.2 and LDAC2.0.2.3+ABR;
  manifests/provenance/license limits. Default daemon keeps SBC, disables optional
  endpoints. `8e7799e` validates policy without invalidating historical fallback.
- `dc9f22e`: truthful Wi-Fi control failure/latency telemetry.
- `d46feaa`: real DMA alignment fix: standard RX no_skb_reserve quirk; conditional
  aligned TX skb copy preserving metadata, allocation failure→existing PIO.
  Exact ARM checksum assembly tested for2mod4 headers under QEMU. Hardware pending.
- `9bd1681`: exact-bin, sole-owner PWRAP/SPM/CCF DVFS; stock1196MHz/1.20V and
  1300MHz/1.25V opt-in, default1040MHz QoS cap. No new generic PMIC write permission.
  `4a35658`: keep safe MAINPLL/2 fallback if ARMPLL mux restoration fails.
- `45c7d42`,`78cf96a`: detailed power/idle evidence and exact gates.

Reborn:

- `5d4ce0e`: block idle control listener instead of100Hz polling.
- `f76f117`: idle audio waits for commands, preserving immediate command wake.
- `1e93600`: observation failures no longer counted as Wi-Fi disconnect/reconnect.
- `47c545f`: live volume after decoder DSP,5ms ramp, no reopen; partial-write,
  mute→raise, queued PCM, signed formats, unity and exact frame-count tests.

Diagnostic ARM app SHA256
`d1bd0b55f4a733a19f5705f8461dcb6c9db4b713828394f84f1286b104f80e9a`, at
`Y2Reborn/target/armv7-unknown-linux-gnueabihf/release/reborn`. It advertises
“development”, built from the shared worktree with Hardware02 SDK; **not a final
commit-certified release artifact**. Codec bundle and build logs:
`out/hardware-final-codec-research/target-codec-bundle.tar.gz`.

## Validation completed and critical next work

Targeted Linux/USB/codec/network tests pass;22 relevant tests in final radio/USB
run,9 Wi-Fi Rust tests,33 Reborn application tests, affected strict clippy and
ARM application/codec builds. DVFS objects+DT compiled before the last mux fix;
last fix host-tested only. Complete current-source kernel/DT/platform/Reborn
workspace/fmt/strictclippy/ARM/Buildroot/QEMU/installed-ARM/FFmpeg/ALSA/package/
ELF/source/license/preserve-data/fallback boundary is **NOT DONE**.

1. Finish independent DVFS review before packaging. Reviewer found mux/divider
   failure fixed in4a35658 but has not independently rechecked it. Check provenance
   in `out/hardware-final-power-source/`, voltage-readback mismatch containment,
   sleep/resume and all fault paths. In particular, current tests assume a failed
   voltage decrease retains old safe voltage; unexpected lower readback requires
   explicit assessment. Runtime path
   `/sys/module/cpu_dvfs/parameters/qualification_max_khz`; never raise installed
   Hardware02 voltage or claim new OPP physical acceptance.
2. Get persistent one-stage pm_test and Hardware02 first-overflow snapshot;
   distinguish USB IRQ overflow from radio restoration timeout. No current suspend
   fix or successful same-boot deep/RTC wake. Only advance after verified recovery.
3. Wired48 product/screen-off/rate switches and live volume: runtime
   `/etc/y2linux/audio-qualified.json` is reread per ALSA probe, allowing a temporary
   S16[44100,48000] profile without flashing. Restore after test; promote metadata
   only after evidence. No S24/S32/88.2/96 packing authorization.
4. Finish isolated storage/library/SFTP/USB/idle/frequency/load and recovery work.
   New DMA alignment must be measured on final kernel; do not falsely compare old
   Hardware02 DMA label to true DMA. Keep exact Hardware02 fallback.
5. Optional codecs are compiled but normal endpoints disabled; UI preferences
   only Auto/SBC. Auto still requires distribution/physical gates. Decide a truthful
   owner-private/manual mechanism if needed; do not turn short AAC listening into
   public-distribution or production Auto approval. AirPods cannot test aptX/LDAC.
6. SLIDLE: exact stock DCM WFI wrapper found, but need TOPCKGEN+4/PERICFG+0x18
   read-only snapshot and eligibility/ownership review. Inherited I2C/APDMA/MSDC
   gates may prevent entry. No module prepared/loaded; do not remove protections
   just to make a counter advance. Deep idle additionally requires exact PCM,
   CIRQ/timer/clock/one-core/context handoffs; suspend PCM is not a substitute.
7. SOC/current/NTC/low-battery reserve/VBUS topology remain concrete physical
   evidence gates. No meter available; no fabricated telemetry or threshold.
8. One consolidated final build and one owner install/qualification only after
   independent work. Use `tools/production/system_update.py` with Hardware02 base
   and exact fallback, **not package.py** (which creates Y2DATA). No intermediate
   image is currently justified. Full8h endurance and OTA/rollback remain pending;
   destructive root OTA application needs explicit owner approval.

Full build requires clean reviewed source in both repos. Preserve unrelated user
changes safely; do not silently commit them or reuse stale root binaries to pass.
Existing SDK/build: `out/hardware02-worktrees/Y2Linux/out/hardware02-build/`.
Prior packaging procedure: `out/hardware02-validation/assemble-package.py` and
check-installed-arm-final.py. Final path remains
`out/y2linux-hardware-final-candidate/`; it does not exist from this pass.

[Campaign](Y2-HARDWARE-FINAL.md), [capabilities](Y2-HARDWARE-FINAL-CAPABILITIES.md),
[qualification](Y2-HARDWARE-FINAL-QUALIFICATION.md) remain provisional ledgers.
