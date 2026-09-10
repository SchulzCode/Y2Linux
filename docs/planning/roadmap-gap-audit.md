# Y2Linux roadmap and gap audit

Original audit date: **2026-09-09**. Documentation/planning only. No implementation, build,
BOOTIMG creation, device access or flash. Repository baseline `00b4afa`; GitHub
issues #1–#27 and M0/M1 inspected before adding five deferred epics #28–#32.
Y2PlayerNative has no open application issues and its checked-in content remains
blueprint/planning material; no native platform readiness is inferred from that.

## M3-AUDIO-01 manual-deployment boundary — 2026-09-10

[Integrated result](../build/y2linux-m3-audio-01-result.md) now passes offline
build, thirteen targeted tests, resolved audio/rail DT, expanded RAM/BOOTIMG and
rootfs checks. Reconciled every coverage row against the same physical DEV-02
baseline and owner authorization: no new audio hardware result exists. Core
acceptance stands, #27/#28 reconnect remains deferred, #29 M3 stays active/open;
M4/M5/GPU/final-platform gates and Y2PlayerNative deferral remain unchanged.
The owner next updates removable SD content and manually flashes BOOTIMG only.
No assistant flash/reboot, internal-storage write or repeat M2 pass occurred.

## M3 activation audit — 2026-09-10

The owner explicitly authorizes one integrated native-audio implementation/build
and manual BOOTIMG deployment, using physical DEV-02 as the baseline (checkpoint
`d76e57f`). This supersedes the reconnect prerequisite below. M2 core/Buildroot is
substantially qualified; #27 and #28 retain USB reconnect for later whole-platform
qualification. Do not repeat reconnect experiments or reopen satisfied #22–26.

Reconciled the coverage matrix and current open issue snapshot with the retained
[physical pass](../knowledge/y2linux-dev02-live-qualification.md) and a narrow SSH
entry check. Four CPUs, 954660 KiB MemTotal, bounded 256 MiB RAM pass, writable SD
Buildroot, protected eMMC, initial SSH/ECM/ACM, visible display, wheel/buttons and
PMIC/core buses retain their physical acceptance. Audio remains UNTESTED; VGP2's
live selector is 2.5 V with no consumer and must be constrained to 1.8 V before DAC
operation. No sound card exists in DEV-02. No new power/radio/GPU acceptance exists.

