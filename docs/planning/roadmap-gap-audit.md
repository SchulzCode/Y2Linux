# Y2Linux roadmap and gap audit

Original audit date: **2026-09-09**. Documentation/planning only. No implementation, build,
BOOTIMG creation, device access or flash. Repository baseline `00b4afa`; GitHub
issues #1–#27 and M0/M1 inspected before adding five deferred epics #28–#32.
Y2PlayerNative has no open application issues and its checked-in content remains
blueprint/planning material; no native platform readiness is inferred from that.

## M2 activation audit — 2026-09-09

### Current checkpoint — first kernel/PID1 USB capture succeeds

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

| Area | Status | Actual evidence / remaining gap | Durable tracking |
| --- | --- | --- | --- |
| Physical Linux + initramfs + native PID1 | **CONFIRMED** | Owner reports 6.18.0-y2-m1, PID1, mounts, increasing BEAT/uptime and working sleep. [Result](../knowledge/m1-runtime-hardware-result.md). | Closed M1 / #20–21; core stays achieved. |
| Boot/kernel stability and maintenance | **PARTIAL** | Reproducible offline artifacts and a working run exist; exact flashed hash for latest report, repeated cold/warm boots, duration, timing accuracy, stress and maintained 6.18.y selection are not qualified. | Y2H-300 #28; maintenance/release policy #32. |
| RAM and reserved/DMA ownership | **PARTIAL** | D08 24.5 MiB described map is visible; photographed MemTotal 22208 kB; working-set adequacy, expanded banks, heap/high carveouts and full DMA containment remain open. [Map](../knowledge/initial-ram-map.md). | #22; #28. No automatic expansion. |
| SMP | **UNKNOWN** | CPU0 works; physical extra cores do not establish release/coherency/interrupt/PM semantics. [Audit](../knowledge/linux-6.18-support.md). | #28; optional for first wired player. |
| Clocks, resets, pinctrl/GPIO/IRQ and I2C | **BLOCKED** | Minimal fixed timer/UART clocks work sufficiently for M1. Common peripheral providers/mux/reset/rail contracts are not established; USB/storage/audio cannot simply inherit sibling compatibles. | #23 and #28, now explicit shared foundation. |
| Watchdog and controlled reset | **PARTIAL** | Reviewed early AP_RGU stop and displayed stopped state; no production driver takeover, pet/timeout strategy, deliberate reset or restart qualification. | #28 with #30. Preserve current experiment behavior. |
| On-device diagnostic channel | **CONFIRMED** | Working inherited RGB565 text and owner-observed heartbeat; exact other fields/duration are not invented. | #20–21; [result](../knowledge/m1-runtime-hardware-result.md). |
| Developer host logs / USB | **PARTIAL** | USBACM-03 physical ACM capture confirms retained kernel sequence 0–77 and PID1 beats 1–43 with no captured relay gaps/errors. [Result](../knowledge/m2-usbacm-hardware-result.md#usbacm-03-kernel-and-pid1-capture-confirmed). Repeatability, late-open/non-reading host, detach/reconnect and stability remain unqualified. | #27, #23, #28. |
| Physical UART / early crash capture | **UNKNOWN** | UART0 candidate exists; no pad/level/wire capture. Ramoops retention/reader is unproved. USB cannot log hangs before its initialization. | Existing #16; broader crash/debug policy #32. |
| Display/controller/panel | **PARTIAL** | Inherited 480×360 RGB565 works; panel candidate and LK evidence exist. No native DRM/panel initialization, standard-interface takeover or display-PM proof. | #25; #28. |
| Backlight | **PLANNED** | Existing illumination persists; independent brightness/control/limits and rail/PWM ownership unverified. Narrow ownership research is queued. | #25/#23; #28/#30. |
| Wheel/select input | **PLANNED** | Stock input nodes/key behavior exist; GPIO/MCU/transport and evdev mapping require the scoped proof. No native input driver. | #24; #28. |
| Other buttons/power-key/touch/wake | **UNKNOWN** | Stock touch-named nodes do not prove physical touchscreen capability. Full key coverage, long-press and wake routes not established. | #28 and #30; no fabricated touchscreen requirement. |
| Removable SD | **PLANNED** | Metadata distinguishes card from internal eMMC; #26 scopes controller/read-only proof. Native driver/clock/pins/DMA contract missing. | #26; #28. |
| Internal eMMC | **BLOCKED** | Capacity/ordinary partition offsets reconciled; no native controller support contract, safe write/flush path or filesystem integrity qualification. | #28; recovery/rootfs policy #32. |
| Development rootfs and filesystem/data layout | **PLANNED** | Current image is diagnostic initramfs only. Block access, filesystem, writable-data boundaries and recoverable rootfs deployment were absent beyond broad blueprint intent. | Added #28/#32; no distribution/layout chosen. |
| PMIC/battery/charger telemetry | **PARTIAL** | Photographed native PWRAP CID 0x2023/VUSB 0xc000 reads work; CHRDET follows reported startup cable state ([result](../knowledge/m2-chrdet-hardware-result.md)). Duplicate Android battery fields still conflict. No trustworthy units/calibration, native gauge/charger policy or voltage measurement. | #23; added #30. |
| Thermal sensors/protection | **UNKNOWN** | Sensor mapping, calibration, trips/cooling and safe operating limits unverified. Required before sustained workload qualification. | #30; basic reporting dependency for #28. |
| cpufreq/voltage/OPP and cpuidle | **UNKNOWN** | Minimal config does not establish frequency/voltage transitions or idle states. Shared clocks/rails precede optimization. | #30 with #28. |
| Runtime PM/suspend/resume/wake | **UNKNOWN** | No native suspend/wake, rail retention, storage resume or screen-off-audio proof. | #30; integrates #28/#29/#31 incrementally. |
| MT6582 AFE/ASoC/I2S/DMA | **BLOCKED** | Later stock references suggest second-I2S/DL1. No established native CPU-DAI/DMA/clock/machine route. [Audio](../knowledge/audio-path.md). | Added #29; shared resource contracts #28/#30. |
| CS43131 / headphone / amplifier / speaker routing | **PARTIAL** | Historical cs43131_dac at 1-0030 and aw87559_pa at 1-0058; stock /proc/asound/cards empty. Binding names are not native PCM, electrical routing or upstream applicability proof. Reset/mute/jack/ACCDET/analog levels remain open. | #29 with #23/#30. |
| Wi-Fi | **PARTIAL** | Stock firmware/module names present, not loaded-blob or native-driver evidence. Transport/revision/calibration/regulatory/power contract missing. | Added #31. |
| Bluetooth | **PARTIAL** | WMT/STP and ttyMT2 clues; no standard HCI transport/firmware initialization or native BlueZ/A2DP proof. | #31; audio #29 and power #30. |
| Firmware and per-device calibration | **PARTIAL** | Names and some hashed artifacts exist; exact selected radio blobs/order/compatibility, loading permissions, redistribution provenance and unique-data retention unresolved. | #31 and #32; private raw data stays private. |
| FM where physically supported | **UNKNOWN** | Stock chip-ID/tune leads do not establish useful RF reception, antenna population or this board revision's native route. | Explicit conditional branch #31 feeding blueprint M16, not a universal promise. |
| Recovery and safe acquisition | **PARTIAL** | Owner-proven SPFT/FM history plus documented BOOTIMG-only Android restoration. Exact current image/per-device backups, consistency and independent retention remain incomplete. [Recovery](../knowledge/recovery.md), [actual trial](../knowledge/first-experiment-result.md). | Open M0; #32 owns continuing production/recovery coverage. |
| Updates / rollback / production security | **PLANNED** | Blueprint names signatures/rollback; no reviewed storage/key/threat/interrupt-safe updater design. Unchanged LK does not automatically verify custom payloads. | Added #32; M18–20 platform handoff, not implementation now. |
| Production rootfs / services / non-root app contract | **PLANNED** | No rootfs/udev/service stack yet. ARMv7 ABI, filesystem permissions, startup/restart policy, footprint and privileged-service boundary need design. | #32 (M6); app implementation belongs to native repo M7+. |
| Time/RTC, entropy, identity and diagnostic privacy | **UNKNOWN** | Wall-clock persistence/source, entropy readiness and secret-redacted export policy not yet qualified. No physical RTC assumed. Relevant to TLS, updates and later history. | #31/#32; added explicit cross-cutting coverage. |

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
| M2 Core Hardware | [Y2H-300 #28](https://github.com/SchulzCode/Y2Linux/issues/28) | **ACTIVE, entry PARTIAL**, under the activation decision above; owns #22–#27 dependencies. |
| M3 Native Audio | [Y2A-300 #29](https://github.com/SchulzCode/Y2Linux/issues/29) | **BLOCKED** on needed core resources; major Gate D before central player/audio work. |
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

The blueprint's Gate C and especially Gate D remain the immediate platform
threshold. Require an evidenced vertical slice:

1. Repeatable, identified Linux boot with sufficient measured memory, recoverable
   artifacts and usable logs; real timer/syscall behavior and basic thermal/power
   safety, not only one screen report.
2. Stable Linux input and a supported display interface (reviewed framebuffer
   fallback is acceptable initially), readable media storage and a recoverable
   userspace layout with writable app data/permissions defined.
3. Real ALSA/ASoC PCM playback through the intended Y2 headphone path, with known
   clock/rate/format/volume/routing behavior and repeated playback evidence.
4. An ARMv7 Rust/C build/runtime and platform API contract using Linux interfaces,
   with boot supervision and non-root application access scoped. This is later
   design work, not proof supplied by the kernel-only toolchain.

Host-only application models/tests could precede that slice, but should not
become the central device workstream now. Full SMP, maximal RAM, Wi-Fi, Bluetooth,
FM, advanced suspend and OTA are **not all prerequisites for initial wired-player
work**. They remain necessary where applicable for later product promises and
Android retirement under Gates E–I; no feature parity or production readiness is
claimed by M1 completion.

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