**M3 ACTIVE (#29)**: ALSA/ASoC DL1 DMA/IRQ -> second I2S CON3 -> upstream CS43131,
regulator-owned power and controlled headphone playback. Speaker identity/routing,
jack reporting and higher rates remain separately qualified subpaths. One clean
kernel build, one audio-equipped Buildroot build, relevant static/DMA/config/DT
checks and current layout/BOOTIMG validation. Stop for owner BOOTIMG-only flash;
no internal storage, loader, calibration or partition writes. M3 remains open until
real clean headphone playback, repeated start/stop and rate tests succeed.

M4 power, M5 connectivity, GPU/lima and final whole-system qualification remain
subsequent platform gates. Y2PlayerNative remains deferred. Unchanged provenance,
recovery research and M2 qualification are not repeated.

## DEV-02 live qualification boundary audit — 2026-09-10

Owner-authorized complete live pass on physical Linux `6.18.0-y2linux-dev02`,
source checkpoint `a34a360` plus preserved DEV-02 working changes. Live DT hash
matches DEV-02; full flashed BOOTIMG was not read back. Reassessed every coverage
row, current #16/#22–32, retained baseline, raw evdev, RAM tests, DRM/sysfs and
host disconnect evidence. [Complete report and evidence](../knowledge/y2linux-dev02-live-qualification.md).

Buildroot PID1 runs on writable removable Y2ROOT, four CPUs online, MemTotal
954660 KiB. The completed 256 MiB allocator test passes; a separate exhaustive
run was capped at 600 seconds without observed memory errors. All eight buttons
and both wheel directions produce evdev events; wheel/I2C IRQs agree and no
transport timeout remains. Owner confirms native display pattern after standard
fb0 unblank. Internal eMMC is disabled and unexposed. PMIC/core buses, initial
ACM enumeration, key-authenticated SSH and USB Ethernet work.

**USB reconnect FAILED.** The host sees removal, then no enumeration after owner
reconnect. The owner reports USB works only after restarting; no post-reconnect
uptime/continuity measurement exists. Do not perform the second cycle or close
M2. The generic core/Buildroot foundation is substantially qualified, with the
real #27/#28 reconnect blocker replacing the old research sequence. A later
shared-framebuffer checksum mismatch is corrected by hiding/restoring the cursor;
visible display acceptance and independent allocator-test success stand.

Safe runtime/module/config changes are represented in canonical Buildroot/host
sources and retained readbacks. Missing evtest/modetest and longer syslog records
are configured for the next userspace build, without triggering a rebuild.
Close satisfied #22–26 narrow slices; retain long-run/DMA, full power, routing,
production and final platform concerns under #29–32. Optional physical UART #16
remains separate, not an M2 closure prerequisite. No new kernel/BOOTIMG, flash,
assistant-initiated reboot, internal storage access or unchanged provenance qualification occurred.

M1 COMPLETE; M2 ACTIVE solely around remaining real qualification/fixes, chiefly
USB re-entry. M3 audio is the next platform phase once that foundation is usable;
M4 full power and M5 connectivity follow with shared dependencies. GPU/lima and
final whole-system qualification precede Y2PlayerNative under the owner's new
explicit platform-first gate. No later hardware phase is activated by this audit.
After the failed reconnect the owner restarted the device; SSH returns at33.32s
with the old tmpfs token missing, confirming a new boot. Runtime fixes persist
and actual ACM capture now works after the host rule. A bounded SD recorder is
installed/tested so a future failure log can survive loss of USB.
A USB-focused DEV-03 requires device-side failure evidence and a justified change;
host absence alone does not identify which guard/callback failed.

## DEV-02 root handoff / wheel correction audit — 2026-09-10

Owner's new photograph confirms SD partition mmcblk0p1, ext4 recovery, exact
Y2ROOT UUID, writable remount and handoff attempt. PID1 then dies by SIGILL
(exitcode4) at23.723s. Visible fbcon remains positive. SD block/filesystem access
now has physical evidence; Buildroot init/SSH do not. The raw fault PC is absent.
All other coverage rows retain their latest evidence and open gaps; no milestone
closes, M2 stays active. No loader/partition/internal-eMMC write or new hardware
subsystem is authorized. Preserve the observed RAM/display/SD configuration.

DEV-02 combines standard ARM userspace Thumb/signal and kuser compatibility,
framebuffer device-node availability, userspace fault reporting/preflight, and
an evidence-backed MT6582 I2C combined-message correction. v6.18 selects WRRD
only with auto_restart, but MT6582 uses the older non-auto-restart compatible;
DEV-01 logs op1/transactions2 with only one byte of completed TX DMA. Restore
combined WRRD for this exact compatible and test the controller-level selection,
without changing APT32F packet framing or adding retries.

Use one new kernel build, existing pinned Buildroot binaries, targeted regression
checks and fresh actual layout/BOOTIMG validation. Keep the already written SD
rootfs usable; the new DRM module loads from rescue before switching. A later
SSH module-index refresh can bring the SD module directory to the new kernel.
Stop at owner BOOTIMG-only flashing, then collect all independent failures again.

## DEV-01 retained rescue/RAM/display checkpoint — 2026-09-10

[Capture-02](../knowledge/y2linux-development-hardware-result.md) now retains all
704 kernel records, visible-console supporting scanout/vblank state, four CPUs,
MemTotal954668KiB and HighTotal228352KiB. RAM/HIGHMEM visibility is physically
observed; stress and full DMA containment remain open. Wheel remains regressed
with ten I2C timeouts; the test-pattern tool cannot open/query fb0. Rescue fallback
is confirmed without Y2ROOT; SD boot/ECM traffic/SSH/reconnect remain unqualified.
M2 ACTIVE/exit incomplete, later gates unchanged. Transfer the existing rootfs to
the owner's Mac, identify its removable card, then prepare exact manual commands.
No hardware scope expansion or new build is needed for this transfer.

## DEV-01 owner-visible console update — 2026-09-10

The owner reports a visible Linux console after manually flashing DEV-01; SD
rootfs is not installed. Host USB descriptors independently identify Linux
6.18.0-y2linux-dev01 and composite ACM+ECM enumeration. This is positive visible
output, superseding the prior black-screen outcome for this boot; display remains
PARTIAL pending broader qualification. [Evidence and host access limitation](../knowledge/y2linux-development-hardware-result.md).

No new RAM, wheel, SD-root, network, SSH or reconnect success is inferred.
M1 COMPLETE; M2 ACTIVE/exit incomplete, later phases unchanged. Continue the
already authorized removable-SD preparation after exact host device identification;
no assistant write or guessed disk target. The existing DEV-01 BOOTIMG can remain.

## Y2LINUX-DEV-01 manual-deployment boundary audit — 2026-09-10

Offline implementation and the combined candidate are now built and validated:
[DEV-01 result](../build/y2linux-dev-01-result.md), [memory reconciliation](../knowledge/development-memory.md).
Rechecked every hardware coverage row against the retained BASELINE-03 capture
and owner observations. No newer physical capture exists, so **no physical
classification or milestone exit is promoted**. M1 COMPLETE; M2 ACTIVE/exit
incomplete; audio/application, radio, product PM/security and later milestones
retain their gaps and owners.

The owner-specified ED25519 public key is packaged; no host private key was
accessed. Kernel/module, large-memory DT, rescue archive, actual pinned Buildroot,
ext4/tar content and key policy pass offline validation. Rootfs/ECM/SSH now have
built implementations; their physical state remains untested. SD writes and
BOOTIMG flashing remain owner-only. Next action is one manual deployment and
combined visible-display, wheel, RAM, SD, persistent USB/network/SSH collection.
No unchanged ROM/recovery provenance or duplicate clean build was repeated.

## Y2LINUX-DEV-01 integration scope audit — 2026-09-10

Fresh-session local baseline **2efcdc1**, clean main; current open issues #16,
#22–32 agree with the local BASELINE-03 checkpoint. Reviewed all coverage rows,
retained capture-02 (102314 bytes, all 606 kernel records, 180 seconds), source
and artifact metadata. No new hardware success is claimed. Owner confirms an
earlier wheel-working integrated baseline; its exact artifact identity is not
established in the retained canonical captures. Treat current eight -110 reads
as an integration regression and preserve register-zero repeated-start framing.

The owner now explicitly authorizes **one integrated development candidate**:
post-modeset display fixes/diagnostics at 480x360; wheel transport correction;
large RAM with explicit reservations and HIGHMEM; pinned ARMv7 hard-float glibc
Buildroot; removable-SD ext4 LABEL=Y2ROOT; rescue initramfs; persistent ACM+ECM;
key-only Dropbear. This supersedes D08 as the final target, identity-only SD and
temporary USB disconnect deadlines. Internal eMMC remains protected; no assistant
flash, SD write, loader/partition/calibration/Android write is authorized.

Every physical coverage classification below remains unchanged. Display remains
PARTIAL with failed visible output despite successful DRM initialization. Wheel
remains PARTIAL with working historical owner observation and current transport
regression. RAM, removable SD and USB are PARTIAL; Buildroot/rootfs/SSH are
PLANNED pending implementation/build and then physical qualification. SMP and
buttons retain their narrow successes. All power/thermal, audio, radio, recovery,
production security, time/entropy and application gaps retain their existing
owners and limitations. M1 COMPLETE; M2 ACTIVE/exit incomplete; M3/application
and later product phases are not activated. Existing #22–28 own this work;
#30 retains sustained-workload/power qualification and #32 production policy.

Proceed offline together, preserving attribution/identity and unchanged donor,
ROM and recovery provenance. One clean kernel tree, one pinned Buildroot build,
changed-subsystem tests and full new memory/layout/BOOTIMG/rootfs validation.
Record exact artifact hashes and remaining physical tests. Stop for owner manual
SD identification/write and BOOTIMG-only deployment. Permanent USB observation
is a development capability, not sustained power/thermal qualification. Re-audit
before milestone closure using the subsequently collected real hardware results.

## BASELINE-03 hardware evidence update — 2026-09-10

Owner capture-02 restores complete kernel/PID1 observation for180seconds, including
successful live-PHY adoption, DSI park, cold Linux PHY initialization, DRM/fb0
registration and module return0. The owner still sees black: visible display
acceptance remains failed. Four CPUs and existing navigation/volume/power input
operate. Wheel EINT55 now fires, but eight I2C reads time out with no controller
completion IRQ; transport debugging replaces the previous unobserved-wheel gap.
[Retained result](../knowledge/m2-baseline-hardware-result.md#latest-baseline-03-logs-the-display-handoff-screen-remains-black).

Next combined source iteration: diagnose actual display output and wheel I2C
completion, include standard runtime state/failure diagnostics and repair snapshot
line framing. Preserve the working single USB writer. No guessed rate/rail/panel
changes, repeated per-register images or milestone promotion. M1 COMPLETE;
M2 ACTIVE, exit incomplete. D08, storage/PMIC firewalls, owner300s limit and later
milestone exclusions remain. This is an evidence/documentation update, not a
new build, hardware scope authorization or five-minute/reconnect qualification.

## BASELINE-03 observation and packaging review — 2026-09-10

BASELINE-02 owner reports grey/black after LK. The retained 180-second capture
identifies baseline2 at high-speed ACM, sends LOG1, but receives zero bytes
(exit2, no protocol header). No kernel/PID1 execution point or display failure
instruction is established. This is a regression of observation, not display
success. All newly tested BASELINE-01 results remain historical, not silently
revalidated for BASELINE-02.

Same M2 hardware scope, revised **initramfs packaging/startup**: compile the
MediaTek DRM aggregate as the sole loadable module, with required DRM helpers
built in. Package the exact module with PID1 in the existing bounded RAM-only
initramfs. Start it once through finit_module in a separate child after LOG1 is
active; PID1 must never wait synchronously for display probing. Keep PID1 on CPU0
and the load child on previously observed CPU1 using normal scheduler affinity;
this reduces local CPU starvation, not global bus/lockup risk. Kernel/driver logs
still use the unchanged /dev/kmsg-to-CDC relay, without competing USB writers.
Repair unpowered DSI host-transfer access and add phase messages. No guessed
panel/clock/rail changes or extra per-register image.

Audit module dependencies, ABI/exports and exact initramfs membership, include
module bytes in the D08 unpacked-file/layout accounting, test child errors and
nonblocking reap, then one clean combined build/package. No second reproducibility
build or unchanged ROM/recovery/source-tree hashing. D08, protected partitions,
PMIC/storage write firewalls, five-minute owner limit and M3/M4/M5 gates remain.
M1 COMPLETE; M2 ACTIVE; stop at BASELINE-03 for manual owner BOOTIMG flash.

## M2 integrated hardware checkpoint — 2026-09-10

[BASELINE-01 physical results](../knowledge/m2-baseline-hardware-result.md): two
complete initial kernel captures confirm four CPUs, shared PWRAP/MFD/regulator
probes, all five navigation channels, both volume keys, PMIC power key through
EINT25, and native microSD/eMMC identity. DSI rejects the inherited PLL before
handoff; wheel rotation has no captured IRQ/transfer/events. These are partial
core results, not M2 closure. M1 COMPLETE, M2 ACTIVE; M3/M4/M5 gates unchanged.

Continue one combined source iteration: repair live-PHY adoption and capture
window defaults; retain D08, PMIC/storage write firewalls, USB logging, the
295-second USB deadline and owner 300-second limit. The five-minute disconnect
report is consistent with the deadline but not captured in the 45-second files.
No memory/rail/production scope expansion or independent per-register flash.

## M2-BASELINE-01 integration authorization — 2026-09-10

Baseline **4dde305** (clean main). The owner explicitly supersedes per-register
and per-driver flash gates with one coherent offline Linux 6.18 core integration.
Reviewed canonical config/DT/PID1/USB, retained USBACM-03 and D08 evidence,
the 36-commit donor and additional history against every coverage row below.
No new hardware result exists: M1 COMPLETE, M2 ACTIVE/exit incomplete, M3 gated,
M4/M5 deferred. No coverage status is promoted by compiling a driver.

Implemented at entry: CPU0/GIC/GPT, early stopped AP watchdog, guarded LK text,
PIO CDC ACM with kernel/PID1 relay, offline USBACM-04 lifecycle and polled
navigation GPIOs. CCF/pinctrl/EINT/I2C/wheel/PMIC MFD/regulators/keypad/DRM/MMC/SMP
are documentation or donor source only. The two previous offline candidates no
longer require separate flashes before the combined baseline.

Integrate evidenced controller contracts together; preserve normal probe errors
and deferral. Remove USB's dependence on display ownership and competing PWRAP
access. Review RAM in the same pass; retain D08 if the LK heap/live DMA ownership
boundary still prevents a defensible larger interval. Storage must enforce no
writes at the kernel boundary. No charger policy, audio, radio, GPU applications,
rootfs installation, protected partition, loader or calibration changes.

The 60-second experiment limit is superseded only for this candidate by a
**maximum five-minute owner test from power-on**. Proven early AP watchdog stop,
Linux/PID1 and USB observation plus retained BOOTIMG recovery justify observing
idle core probes longer. This does not qualify thermal/charging policy or permit
stress: no CPU load test, charging experiment or automatic reboot. USB/PID1 must
remain observable within that interval; early stop on abnormal warmth, repeated
faults or lost observation. End with the existing manual BOOTIMG restoration.

One combined source/config/DT review, one clean build (repair errors in that
build if necessary), shared-provider tests and final layout/package validation.
No duplicate reproducibility build or unchanged ROM/source/recovery audits.
Stop at the resulting BOOTIMG for the owner's flash; classify the complete live
log and fix independent failures together in the next source iteration. M2
closure still requires the standing strict milestone audit and real exit evidence.

## Additional reverse-engineering evidence — 2026-09-10

Baseline **e4a0a81**. Reviewed the blueprint, current issues #1–32 (open
#16/#22–32), every coverage row, canonical source and retained physical results
against the [additional research](../knowledge/reverse-engineering-audit.md).
External f96d4c7 pins our existing donor 53fb57bb. Missing private captures and
stale historical statements are identified in the research matrix.

**Every hardware classification remains unchanged.** USBACM-03 remains the
11,754-byte kernel/PID1 capture. USBACM-04 and M2-INPUT-01 have offline validation
only; host inventory found no Y2 attached. M1 stays complete, M2 active/exit
incomplete, M3 gated, M4/M5 deferred beyond shared M2 prerequisites. D08, the
rootfs/application plan, recovery rules and older-board FM exclusion remain.
No new boot, build, flash or unchanged provenance requalification occurred.

The implementation strategy advances where evidence is stronger:

- #24/#28: validate the navigation candidate, then adapt EINT/I2C/AP-DMA/wheel
  and keypad by dependency. Upstream 6.18 already fixes the donor's PMIC
  single-key null-pointer problem.
- #23/#28: one WACS2 owner shared with USB; distinguish the wrapper IRQ storm
  from PMIC EINT25. Derive CCF/pin/rail contracts; absent PLL probe files cannot
  validate approximate donor rates.
- #25: use the later 480×360 correction, cold PHY/reset findings and live-pipeline
  failure history. v6.18 already protects the OVL address from PITCH_MSB writes;
  port remaining variant differences instead of redundant 6.12 fixes.
- #22 still needs loader/DMA/lowmem ownership despite main-bank agreement.
  The modem/HYP collision reinforces high exclusions. #26 can validate reported
  MSDC operation after clocks/pins/rails/DMA; there is no local native read result.
- #29 retains CON3/VGP2-brownout evidence for M3; #30 retains separate charger/AP
  watchdogs and unresolved thermal/DVFS/suspend limits. #31 retains cold radio
  calibration and failed EDR experiments; #32 retains address-space/recovery
  pitfalls. No later phase is activated.

Execution remains navigation/USB evidence → corrected shared providers and useful
RAM reconciliation → wheel/power, display and storage/rootfs → real M2 stability
and exit audit. The research report specifies the next provider contract.
Existing issues absorb the findings; no new issue/milestone fan-out. Physical
flashes remain owner-performed; protected partitions/calibration are untouched.

## M2 activation audit — 2026-09-09

### Donor adoption and GPIO input scope — baseline 8ebc800

2026-09-09. The owner authorizes donor-assisted Linux **6.18** implementation,
build/test/Git/GitHub and live host logs, with manual owner BOOTIMG flashes.
Reviewed the complete 364-file/36-commit donor inventory, local blueprint M2/M3
and later gates, current GitHub issues #1–32, canonical config/DT/source and
retained stock/RAM/Linux evidence. [Subsystem audit and dependencies](../knowledge/donor-audit.md).
No new physical result: USBACM-03 remains the 11,754-byte kernel/PID1 success;
USBACM-04 robustness remains pending. Host USB is accessible outside the sandbox;
no Y2 was attached at this inspection. Identity/recovery/provenance is unchanged.

Every coverage row below was reconsidered. Hardware status stays unchanged
except that other navigation buttons now have a concrete **PLANNED** evdev
slice; source availability does not promote hardware readiness. The former
"common providers not established" blocker now has substantial donor contracts
to correct/forward-port. SMP, CCF, EINT, PMIC, input, display, storage, audio,
radio and haptic source availability is detailed in the separate reuse matrix.
Thermal/DVFS/idle, full memory/DMA ownership and production/recovery gaps remain.
FM reception is inapplicable to the owner's older board; retain donor knowledge
only for other revisions. M1 complete, M2 active/exit blocked, M3/app deferred.
All M2 rootfs/display/input/storage/observation/battery criteria still apply.

Supersede the rigid #27→#22→#23→#24→#25→#26 order. Keep USB robustness open and
preserve both USB candidates; use GPIO navigation→upstream gpio-keys-polled→
evdev→USB as the first coherent slice under #24/#28. D08 stays unchanged.
The five inherited input lines can be sampled without EINT, I2C DMA, supply,
clock or pinmux writes. Refuse output-configured lines. Poll at 20 ms through
the input core; bounded userspace evdev capture must preserve USB/heartbeat.
This is real input integration, not a hardware-success claim. General pinctrl,
EINT, wheel rotation, keypad and PMIC power key remain subsequent dependencies.

Next combine corrected CCF/PWRAP/GPIO/EINT/I2C foundations by consumer dependency;
reconcile a useful large RAM expansion under #22 instead of arbitrary tiny
increments. DRM/panel/backlight and removable storage/rootfs follow their actual
rail/clock/DMA requirements. Retain donor AFE/codec knowledge for M3, with radio
work deferred. Planning-only wording in old issue bodies is superseded only by
the owner's explicit implementation scope; it never authorizes device flashes.

New input subsystem qualification: one clean build, full existing offline suite,
GPIO range/refusal and ARM evdev/relay cases, linked access review, resolved
config/DT and D08/BOOTIMG checks. No memory architecture change or repeated ROM,
source or recovery provenance. Stop at the validated candidate for the owner's
physical test. Source/license attribution remains with each reused piece;
donorSource stays immutable and outside versioned implementation.

[M2-INPUT-01](../build/m2-input-01-result.md) is now offline validated: one
clean kernel build, corrected PID1/package after a caught parser defect,
29 build-suite and 12 observation tests, emitted GPIO loads/refusal review and
current D08/BOOTIMG checks. Candidate 1,204,224 bytes, SHA-256
`05c6f33d7fadb75c9bc5d8cc4a23aed1a5dcc79cf6b4fe735ce9006906290d37`.
Stop at this owner-flash boundary; no input/USB hardware classification advances.

### Current scope review — one bounded USB reconnect, baseline 7ee780a

2026-09-09. The owner explicitly authorizes completing #27 robustness before
#22 → #23 → #24 → #25 → #26 → M2 boot/stability qualification and closure audit.
Reviewed the current repository, open #16/#22–#32, #27 acceptance and #28 epic,
the existing blueprint/coverage matrix and USBACM-03 capture/source/build evidence.
The successful baseline is now pushed; its 67 retained implementation/test/tool
files match the build snapshot. No repeated source/ROM/recovery provenance audit.

Every coverage row below is re-evaluated and retains its classification: narrow
kernel/PID1 logging CONFIRMED inside overall USB PARTIAL; boot, RAM, display,
power, recovery and later product gaps remain with their existing issue owners.
M1 stays complete; M2 active with blocked exit; M3 and application work deferred.
No optional exit criterion or D11 experimental limitation becomes a production
waiver. The existing backup/calibration and full DMA ownership gaps remain open.

USBACM-03 stops permanently at first detach, so it cannot satisfy the requested
reconnect criterion unchanged. Develop USBACM-04 using the already-owned PIO
controller: on first sampled CHRDET loss remove SOFTCONN, notify upstream gadget
disconnect and restore only the existing saved digital PHY session inputs;
retain the controller and PID1 history while unplugged. Permit one restart only
after fresh supply/clock, passive digital PHY, zero DMA controls and unchanged
mode/trim checks, using the same guarded session writes and upstream controller
start. A second detach, any failed guard or the original 50-second deadline is
terminal. PID1 heartbeat and the 60-second owner limit remain unchanged.

This is a localized lifecycle extension of the proven subsystem, not new USB,
DMA, memory, boot packaging, rail or production ownership. Targeted transition,
relay/ARM PID1, host capture, linked and D08/BOOTIMG checks plus one clean build
apply. Qualification still needs the physical combined delayed-open/no-reader/
disconnect/offline-heartbeat/reconnect/replay test; do not close #27 from fixtures.
The real host sees Y2 at 5-2, ttyACM0, 0525:a4a7 with the USBACM-03 manufacturer;
this session lacks tty permissions, independently of sandbox visibility.

[USBACM-04 is now ready for the owner test](../build/m2-usbacm-04-result.md).
One clean build, 16 targeted methods and eight host observation/orchestration
methods pass; 34 ARM PID1, eight worker and 29 lifecycle/guard scenarios are
included. Current D08/BOOTIMG and focused emitted-ARM checks pass. Candidate:
1,161,216 bytes, SHA-256
`ecdce008c34998dce03e1b5dd88b0c78c3015e5865b93d4ba609f6cbf134f365`.
No physical robustness run occurred and no capability classification is promoted.
Stop at this BOOTIMG for the owner flash/cable test; #27 remains open.

### Previous checkpoint — first kernel/PID1 USB capture succeeds

2026-09-09, USBACM-03 retained candidate/source identity. The owner-run
[capture-03](../knowledge/m2-usbacm-hardware-result.md#usbacm-03-kernel-and-pid1-capture-confirmed)
contains 11754 verified bytes over a 45-second reader window: exact LOG1 build
header, contiguous kernel sequence 0–77, every PID1 beat 1–43, US:6 RC:0 from
beat 4 and increasing IRQs. No relay GAP/ERR or PID1 error appears. The normal
reader timeout exits 0. The prior host-permission blocker is resolved for this
run; this is the first captured log result, not just enumeration.

Update the USB coverage row to PARTIAL: narrow kernel/PID1 logging is confirmed,
while repeatability, late-open/non-reading host, detach/reconnect and stability
remain unqualified under #27/#28. All other coverage classifications and owners
remain unchanged; no boot/memory/power or M2 exit criterion is promoted by this
capture. M1 complete, M2 active/exit blocked, M3/application deferred. This is
an evidence review within the existing experiment: no milestone closure,
new hardware scope, build or flash. Repeat the standing full audit before
activating a different scope or closing the milestone/qualification boundary.

### Previous checkpoint — USBACM-03 enumerates; host capture access blocked

2026-09-09. [Host journal](../knowledge/m2-usbacm-hardware-result.md#usbacm-03-host-enumeration-confirmed)
and owner report confirm high-speed 0525:a4a7, the USBACM-03 kernel identity
and CDC ACM ttyACM0. The former capture timeout came from the assistant's
isolated `/dev`; an unsandboxed run matches the device but gets EACCES because
the host account lacks tty permission. Neither error disproves enumeration.
No LOG1 payload was captured. The next step is the same image and script with
owner-authorized host access, not a new hardware change or build.

Narrow enumeration is now evidenced. Overall host logging remains BLOCKED on
reader access and log-content proof; other coverage statuses and owners remain
unchanged. No milestone activation/closure, memory/production scope change or
reconnect/boot-repeatability qualification is inferred. M1 complete, M2 active
with blocked exit, #23/#27/#28 open, M3/application deferred.

### Previous checkpoint — USBACM-02 sync-only refusal

2026-09-09, `c066d62` plus the retained USBACM-02 source snapshot. The owner
reports the attach prompt now appears, followed by an error **before attachment**.
The [new photo](../knowledge/m2-usbacm-hardware-result.md#usbacm-02-result)
shows poll 5, US:2 RC:-16, WACS2 `00200001` at entry/failure, valid mask zero,
and passing wrapper gates. Init is set, request is clear and FSM is idle;
only sync-idle is absent. The snapshot does not establish how long this lasts.
No controller registration or enumeration occurred. Wake/clock evidence remains
confirmed within the prior narrow scope; USB BLOCKED, boot/memory/power PARTIAL,
all other coverage classifications and issue owners unchanged. M1 stays complete,
M2 active with blocked exit, M3/application work deferred.

The next localized fix is a bounded, read-only pre-command wait for **sync only**
in M2-USBACM-03. Retained MT6582 definitions identify bit 20 separately from
channel FSM/request; vendor `wait_for_sync`/`wait_for_idle_and_sync` explicitly
poll this condition. Require init/no-request/FSM-idle on every sample, retain
the full existing ready predicate before any command, and stop immediately on
stale/active/invalid channel states. Limit each wait to 1000 ten-microsecond
delays; no transaction retry, stale acknowledgement, wrapper reset, new register,
PMIC write, memory map or packaging scope. This repairs single-sample rejection
within the active PWRAP/USB contract; it does not authorize channel recovery or
promote hardware readiness. Targeted protocol/fault, worker/ARM PID1, linked
access and one clean BOOTIMG/D08 validation apply before the owner test.

[M2-USBACM-03](../build/m2-usbacm-03-result.md) now passes one clean build,
15 targeted methods plus four host-capture methods and linked/D08/package
review. The new protocol cases cover sync settling and failure at all four
pre-command gates. Image: 1,159,168 bytes, SHA-256
`20c7f01ad97832def704523897fb5ec08cbfacd560558bbd3989b4f4426f734a`.
Hardware sync recovery/enumeration remains untested; no capability is promoted.

### Previous checkpoint — USBACM cable-wait poll refusal

2026-09-09, baseline `c066d62`. The owner reports unplugged startup and supplies
[one M2-USBACM-01 photo](../knowledge/m2-usbacm-hardware-result.md): cached
power/clock and PHY wake succeed, US:2 RC:-16 with invalid live CHRDET stops the
worker before controller registration. BEAT 12 and MemTotal 22096 kB are visible;
there is no enumeration/logging success or identified repeatability result.
The raw failed WACS2 sample was discarded, so the particular idle-gate cause
remains unknown. The earlier CHRDET/wake evidence is retained.

The localized next candidate, M2-USBACM-02, preserves that failed sample and
corrects the stale screen heading. No hardware access, guard, memory map,
packaging or production scope changes; targeted validation applies. This is
an evidence-display repair within the active experiment, not a milestone
boundary or authority to recover a busy/stale channel. Coverage classifications
and owners remain unchanged: USB BLOCKED, boot/memory/power PARTIAL, M1 complete,
M2 exit blocked, M3/application and other input/display/media/thermal gaps deferred
to their existing gates. Stop at the validated candidate for owner testing.

[M2-USBACM-02](../build/m2-usbacm-02-result.md) now passes one clean build,
15 targeted methods and four host-capture methods, including 33 ARM PID1 paths
and real-artifact/D08 checks. Candidate is 1,159,168 bytes, SHA-256
`4ff741513ef842fca28e6fd8cd08829c9c8c9d2e3e160da70552e06166460e20`.
It awaits the failed-poll screen or bounded enumeration evidence; no busy-state
recovery or new hardware success is claimed.

### Combined USB ownership scope review — baseline d9c2c98

2026-09-09. The owner explicitly replaces the register-by-register handoff with
one guarded candidate targeting enumeration and kernel/PID1 logs. USBGUARD-01
remains an untested fallback; there is no new physical result. Reviewed the
blueprint M2/exit and later gates, current open #16/#22–#32, this coverage matrix,
the CHRDET and PHY-wake reports and current implementation. All capability
classifications remain unchanged: M1 complete; M2 active, exit blocked; host USB
logging blocked pending implementation and hardware proof. Boot/memory/power
remain partial; input, standard display, media/rootfs, thermal/battery and all
later-phase gaps retain their existing owners. No milestone criterion waived.

Authorize development of one experimental vertical slice under #23/#27:
source-reviewed MT6582 controller/PHY ownership, peripheral-only PIO MUSB,
one standard g_serial ACM function and bounded kernel/PID1 log relay. Use the
observed passive handoff as a runtime gate, initially booting without USB and
requesting attachment only after successful preflight. The unobserved complete
connected-start save-current state is not normalized by guessing. Refuse unknown
clock/supply/PHY/DMA states and preserve the screen at every stage. Resolve each
write/IRQ/FIFO contract from retained FM/LK and pinned Linux/vendor source before
implementing it; combine resolved stages within the candidate, not hardware claims.

This is first controller/IRQ/FIFO ownership and requires broader qualification:
production guard/fault tests, actual ARM relay/syscall tests, full existing
offline suite, linked access review, resolved config/DT and D08/BOOTIMG checks.
One clean candidate build, no duplicate reproducibility or unchanged source/ROM/
recovery audit. Keep D08/CPU0, watchdog stop, framebuffer fallback and original
loader/partition/calibration boundaries. No USB DMA, host VBUS supply, analog
calibration rewrite, M3/audio or application work. Stop at validated BOOTIMG for
owner physical test; successful enumeration cannot be promised offline.

[M2-USBACM-01](../build/m2-usbacm-01-result.md) is now the immediate next owner
test: one clean build, full offline suite plus ARM relay/linked syscall and host
capture checks, unchanged D08 bounds and valid BOOTIMG. It attempts enumeration
and logging in one unplugged-start/late-attach boot. Implementation has advanced;
USB hardware status remains BLOCKED pending that evidence. No milestone or
capability is promoted from the build. First-detach/reconnect qualification and
connected-start recovery remain outstanding; the original #27 criteria stand.

### Previous checkpoint — CHRDET confirmed, connected-start guard refusal

Baseline `de1a6d0`, 2026-09-09. [Three owner photos](../knowledge/m2-chrdet-hardware-result.md)
show CHRDET 007B/1 with USB attached at startup and 0001/0 without it. PWRAP,
clock and initial USB snapshots succeed. The connected boot reaches BEAT 10 but
refuses wake before writing (6a=BE, W0, V00/000); no-USB photos show successful
wake and BEAT 11/46. Same-boot chronology of the no-USB pair is not explicit.
The connected screen's zero wake fields are unread, not hardware values.

Narrow PMIC charger detection advances to CONFIRMED. Broader power/boot/memory
remain PARTIAL and host USB logging BLOCKED. Current #22–#32 tracking and the
coverage matrix retain their remaining gaps: no input/display/media/rootfs,
thermal or later-phase capability advances from these photos. M1 stays complete,
M2 active with blocked exit, M3 and Y2PlayerNative deferred; no criterion waived.

The next localized step is [M2-USBGUARD-01](../knowledge/m2-usb-guard-diagnostic.md),
preserving the already-captured raw USB state on early guard refusal and marking
unread data. Hardware access/guards, 260-byte ABI, D08, DMA, loader, calibration,
recovery and packaging policies are unchanged. This is a presentation fix within
the active prerequisite, not a new subsystem or milestone boundary. Targeted
tests and one clean build with D08/BOOTIMG checks apply. Stop at the candidate
for connected-start owner photos before selecting a recovery/session write.

[M2-USBGUARD-01](../build/m2-usbguard-01-result.md) now passes one clean build,
all 15 selected methods (26 ARM PID1 and 49 PHY-wake scenarios), linked probe
comparison and D08/BOOTIMG checks. Candidate: 1,097,728 bytes, SHA-256
`e4078583ab53bd7e6388c94a357023fe8a7504ccc5a7499341cf72287ecd19b5`.
It awaits the connected-start owner test; no new hardware result is inferred.

### Previous checkpoint — PHY wake confirmed, baseline 305f8f9

The owner-confirmed [same-boot USB-connected pair](../knowledge/m2-phy-wake-hardware-result.md)
shows BEAT 10→49, IRQ 1134→5463, MemTotal 22208 kB, no reported error, the guarded
6a 04→00 write and unchanged sampled trims. The earlier conflicting pair remains
retained with unresolved chronology. Full PHY readiness/session detection is
still unproved: POWER20/DEV80 remain after release with the cable attached.

Reconsidered the coverage matrix and current open #22–#32: narrow PHY release
advances to CONFIRMED; boot qualification/memory/power remain PARTIAL, USB logging
BLOCKED, and input/display/media/rootfs/thermal and later-phase gaps unchanged.
No optional criterion waived, M1 reopened or M3 work started. The next localized
step is [one PMIC CHRDET status read](../knowledge/m2-chrdet-probe.md) through the
proven PWRAP transport, addressing only the cable/session prerequisite. It adds
no new PMIC/analog/VBUS/MAC/DMA write, changes no memory/packaging policy and uses
targeted validation. Stop at the next BOOTIMG for owner test. Existing recovery,
stock loader, partition and calibration boundaries remain unchanged.

[M2-CHRDET-01](../build/m2-chrdet-01-result.md) passes one clean build and all
18 selected methods plus linked transaction and D08/BOOTIMG checks. Candidate:
1,097,728 bytes, SHA-256
`e511fb89e45326673183f426a4bdd3b6ca708c25bf3e6ceecfc4a67478aa9660`.
It awaits owner test; no physical charger-presence result is claimed yet.

### Latest scope review — PHY suspend release, baseline 726d38d

[USB-state photos](../knowledge/m2-usb-state-hardware-result.md) confirm all 21
reads, BEAT 11→50 without errors, MemTotal 22204 kB, disconnected B-device MAC,
eight zero DMA controls, inherited IRQ enables and PHY force_suspendm=1/value=0.
Current active issues and every coverage row were reconsidered against owner/
blueprint requirements. Narrow MAC/PHY read access is CONFIRMED; the controller
contract/host logging remain BLOCKED. Memory, boot qualification and power stay
PARTIAL; input/display/media/rootfs/thermal/battery and later-phase gaps remain.
No optional exit criterion is waived; M1 stays complete and M3 deferred.

The smallest functional next step is [M2-PHYWAKE-01](../knowledge/m2-phy-wake-probe.md):
read/check remaining PHY mode controls, then conditionally clear only 6a bit2,
as corroborated by FM/LK. Require disconnected B-device and zero DMA, preserve
calibration and all analog/VBUS settings, inspect readback without retry. This
adds one bounded PHY control write, not UDC/DMA ownership or calibration policy.
Full existing offline checks plus emitted access review apply; stop at BOOTIMG
for owner test. Recovery/loader/partition boundaries and D08 are unchanged.

[M2-PHYWAKE-01](../build/m2-phywake-01-result.md) now passes one clean build,
21 top-level and four observation test methods plus linked-write/layout review.
Candidate: 1,097,728 bytes, SHA-256
`d68c8fbc56721c7aa1fda5b3e321a9aff5876661aac857505864e28d8924793a`.
It awaits owner test; no successful PHY release is claimed yet.

### Latest scope review — USB state observation, baseline 23d09db

The [returned clock-probe photographs](../knowledge/m2-usb-clock-hardware-result.md)
show no errors, BEAT 15→50, IRQ 1689→5574, MemTotal 22208 kB, successful
PWRAP/VUSB reads, USB0 ungated, USB mux selector 1 enabled and PLL USB reference
enabled with power on/isolation off. Reconsidered every coverage row below,
current open issues and owner/blueprint requirements: only narrow clock-read and
timer-reporting facts advance. Core clocks/pins/IRQ/power stay incomplete;
boot/memory remain PARTIAL, USB logging BLOCKED. Input, standard display, media,
rootfs, thermal/battery and later milestone gates are unchanged. No optional
feature is waived, no M1 reopening and no M3 work.

Next scope: a single cached read-only MAC/digital-PHY/DMA-control snapshot under
strict observed supply/clock guards. This is first access to those register
windows, requiring source/FM/LK width/side-effect review, fault tests and the
full existing offline suite plus D08/package validation. No interrupt status,
FIFO, endpoint index, analog trim/calibration, clock or controller writes; no
DMA ownership change. [Contract](../knowledge/m2-usb-state-probe.md).
Stop at the new BOOTIMG for owner test. Keep all existing recovery boundaries;
no repeated unchanged source/ROM/recovery audit is needed.

[M2-USBSTATE-01](../build/m2-usbstate-01-result.md) now passes one clean build,
20 full build-suite methods and four observation-tool methods. Candidate:
1,095,680 bytes, SHA-256
`8c1f701b61a1adb70b1377071e6cb885cda989a422fd75d626cd5c7da5bdb5e7`.
It awaits the owner test; no MAC/PHY hardware result is inferred.

### Original activation review

Baseline `ebeed72`, clean `main`; current GitHub #1–#32, open M0 and closed M1
reviewed against the local blueprint and retained hardware result. The owner now
explicitly authorizes autonomous M2 research/implementation/builds and normal
Git/GitHub operations, with a stop at each hardware-testable BOOTIMG and before
any physical-device modification. This supersedes the earlier planning-session
restrictions and queued research order; it does not establish any hardware fact.

**M2 is ACTIVE, entry PARTIAL; exit BLOCKED on unimplemented/unverified core
interfaces.** M1 stays complete. All coverage rows below were reconsidered:
their capability classifications and evidence limits remain unchanged because
no newer physical result exists. M2's phase status changes from PLANNED to ACTIVE;
#22–#27 remain open, and #27's controller gate is still BLOCKED. The working
owner-reported version is 6.18.0-y2-m1; exact flashed SHA-256, MemTotal, repetition
and timing are not supplied. The retained time32 candidate identity is recorded
in [the M1 result](../knowledge/m1-runtime-hardware-result.md), not promoted to
an identified hardware trial.

Execution priority: (1) #27 USB contract and only #23's needed VUSB/PWRAP/clock
prerequisite; (2) one logging implementation/trial if that contract permits it;
(3) RAM adequacy and #22's bounded expansion decision; (4) #24 input; (5) #25
standard display handoff; (6) #26 read-only removable media and recoverable
stage-appropriate rootfs; (7) identified repeatability/stability and M2 exit audit.
RAM expansion, UART wiring, full SMP and full power-framework implementation are
not prerequisites for USB research. Reorder subsequent work only on evidence.
The smallest current blocker is the FM-normal-handoff USB PHY/clock/VUSB/DMA
contract; inspect those exact functions before selecting a runtime change.

D08, CPU0, stopped AP watchdog and guarded framebuffer remain the default.
Stock preloader/LK, partition tables and calibration remain protected. Existing
SPFT/FM recovery evidence is retained without another provenance/recovery audit;
M0 backup/installed-lineage and inherited-DMA uncertainties remain residual risks,
not production waivers. New subsystem ownership must be justified separately.
No M3 audio or Y2PlayerNative implementation starts before real M2 exit evidence.
No optional feature has been waived merely to reach today's target.

### Current execution checkpoint

[M2-PWRAP-01](../build/m2-pwrap-01-result.md) is offline validated, 1,093,632 bytes,
SHA-256 `c51102c861d65d39f5af2425ed73e9c1ba21d8adaae1b016f450a58630946ac0`.
The owner has now supplied [two physical photographs](../knowledge/m2-pwrap-hardware-result.md):
PWRAP RC=0/VALID=3, CID=0x2023, VUSB=0xc000, BEAT 18→50 and MemTotal 22208 kB.
The cached PWRAP/VUSB result is confirmed on this boot; exact flashed hash,
host timing and restoration remain unreported. Timer -61 is a source-identified
IRQ-name parser mismatch. #23/#27 and M2 remain open.

Scope review at `c6709cc`: the next localized candidate corrects IRQ parsing and
reads four exact USB clock handoff registers; no clock/PHY/USB controller/PMIC
write or DMA ownership change is added. [Contract](../knowledge/m2-usb-clock-probe.md).
All coverage rows were reconsidered against these photographs: boot stability,
memory and power remain PARTIAL (now with measured MemTotal and one successful
PWRAP read path); USB remains BLOCKED on its lower-level ownership contract.
Other input/display/storage/rootfs/thermal/battery gaps and later-phase gates are
unchanged. Preserve D08, CPU0, screen/watchdog and established recovery policy.

[M2-USBCLK-01](../build/m2-usbclk-01-result.md) is now offline validated and
awaits the owner test: 1,093,632 bytes, SHA-256
`602080eb199f3a4be30082ee114dfeb97ef414e10d8ad4acf5d239f2096cd6ec`.
One clean build and 19 targeted validation methods pass; no device action occurred.

### First prerequisite scope review

The focused [PWRAP/VUSB probe contract](../knowledge/m2-pwrap-probe.md) now resolves
one implementable #23 step using FM/LK and pinned upstream/vendor source. This
adds bounded AP PMIC read transactions to a candidate, with no PMIC register
writes or USB ownership. Power remains PARTIAL and USB remains BLOCKED pending
physical state evidence and the remaining PHY/clock/DMA contract. D08 and all
other coverage classifications remain unchanged. Basic battery/charger reporting
also remains an M2 exit gap from the blueprint; VUSB status cannot satisfy it.

## Blueprint assessment and authority

The [Y2Linux/Y2PlayerNative blueprint](https://github.com/SchulzCode/Y2PlayerNative/blob/main/YY2PlayerNativeBlueprint.md)
still covers the project's direction adequately: upstream 6.18, standard Linux
interfaces, native Rust/C application, native audio before central player work,
power/connectivity/product gates and rolling-wave planning. This audit reviewed
the local blueprint, the actual Y2Linux code/config/DT and retained build reports,
current issues, and the stock evidence indexed in
[evidence-index.md](../knowledge/evidence-index.md). Local blueprint snapshot SHA-256: `f9850e35ed790008cbb125001cf166848c7dd1c11c580d41299ecbe311add95e`.
No repeated provenance audit
or new driver investigation was performed. Source applicability is not hardware
success; Android behavior is a reference, not the native implementation.

Important reconciliations:

- Blueprint example task IDs are illustrative. Existing Y2E/Y2B IDs and results
  remain authoritative; do not create duplicate tasks with those sample IDs.
- The owner superseded the original role split with direct M1 implementation.
  That authorization is not a standing permission to execute a planning epic.
- The owner accepted specific first-boot risks under D11. M1 core success does
  not retroactively certify all M0 backups/lineage/DMA questions. M0 remains open;
  proven same-device SPFT recovery must not be repeatedly rediscovered.
- **M1 core is achieved**, as explicitly reported by the owner. Blueprint M1
  wording about reproducible boots/stability is broader than the retained report:
  boot identity/repetition/timing/stress evidence remains PARTIAL and is tracked
  under Y2H-300. Do not reopen the achieved core or claim release qualification.
- New recurring milestone audits and the fast-iteration validation policy below
  supplement the blueprint. They are not permission for a full build on a
  documentation-only day.

## Classification key

One primary status describes the scoped area, not every fact in it:

- **CONFIRMED:** the stated narrow result is directly evidenced; scope matters.
- **PARTIAL:** useful evidence or execution exists, but the platform capability
  or its qualification is incomplete.
- **PLANNED:** a bounded next task exists; it has not been implemented/validated.
- **UNKNOWN:** the required contract/capability is not established; an epic is a
  reminder, not enough evidence to upgrade this to a resolved plan.
- **BLOCKED:** a specific missing prerequisite prevents the stated next capability.

Hardware-fact labels in older knowledge documents (including INFERRED/DISPROVEN)
remain unchanged. An epic does not turn unknown hardware into confirmed support.

## Coverage matrix

Reassessed 2026-09-10 against the [DEV-02 live qualification](../knowledge/y2linux-dev02-live-qualification.md); every row reviewed, including untouched later-phase gaps.
Status measures our hardware; registration alone never establishes consumer operation.

| Area | Status | Actual evidence / remaining gap | Durable tracking |
| --- | --- | --- | --- |
| Physical Linux + initramfs + native PID1 | **CONFIRMED** | DEV-02 now switches to real Buildroot BusyBox PID1 on removable ext4 Y2ROOT; rescue and earlier M1 results remain historical. | Closed M1 / #20–21; core stays achieved. |
| Boot/kernel stability and maintenance | **PARTIAL** | DEV-02 live work, bounded RAM and initial SSH operate. First USB reconnect fails before enumeration; post-detach execution/uptime unavailable. Repeated cold/warm boots, long-run stress and maintenance remain later qualification. | Y2H-300 #28; maintenance/release policy #32. |
| RAM and reserved/DMA ownership | **PARTIAL** | 992 MiB bank with exclusions; physical MemTotal954660KiB and HighTotal228352KiB. Completed 256MiB short allocator test passes, substantial HIGHMEM consumed. Full-suite run capped600s; every-page/long-run/inherited-DMA containment is not proved. Narrow expansion #22 satisfied. | #22; #28. Owner-authorized development qualification. |
| SMP | **CONFIRMED** | Four CPUs online on DEV-02 with advancing cross-CPU/timer IPIs and no interrupt errors; full coherency/PM stress is a later qualification scope. | #28; optional for first wired player. |
| Clocks, resets, pinctrl/GPIO/IRQ and I2C | **PARTIAL** | DEV-02 CCF/PWRAP/MFD/regulator/core consumers operate; wheel has52IRQ/52I2C completions and both directions, no timeout. Exact full clock rates/unused orphan gates and future consumer reset/rail ownership remain #29/#30 prerequisites. | #23/#24/#28; [research](../knowledge/reverse-engineering-audit.md). |
| Watchdog and controlled reset | **PARTIAL** | Reviewed early AP_RGU stop and displayed stopped state; no production driver takeover, pet/timeout strategy, deliberate reset or restart qualification. | #28 with #30. Preserve current experiment behavior. |
| On-device diagnostic channel | **CONFIRMED** | DEV-02 native fbcon and owner-confirmed visible color/checkerboard pattern; supported unblank succeeds. Cursor handling fixes a later shared-buffer checksum mismatch without a kernel change. | #20–21; [result](../knowledge/m1-runtime-hardware-result.md). |
| Developer host logs / USB | **PARTIAL** | DEV-02 ACM enumeration, complete logs over authenticated SSH and ECM traffic work. First physical reconnect fails before enumeration; no boot-continuity result. Host static-address/uaccess fixes verified after an owner restart; ACM transfers61463bytes/12s. This is restart recovery, not reconnect success. | #27, #23, #28. |
| Physical UART / early crash capture | **UNKNOWN** | UART0 candidate exists; no pad/level/wire capture. Ramoops retention/reader is unproved. USB cannot log hangs before its initialization. | Existing #16; broader crash/debug policy #32. |
| Display/controller/panel | **CONFIRMED** | DEV-02 DSI-1 connected, CRTC55 active, framebuffer56 XR24 480x360/pitch1920, fbcon bound, safe GEM/OVL address match and owner-confirmed pattern. Blank state corrected through sysfs; broader modes/power sequencing remain later. | #25/#28; [research](../knowledge/reverse-engineering-audit.md). |
| Backlight | **PARTIAL** | DEV-02 brightness/actual/max32 with physically visible panel; native brightness range, PWM/rail transitions and suspend sequencing remain #30. | #25/#23; #28/#30. |
| Wheel/select input | **CONFIRMED** | DEV-02 raw evdev has balanced Select and both wheel KEY_UP/KEY_DOWN directions;52wheelIRQ/52I2C completions. Supersedes DEV-01 -110 transport regression. | #24/#28; [research](../knowledge/reverse-engineering-audit.md). |
| Other buttons/power-key/touch/wake | **PARTIAL** | All five navigation keys, both volume keys and brief Power have balanced DEV-02 evdev events. No redesign; wake/suspend remains M4 and no touchscreen claim. | #24/#28 and #30; [donor audit](../knowledge/donor-audit.md). |
| Removable SD | **CONFIRMED** | DEV-02 SD128 on11240000.mmc at13MHz/one bit; ext4 Y2ROOT boots Buildroot, reads files, accepts verified userspace updates and sync.512MiB filesystem retained; throughput/hotplug/card-removal/power-fail qualification later. | #26/#28; [research](../knowledge/reverse-engineering-audit.md). |
| Internal eMMC | **CONFIRMED** | DEV-02 live DT disables11230000.mmc, no native eMMC block/partition node or mount. Removable-only root resolver and no automounter exclude Android partitions. Identity remains historical because disabled host exposes no live CID. | #28; recovery/rootfs policy #32. |
| Development rootfs and filesystem/data layout | **CONFIRMED** | Physical Buildroot2025.02.17/glibc/BusyBox/Dropbear on writable removable Y2ROOT, key authentication and complete command logs. Runtime mount and matching-module index fixes applied. Owner restart validates persistent runtime/module fixes; production layout separate. | Added #28/#32; owner selects Buildroot/glibc, removable ext4 Y2ROOT and rescue fallback. |
| PMIC/battery/charger telemetry | **PARTIAL** | DEV-02 PWRAP/MFD/regulator/PMIC key consumers work and USB presence power_supply ONLINE=1 is readable. This does not provide battery units/calibration, charging policy or voltage/thermal telemetry; full power now belongs to M4 #30. | #23; added #30. |
| Thermal sensors/protection | **UNKNOWN** | Sensor mapping, calibration, trips/cooling and safe operating limits unverified. Required before sustained workload qualification. | #30; basic reporting dependency for #28. |
| cpufreq/voltage/OPP and cpuidle | **UNKNOWN** | Minimal config does not establish frequency/voltage transitions or idle states. Shared clocks/rails precede optimization. | #30 with #28. |
| Runtime PM/suspend/resume/wake | **UNKNOWN** | No native suspend/wake, rail retention, storage resume or screen-off-audio proof. | #30; integrates #28/#29/#31 incrementally. |
| MT6582 AFE/ASoC/I2S/DMA | **PARTIAL (card/codec and audible path; PCM defect)** | Our HAL evidence and external pad/audio results support second-I2S/DL1; CON1 silence and VGP2 brownout have failure explanations. M3-AUDIO-01 probes ALSA/CS43131, advances CON3/DMA/IRQ and produces audible headphones with clicking. A startup success-handling defect prevents ALSA period notification; corrected kernel acceptance is pending. [Audio](../knowledge/audio-path.md). | #29; shared resources #23/#28/#30. |
| CS43131 / headphone / amplifier / speaker routing | **PARTIAL** | Historical cs43131_dac at 1-0030 and aw87559_pa at 1-0058; stock /proc/asound/cards empty. Binding names are not native PCM, electrical routing or upstream applicability proof. Reset/mute/jack/ACCDET/analog levels remain open. | #29 with #23/#30. |
| Wi-Fi | **PARTIAL** | Stock firmware/module names present, not loaded-blob or native-driver evidence. Transport/revision/calibration/regulatory/power contract missing. | Added #31. |
| Bluetooth | **PARTIAL** | Local WMT/STP metadata; external BTIF/HCI, cold calibration and A2DP records reduce research uncertainty. Own firmware/calibration and cold/link/rate qualification remain open; EDR fallback is a workaround. | #31; audio #29 and power #30. |
| Firmware and per-device calibration | **PARTIAL** | Names and some hashed artifacts exist; exact selected radio blobs/order/compatibility, loading permissions, redistribution provenance and unique-data retention unresolved. | #31 and #32; private raw data stays private. |
| FM where physically supported | **PARTIAL** | Donor STP/FM/AFE source exists; owner confirms this older Y2 lacks usable reception hardware. No reception implementation/qualification target for this board; other revisions remain untested. | Conditional #31/M16 reference only; [donor audit](../knowledge/donor-audit.md). |
| Recovery and safe acquisition | **PARTIAL** | Owner-proven SPFT/FM history plus documented BOOTIMG-only Android restoration. Exact current image/per-device backups, consistency and independent retention remain incomplete. [Recovery](../knowledge/recovery.md), [actual trial](../knowledge/first-experiment-result.md). | Open M0; #32 owns continuing production/recovery coverage. |
| Updates / rollback / production security | **PLANNED** | Blueprint names signatures/rollback; no reviewed storage/key/threat/interrupt-safe updater design. Unchanged LK does not automatically verify custom payloads. | Added #32; M18–20 platform handoff, not implementation now. |
| Production rootfs / services / non-root app contract | **PLANNED** | DEV-01 has a development BusyBox-init/glibc/Dropbear stack. Production services, non-root app boundaries and lifecycle policy remain unimplemented; no native app starts. | #32 (M6); app implementation belongs to native repo M7+. |
| Time/RTC, entropy, identity and diagnostic privacy | **PARTIAL** | DEV-02 has no RTC class, starts at1970, and initializes CRNG at153.97s before ED25519 host-key generation. Key persists on SD. Entropy sysctl/boot_id files absent; no insecure entropy-credit workaround. Time/privacy policy remains #31/#32. | #31/#32; added explicit cross-cutting coverage. |

## Gaps found and minimum additions

The blueprint already names the major phases; the repository had only M0/M1
GitHub milestones and narrow #22–#27 follow-ups. Audio, product PM, connectivity
and production lifecycle had no durable epic owner. Common clock/pinctrl/I2C,
watchdog/reset, eMMC/rootfs, firmware selection/redistribution/calibration and
conditional FM could otherwise fall between the narrow issues.

Five **deferred planning epics** now retain these gaps. No detailed children or
new GitHub milestone shells are needed today:

| Blueprint phase | Tracking epic | Current phase status / dependency |
| --- | --- | --- |
| M0 Evidence & Recovery | Existing open milestone 1 | **PARTIAL**; owner-accepted experimental exceptions are documented, not a full pass. |
| M1 First Boot | Existing closed milestone 2 | **CONFIRMED core**; broader stability qualification moves into #28. |
| M2 Core Hardware | [Y2H-300 #28](https://github.com/SchulzCode/Y2Linux/issues/28) | **Core/Buildroot qualified for M3 progression**; #22–26 satisfied. USB reconnect #27 stays open under #28 for later whole-platform qualification. |
| M3 Native Audio | [Y2A-300 #29](https://github.com/SchulzCode/Y2Linux/issues/29) | **ACTIVE by explicit owner authorization**; headphone ALSA candidate produced audible sound with clicking. PCM notification fix needs physical retest; clean playback and rates remain unqualified; #27 is deferred. |
| M4 Power Viability | [Y2P-400 #30](https://github.com/SchulzCode/Y2Linux/issues/30) | **PLANNED**; basic safety begins alongside M2, full suspend/playback integrates later. |
| M5 Connectivity | [Y2N-500 #31](https://github.com/SchulzCode/Y2Linux/issues/31) | **PLANNED**; radio transport/firmware dependencies, conditional FM foundation. |
| M6 Production Y2Linux | [Y2R-600 #32](https://github.com/SchulzCode/Y2Linux/issues/32) | **PLANNED**; storage/recovery/PM gates and later M18–20 platform release obligations. |
| M7–M20 application/product phases | Existing blueprint, no native backlog added | **PLANNED** at blueprint level only. Native implementation is gated; do not manufacture detailed tasks now. |

M4 numbering is not permission to defer basic thermal/power safety until after
audio. M5 radio-specific power measurements depend on radio operation, so these
phases have shared gates rather than a rigid all-or-nothing serial chain. M6
rootfs contracts can mature with M2 without prematurely choosing production OTA.
The original next-five order is retained as history. The activation decision above
prioritizes USB logging #27 and its smallest shared prerequisite; it does not
remove the controller evidence gate.

## When Y2PlayerNative can become the serious workstream

The owner's 2026-09-10 platform-first instruction supersedes the earlier
wired-player-only gate. First qualify the reusable general-purpose Y2 platform:
M2 core/Buildroot including reliable development reconnect; M3 native ALSA/ASoC,
MT6582 AFE, CS43131, amplifier, headphone/speaker routing and jack detection;
M4 battery/charging/thermal, cpufreq/cpuidle, suspend/resume/wake and controlled
shutdown/reboot; M5 Wi-Fi/Bluetooth with own firmware/calibration, BlueZ,
reconnect/coexistence. Then complete GPU/lima and final whole-system qualification.
Only after that does Y2PlayerNative implementation begin. #29–32 retain these
gates; no application or later hardware phase starts in this live audit.

## Standing milestone-boundary rule

**At every major milestone boundary, repeat this gap audit and update the roadmap
from real hardware evidence before proceeding.** Apply it before phase activation,
exit/closure or a material change in hardware/memory/production scope. Root
`AGENTS.md` links this rule so it remains visible in future sessions.

The boundary review must:

1. Compare current blueprint/owner decisions, repository changes, all active
   issues/epics, retained validation and actual hardware results. Record date,
   revision, tested artifact identity when known, evidence origin and limitations.
2. Refresh every coverage row using this classification vocabulary. Separate
   working narrow behavior, untested driver availability and planned work. Preserve
   older failures/history with explicit superseding notes; do not invent missing
   timings, hashes, firmware selection or hardware success.
3. Reconcile dependency/phase gates, accepted risks, unresolved backup/calibration
   requirements and the next smallest blocker. An experiment's accepted risk is
   not a permanent production waiver. Honor all later explicit owner decisions.
4. Link each material gap to an existing issue/epic; add only a missing high-level
   bookmark when necessary. Decompose only the next evidence boundary. Record
   whether the next phase is ready, partially ready or blocked and why.
5. Update this audit, relevant knowledge pages, roadmap and GitHub milestone/epic
   status before proceeding. Hardware authorization remains separate.

An audit is a planning/evidence review, not an automatic test run. Future small
localized code changes use one clean build, affected tests, D08/layout/BOOTIMG
checks and final size/hash. Architecture/memory/packaging changes, new hardware
subsystems, milestone qualification and release candidates use the appropriate
full verification boundary. Do not rehash the unchanged entire source tree or
redo ROM/SPFT provenance without relevant cause. A documentation-only request
never triggers a build or hardware operation to fill a missing audit datum.
