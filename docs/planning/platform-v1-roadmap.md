# Platform v1 software completion roadmap

## Baseline02 integrated candidate admission — 2026-10-03

Owner requests a fresh integrated baseline after UART01 physical qualification.
Read-only authorized SSH reconfirms installed Linuxe9e8d63/Rebornb71b4688,
kernel `6.18.0-y2linux-cpu-c3-uart-01`, rootv1.20, unchanged boot
`fd955840-35d9-47db-83e0-ff47d6bb2d2b`, taint0. C3 still has3647 real
entries/resumes/successes/UART ACKs, zero timeout/context/clock/CIRQ failures.
The completed foundation and21 bounded C3 trials plus60s runtime observation
remain authoritative; see [UART01 physical evidence](../validation/Y2-CPU-C3-UART-PHYSICAL.md).
This is a packaging boundary, not a general CPU audit or new hardware phase.

Prepare one fresh BOOTIMG/Y2ROOT baseline from current committed Linux and
Rebornb71b4688, retaining all integrated storage/USB/audio/power/radio/product
work and current UART C3 architecture. Fresh kernel, Buildroot/Reborn ARM,
production regressions, QEMU/ELF/config/DT/module/ABI and data-preserving package
checks are required. Preserve Y2DATA, protected partitions, guarded five OPPs,
C1/C2 and default-off C3 policy; include latest guarded qualification harness.
New image physical qualification remains NOT_RUN until owner installation.
Fallback is the unchanged physically qualified UART01 BOOTIMG/Y2ROOT pair.

#28/#31/#34 remain OPEN: persistent C3 boot policy, full-system suspend,
additional wake sources, endurance and electrical battery evidence are distinct
unclosed coverage. No memory/layout/data-schema change, flash, push or public
redistribution authorization. Six preexisting owner documentation edits remain
separate. Boundary receipts: `out/baseline-02-validation/`;
[new baseline record](../validation/Y2-BASELINE-02.md).

## UART01 C3 hardware closing audit — 2026-10-03

**C1 WORKING; C2 WORKING; C3 WORKING_AND_REPEATEDLY_OBSERVED** on installed
Linuxe9e8d63/Rebornb71b4688, kernel `6.18.0-y2linux-cpu-c3-uart-01`, rootv1.20,
unchanged boot `fd955840-35d9-47db-83e0-ff47d6bb2d2b`, taint0. Fresh physical
foundation checks pass: four-core C1,18 normal hotplug transitions, all five
OPPs, GPT6/GPT4/PPI29/13MHz/highres/NO_HZ, natural owned parking, C2+6971
entries/+50.654676s in60.054881s (84.35%), zero clock restore failures.
C3 proves21 budget1/RGU/SRAM checked reset returns, then+3624 normal-policy
entries/+39.204662s in60.088344s (65.25%). Final entries=resumes=successes=
UART attempts=ACKs=CIRQ transactions/flushes=timer saves/restores=3647.
No UART timeout, restore/context/clock/media/IRQ fault or reboot. eMMC/SD,
five OPPs, screen/workload/playback, USB and Wi-Fi postwake regressions pass.

UART1 remains clocked: conditional exact MT6582 PIO ownership and global
SPM request/ACK resolve admission without forced gating. No kernel/image
changes after installation; locally committed c8ae170 fixes observer fsync
sequencing and accounts safe guard refusals separately from actual entries.
33 targeted tests pass. Original failed observer verdicts are retained and
independently rechecked against stronger UART/SRAM/context/media guards.
Authoritative joined proof: `out/cpu-c3-uart-physical/20261003T183259Z-uart01/final-verdict.json`;
[hardware record](../validation/Y2-CPU-C3-UART-PHYSICAL.md).

F027/F028/F030 may advance to this bounded same-boot physical qualification.
The UART C3 blocker is closed. No new flash is required. C3 is intentionally
runtime-enabled/budget-1 for THIS BOOT, RGU disarmed and original radios,
screen, schedutil598–1300MHz and coordinator restored. Fresh boot still uses
image default-off/budget0; no persistent default-on policy is claimed or
changed. Full system suspend, new wake-source coverage, endurance, electrical
battery measurement and other #28/#31/#34 dependencies remain OPEN. No general
milestone closure, memory/partition/protected-data change, flash or push.
Six preexisting owner documentation edits remain preserved separately.

## UART01 physical trial admission — 2026-10-03

Owner confirms the new candidate installed and authorizes the prepared hardware
qualification. Read-only SSH verifies Linuxe9e8d63/Rebornb71b4688, kernel
`6.18.0-y2linux-cpu-c3-uart-01`, rootv1.20/buildY2LINUX-CPU-C3-UART-01,
boot `fd955840-35d9-47db-83e0-ff47d6bb2d2b`, taint0. Checkout55db6d3 is a later
receipt, not installed source. C3 remains disabled/budget0 and actual dormant/
UART handshake counters0. UART0 sleep enable1 and UART1 conditional deferred
bit0x20000 are now physically observed. UART1 is no longer a mandatory gate
prerequisite; global SPM ACK and context return remain unobserved. Current
normal Wi-Fi/USB/1300MHz adds legitimate dynamic blockers, not a new settled
UART diagnosis. Existing candidate02 C1/C2 qualification stays authoritative
until regression checks on this image complete. Raw evidence:
`out/cpu-c3-uart-physical/20261003T183259Z-uart01/`.

Dependencies #28/#31/#34 stay OPEN; no milestone closure or hardware-limit
claim. Proceed with normal C1/C2/timer/hotplug/storage/five-OPP regressions,
then natural quiet CPU0/low OPP/radios-off/USB-detached topology. One budget1
RGU/SRAM/timer-wake trial must show real request/ACK and checked same-boot
return before20 further bounded cycles. Failure disables C3 and stops trials;
observe the precise stage, retain independent regressions and fix that cause.
Stop paused playback through the existing authorized owner control. No forced
UART gate, arbitrary MMIO/probe, memory/partition change, flash or push; preserve
six preexisting owner documentation edits. No general CPU redesign, full-system
suspend or electrical battery claim is authorized by this narrow trial.

The installed UART01 regression pass now proves four-core C1, natural parking,
C2 +6,971 entries/+50,654,676us over60.0549s with zero restore failures, all
five OPPs, hotplug, timers, eMMC/inserted SD, screen/workload/playback and
USB/Wi-Fi integrity. Same boot/taint0, no filesystem/IRQ faults. Its first C3
window stopped at legitimate MSDC0 bit12 (0x1000), not UART1: the harness
fsynced its durable arm receipt after the clean preflight, waking eMMC. Actual
UART requests/ACK attempts, CIRQ transactions, CPU context entries and returns
all remain0. Therefore this is an observer sequencing defect, not a failed
deep-idle experiment. Preserve the kernel/storage guard. Targeted harness fix
saves before a read-only two-second stable admission window, checking the real
MMC gates, all exposed prerequisites and owned physical CPU0 topology; no
storage writes occur between that window and enabling C3. A C3-only continuation
requires the exact clean same-boot regression receipt and zero actual UART/CPU
entries both in that receipt and on-device immediately before mutation. Any
real UART/context refusal stops repeats; first checked entry must still precede
20 further cycles. Thirty targeted harness/CPU/UART tests pass. Scope remains
unchanged; dependencies stay open pending real hardware ACK/context results.

UART01 FIRST REAL DORMANT RETURN is now physically proved on the same boot.
Corrected observer sequencing reached global UART request/ACK1, exact POWER1
0x15820→0x15821→0x15820, R13 ACK0x8140000, one CPU entry/resume/success,
8,065us SPM/9,363us cpuidle residency, GPT wake0x10, CIRQ clone/flush1/1 and
local timer context save/restore1/1. SRAM records UART_REQUEST→UART_ACK→
DORMANT_CONTEXT→DORMANT_FINISH→DORMANT_RETURN→DORMANT_COMPLETE with actual
reset-resume assembly marker0x59325253; no restore/context/media/IRQ fault,
taint0 and unchanged fd955840 boot. The harness incorrectly rejected the
otherwise successful trial because spm.c counts every admission before the
budget check: +3 admissions included +2 safe -EACCES budget refusals after the
single physical entry. Fix measurement to require EXACTLY one physical entry/
return/success/ACK, account all extra admissions as aborts, require final budget
refusal and every original restore guard plus SRAM reset marker. Original raw
FAIL receipt remains intact; independent first-entry recheck is attached.
33 targeted tests pass. Continue ONLY20 further budget1 guarded cycles after
rechecking the same-boot first-entry and original regression receipt hashes;
no retry of any actual hardware failure. C3 stays disabled/budget0 between
trials. Full repeatability/policy qualification and dependency closure remain
pending; no kernel build or image changes are needed for these harness fixes.

Repeatability now physically proves18 UART ACKs/CPU reset entries/resumes/
successes on unchanged fd955840 boot, positive residency and zero UART/CIRQ/
timer/clock/context/media faults. Sixteen further cycles passed directly;
cycle17 also genuinely succeeded, but the checker overrequired the LAST
admission to be budget-refused when its two safe guard refusals preceded
the successful entry. Correct verdict permits last resumed/result0 as well
as budget/-EACCES, still accounting every refusal and requiring exactly one
new physical entry/ACK/checked reset return and every restore. Independently
rechecked all18 raw cycles (original FAIL receipts retained with their SHA256).
Continue only the remaining3 bounded cycles, then near-deadline fallback and
60s normal-policy residency. Any real failure still stops; ordinary policy
may stay enabled for THIS BOOT only after all21 checked cycles and postwake
regressions pass. Fresh boots retain image default-off/budget0; no persistent
policy/image/default-on change is implied. No general CPU or suspend milestone
closure; release claims may advance only to the actual bounded idle evidence.

## C3 UART sealed candidate boundary — 2026-10-03

Exact MT6582 PIO-owner admission/global UART request-ACK integration is fixed;
C1/C2 and all unrelated clock/context/timer/OPP architecture are preserved.
Candidate `out/y2linux-cpu-c3-uart-candidate/` at Linuxe9e8d63/Rebornb71b4688
is freshly built/sealed. Production375/source109/native host/Reborn238 tests,
ARM/QEMU/config/DT/modules/ABI/ELF and preserving package checks pass; five
minimal-host dependency skips have separate native/ARM coverage. Sources and
collector limitations are attached; package remains owner-local.

Fresh closing SSH still verifies installed candidate02/unchanged boot,
C3 disabled/budget0/entries0; C1/C2 working evidence remains authoritative.
C3 has never entered. #28/#31/#34 and CPU idle acceptance remain open until
owner-installed UART01 gives real ACK and bounded same-boot context/timer wake.
No hardware-limit proof, full suspend/electrical battery claim, protected data
change, flash/push or milestone closure. Owner alone installs BOOTIMG+Y2ROOT
with the preserving scatter; harness is ready for first entry then20 further
cycles. Six preexisting owner doc edits remain separate. Exact source, hashes,
checks and owner action are in
[the C3 UART handoff](../validation/Y2-CPU-C3-UART.md).

## C3 UART software freeze audit — 2026-10-03

Targeted stock-contract implementation and100 focused tests are complete.
Installed candidate02/source/boot and actual C1/C2 results remain unchanged;
C3 has never entered. The copied default UART1 mask lacked stock PIO-owner
integration. Conditional UART1 sleep adoption, exact MT6582 console driver
sleep capability, bounded global SPM ACK/refusal, checked unwind, retained
stages and first-entry/20-further-cycle harness replace that missing contract.
All unrelated clock guards, C1/C2/timers/parking/OPPs/media architecture remain.
Exact source/ownership/unknown RTL limits are documented in
[the C3 UART record](../validation/Y2-CPU-C3-UART.md).

Before production handoff, build one fresh kernel/Buildroot/Reborn ARM pair and
validate config/DT/modules/ABI, regressions, ELF/QEMU and preserving package.
Candidate scope is BOOTIMG+Y2ROOT at `out/y2linux-cpu-c3-uart-candidate/`,
API1/data/layout unchanged, no protected payload, no flash/push. No hardware
promotion: #28/#31/#34 and CPU completion remain open pending real ACK and
bounded same-boot DORMANT wake after owner installation. No new general audit,
ROM/recovery provenance pass, electrical measurement or full suspend claim.

## C3 UART stock-contract admission — 2026-10-03

Owner narrows work to the candidate02 UART1 C3 blocker. Preserve physically
working C1/C2 and their architecture; do not conduct another general CPU audit.
Fresh targeted SSH verifies installeddb0234e/b71b4688, kernelidle-02/rootv1.19,
buildY2LINUX-CPU-IDLE-COMPLETION-02, same boot
`f155196b-64d4-45a4-88b3-27755a1a8926`. UART1 remains clocked with IRQ/DMA/LSR0;
actual dormant calls0, C3 disabled/budget0. Restored normal radio/OPP policy
legitimately adds other current preflight reasons; the prior settled receipt
still isolates UART1 alone. Expected prior diagnostic taint4096 is retained.
New read-only admission: `out/cpu-c3-uart-physical/20261003T164546Z/`.

Exact MT6582 d53dd75c `mt_idle` default PERI mask includes UART1 bit17;
`mtk_uart_startup` enables DPIDLE for non-DMA ports and enables UART_SLEEP_EN,
while DMA ports remain blocked. The global SPM R7 request/R13 ACK follows PCM
fetch and precedes PCM register/power/run/WFI programming. Existing Y2Linux
has the handshake but lacks UART's conditional mask/sleep-owner integration.
Therefore merely dropping bit17 is unauthorized by the hardware evidence.
The next implementation boundary is conditional PIO sleep adoption with bounded
ACK/refusal, exact unwind and diagnostics, preserving all unrelated blockers,
CCF ownership, C1/C2 and existing timer/context/CIRQ safeguards. No forced gate,
FIFO/data/baud/reset writes, invented PCM, memory/partition expansion or live
C3 trial on the old kernel. Normal console ownership must provide its sleep
capability as required by the same global handshake.

Existing #28/#31/#34 clock/idle dependencies remain open; CPU completion is not
closed or hardware-impossible. User authorizes one fresh preserving candidate
`out/y2linux-cpu-c3-uart-candidate/`, BOOTIMG+Y2ROOT only, with owner-only flash
and no push. Software validation and a prepared first-entry/20-cycle harness
must precede handoff; physical DORMANT success remains unobserved until then.
Unchanged ROM/recovery provenance, protected data and six owner doc edits stay
separate. Full-system suspend/electrical battery benefit remain outside scope.

## Candidate02 physical closing audit — 2026-10-03

Owner-installed candidate02 is verified at Linuxdb0234e/Rebornb71b4688,
kernelcpu-idle-02/rootv1.19/releasecandidate.2, unchanged boot
`f155196b-64d4-45a4-88b3-27755a1a8926`. Repo HEAD at admission76567e5 is a
later receipt, not the installed source. Fresh raw SSH/source/issue evidence:
`out/cpu-idle-completion-physical/20261003T144857Z-candidate02/`.
Blueprint/owner scope remains real C1/C2/C3 with no fake guards/counters, no
flash/push and unchanged memory/partition/recovery contracts. The existing
#16/#27/#28/#29/#31/#32/#33/#34 refresh remains OPEN; no epic is activated/closed.

| Dependency / coverage | Candidate02 actual evidence | Boundary |
| --- | --- | --- |
| C1/GPT6/GPT4/PPI29/highres/NO_HZ, #28 | All four states/wakes/residency, timer continuity and13MHz readbacks pass | Preserve working architecture |
| C2/parking/MMC, #28/#33 | Natural CPU0/owner0xe;6772entries/50.669913s in60.048084s (84.38%); restores0; both media integrity/errors pass | Working narrow behavior; SD absent/electrical energy unobserved |
| Hotplug/DVFS/display/USB/radios, #27/#29/#31/#34 |18 exact physical hotplug transitions, five OPPs, screen/playback/USB and independent Wi-Fi observer pass | Carry qualified regressions; radio endurance/actual failover not claimed |
| C3/retained clock owner, #28/#31/#34 | SPI0 released; settled CPU0/747.5MHz/both power copies clear; only UART1 PERI0x20000 fails; actual dormant calls0 | Blocked; close exact UART ownership before bounded dormant trial |
| Dormant context/CIRQ/GPT4/cache/coherency | Static readiness/software checks only; first/repeated deep wake not exercised | Unqualified; no hardware-limit proof |
| Full-system suspend/electrical battery/other release gates | Prior failures/missing measurements retained | No promotion or waiver |

Main qualification/settled preflight taint0. Two bounded exact-kernel probes
then record UART1 reset/sleep/IRQ/DMA/LSR0, DLL1/DLH0, exact LCR restore and
balanced CCF refs; they do not prove safe gating. Expected out-of-tree taint4096
remains truthfully until owner reboot, with no resident probes or new critical
errors. No C3 trial under diagnostic scope. Final same-boot controls: C3 off,
budget0/RGU disarmed, schedutil598000–1300000/coordinatorY, USB device, original
screen/radio policy restored, ext4/MMCerrors0, temporary observers/files removed.
Playback stays stopped because its initial paused fixture was already deleted.

The harness now waits for normal parking again after synced storage, checks
owner and both power copies for20s, and keeps progress saves sparse. Fresh locked
CPU90+slow-idle/suspend8+workload2 cases pass;19 native probe guard cases pass.
No production kernel/root changes or new image; sealed02 hashes still match.
Preserve the six preexisting owner documentation edits and unchanged provenance.
The physical pass is complete, but CPU idle completion/release acceptance is
**blocked by UART1**, not closed. Current candidate02 receipt supersedes its
historical NOT_RUN/owner-flash instruction below. See
[physical record](../validation/Y2-CPU-IDLE-COMPLETION-PHYSICAL.md) and
[source/harness record](../validation/Y2-CPU-IDLE-COMPLETION.md).

## Candidate02 bounded UART1 divisor snapshot admission — 2026-10-03 15:34 UTC

The first nonresident exact-kernel probe retains UART1 gate0 before/after normal
CCF borrow/release, reset0/0, sleep0, FCR_RD0, ACTIVE_EN0, IRQ/DMA0, LSR0;
source-pinned non-destructive operands are captured in the current physical run.
This disproves a held PERI reset or enabled sleep-control explanation; it does
not establish transmitter-idle or a hardware limit. Same installed pair/boot,
C3 disabled/budget0, expected external-module taint4096, no resident probe.

The next existing-UART observation uses the exact MT6582 BSP
`mtk_uart_save`/`mtk_uart_cal_baud` LCR.DLAB select/read DLL+DLH/restore sequence.
It changes only the saved register-bank selector and checks its restoration;
never writes baud, FIFO, data, IRQ, DMA, GPIO, or reset. Host disables the normal
coordinator, waits for all four cores, and switches radios off through their
owners; both physical power copies must have MD/CONN off. Module requires CPU0,
all four cores, UART1 already clocked, LCR0, and IRQ/DMA disabled before its
bounded IRQ-disabled bank transaction. This excludes concurrent unused-clock
handoff (its worker requires enabled coordinator/one core). Unknown/live/banked
states are skipped. Exact ARM build and11 actual-function guard/cleanup tests
pass; module returns EAGAIN and is not resident. The existing expected4096 taint
is retained truthfully. Radio/coordinator settings restore in finally. No C3
entry, guard relaxation, live-media reset, memory/partition change, flash/push,
external milestone activation/closure, or unrelated release promotion.
Raw evidence remains `out/cpu-idle-completion-physical/20261003T144857Z-candidate02/`;
#28/#31/#34 remain the clock-owner dependencies.

## Candidate02 UART1 read-only diagnostic admission — 2026-10-03 15:20 UTC

Fresh settled preflight in boot `f155196b-64d4-45a4-88b3-27755a1a8926` at
Linuxdb0234e/Rebornb71b4688 proves CPU0-only, owner0xe, both secondary power
copies0, domains0,747.5MHz, CIRQ/timer/context/PCM prerequisites available;
only `clocks` fails, PERI0x00020000/UART1. SPI0 is correctly released. LCR/IER/
DMA/LSR remain0 after repeated clocked rechecks. No C3 call occurs. Supported
USB/radio/DVFS/screen/coordinator settings restore, same boot/taint0/ext4errors0;
new live admission verifies C3 disabled/budget0/backstop0 and exact installed pair.

The existing #28/#31/#34 clock-owner dependency needs UART reset/sleep operands,
which the installed handoff diagnostic does not expose. A bounded, source-pinned
read-only kernel probe is admitted for this existing subsystem. It accepts only
the exact Y2 CCF UART1 handle and fixed MT6582 register windows, requires its gate
already on, borrows/releases through normal CCF, skips alternate banks and live
IRQ/DMA LSR, and never writes UART/GPIO/IRQ/DMA/reset controls. All eight actual
probe guard/ref-cleanup fault cases and its exact installed-kernel ARM build pass.
It returns EAGAIN after cleanup, leaves no resident module, and truthfully leaves
the expected external-module taint until the next owner boot. Qualification
results obtained beforehand retain their original taint0 receipt. No C3 entry
will be attempted under this diagnostic scope. This is not production activation,
hardware-limit proof, a blocker-mask relaxation, memory/partition expansion,
flash/push, or an unrelated release closure. Full-system suspend remains excluded.
Raw source/build/fault/live receipts stay in the candidate02 physical session.

## Candidate02 post-flash hardware admission — 2026-10-03 14:50 UTC

Owner reports installing the sealed follow-up02 and requests physical tests.
Fresh authenticated USB SSH verifies Linux `db0234e7519c559031da6f427869ab683ffe0f3c`,
Reborn `b71b468860233faa0a42b8448ec5777fa952b8e3`, kernel
`6.18.0-y2linux-cpu-idle-02`, root `2025.02.18-platform-v1.19`, release
`1.0.0-cpu-idle-completion-candidate.2`, and boot
`f155196b-64d4-45a4-88b3-27755a1a8926`. All manifest identity checks match;
taint0 and mounted ext4 error counts0. Raw commands/exit codes/timestamps:
`out/cpu-idle-completion-physical/20261003T144857Z-candidate02/`.
Read-only issue refresh retains #16/#27/#28/#29/#31/#32/#33/#34 OPEN.

| Dependency | Current hardware admission | Required fresh result |
| --- | --- | --- |
| C1/timers, #28 | All four 13MHz timers admitted, C1 registered; prior01 physical pass retained | Four-core residency/wake and GPT6/GPT4/PPI29/highres/NO_HZ continuity |
| C2/parking/storage, #28/#33 | C2 registered; current Wi-Fi APDMA/BTIF clocks legitimately block; both MMC hosts gated/error0 | Normal radio policy/quiet parking, useful residency, exact restore, integrity |
| C3/clock owners, #28/#31/#34 | SPI0 quiet/released, UART1 retained with LSR0; C3 disabled/budget0, actual entries0 | Guarded late handoff and individual preflight; only then one bounded wake, 20 only after success |
| DVFS/display/USB/radios, #27/#29/#31/#34 | Guarded1300MHz ceiling and display handoff preserved | All five OPPs, screen/playback, USB and independent Wi-Fi regressions |
| Full-system suspend/electrical battery/other release gates | Separate failures or missing evidence retained | Excluded; no unsupported promotion |

The prepared durable device-side runner uses supported owners and restores
screen/radio/DVFS/coordinator settings. Playback state is recorded before normal
stop. It does not force busy UART/DMA/IRQ gates, weaken masks, reset live media,
perform userspace MMIO, flash, push, change memory/partition scope, or close
external milestones. C3 remains disabled on any failed prerequisite or wake;
normal enablement requires repeated physical success and remaining regressions.
The physical phase is admitted, not accepted. Preserve owner documentation edits
and the sealed packages; unchanged ROM/recovery provenance needs no recapture.

## CPU idle hardware closing / candidate02 owner-flash boundary — 2026-10-03

Fresh final SSH receipt after build/sealing retains installed01 Linux3dfb5f5/
Rebornb71b4688, boot `e0eb2f36-7c1f-4007-99e2-81d2eae047d4`, taint0, mounted
ext4 errors0. Screen off, Wi-Fi online/Bluetooth off, USB device, original DVFS/
coordinator restored; C3 disabled/budget0; temporary observer removed. Active
issues#16/#27/#28/#29/#31/#32/#33/#34 refresh read-only and remain OPEN.

| Dependency | Installed01 actual evidence | Follow-up02 boundary |
| --- | --- | --- |
| C1/GPT6/GPT4/PPI29/highres/NO_HZ, #28 | Four-core WFI/wake/timer continuity passes, failures0 | Preserve; candidate02 repeat |
| C2/parking/MMC, #28/#33 | 7040entries/49.674049s of60.047965s; restores0, both media integrity/errors pass | Preserve; candidate02 repeat |
| Display/USB/demand/OPPs, #27/#29/#34 | Screen/playback/five OPPs/USB/Wi-Fi passes; clocks release normally | Candidate02 affected regressions |
| C3/clock owners, #28/#31/#34 | UART1/SPI0 PERI0x02020000 preflight reject; no actual entry | Source-correct checks and guarded late handoff/operands; actual guards/wake unknown until owner flash |
| Full-system suspend/electrical battery/other release gates | Prior failures or missing evidence retained | No promotion or external closure |

Candidate02 source db0234e/b71b4688 is freshly built and sealed, kernelcpu-idle-02/
rootv1.19. CPU owner/fault100, production366 with native skips covered, Reborn238/
fmt/clippy, fresh ARM kernel/root, config/DT/modules/ABI, QEMU/dependencies/
inventory/legal-info and preserving/root/source/checksum checks pass. C3 category
**SOFTWARE_READY_NEEDS_NEW_FLASH** means ready for bounded qualification, not
proof both clocks clear. One guarded entry and then19 more only on first success
remain mandatory. Latest package:`out/y2linux-cpu-idle-completion-02-candidate/`;
installed01 package remains unchanged. No reused root image; all affected/local
ARM components rebuilt after final source cutoff. Exact Hardware02 fallback,
Y2DATA/protected partitions, memory ABI and unchanged ROM/recovery provenance
remain preserved. No unsafe MMIO/IRQ/DMA/reset forcing, guard-mask reduction,
flash/push or unrelated release closure. Owner-controlled installation is the
remaining boundary; see [source/hashes/action](../validation/Y2-CPU-IDLE-COMPLETION.md)
and [physical result](../validation/Y2-CPU-IDLE-COMPLETION-PHYSICAL.md).

## CPU idle candidate hardware result and follow-up admission — 2026-10-03

Fresh candidate01 run at Linux3dfb5f5/Rebornb71b4688, boot
`e0eb2f36-7c1f-4007-99e2-81d2eae047d4`, completes with **CPU idle acceptance FAIL**:
C1 passes all four cores; natural parking reaches CPU0/owner0xe; C2 adds
7,040 entries and49.674049s/60.047965s (82.72%), restore failures0. Both mounted
media pass16 fsynced checksum cycles each across the two storage phases, ext4
counts0. Three hotplug cycles, all five OPPs before/after preflight, screen wake,
real playback and bidirectional USB/independent Wi-Fi integrity pass. Same boot,
taint0, owner screen/radio/DVFS/coordinator restored; C3 off/budget0/backstop0.

C3 is **not physically working**: preflight rejects PERI0x02020000, exact UART1
bit17 and SPI0 bit25, after normal radio-off/USB-role-none, CPU0-only topology,
747.5MHz cap and an armed RGU backstop. Every other reported static predicate
passes; no actual cpu_suspend call occurs. Boot warnings and CCF summary show
both retained, zero references, no Linux consumer. Installed diagnostics omit
raw handoff operands; do not infer UART's exact busy field or fake C3 success.

Pinned exact MT6582 d53/3be93a68/krillin SPI source identifies STATUS1[0]=1 as
idle; the installed predicate instead accepts0/rejects1. UART platform source
identifies DMA_EN bit2 as timeout-counter metadata, unlike RX/TX bits0/1.
Correct those source-backed decisions, retain unknown/live engines, capture
read-only clocked handoff operands, and preserve aliased UART banks. Partial
NAND/PWM handoff must retain its gate without aborting the shared CCF provider.
No blocker masks, timer/context/PCM/CIRQ/OPP algorithms or memory ABI change.

The unused-clock owner also rechecks retained UART1–3/SPI0 through the existing
CPU0-only, screen-off, lease-free deferrable worker, only with both radios
physically off. It borrows/releases through CCF, keeps IRQ/DMA/unknown engines,
never reads gated or aliased windows, avoids live UART clear-on-read LSR, and
quarantines a failed gate readback. A transient loader operation no longer pins
its clock forever. No new timer, coordinator-policy/hysteresis change or reset.
Native fault tests cover real refs, still-busy engines, unavailable domains,
unclocked skips, allocation/borrow and gate-readback failures.

Existing#28/#31/#34 track this next owner dependency; all refreshed issues remain
OPEN. The original autonomous completion instruction authorizes safe source
fixes and one coherent follow-up preserving candidate. Candidate02 gets a fresh
kernel/root build and affected/full production verification; no reused root
image, no flash/push, no external closure, no unrelated qualification claim.
Current candidate01 hardware evidence stays attached to its actual source pair;
candidate02 will remain physically unqualified until owner flash. Full-system
suspend and electrical battery improvement remain separate unresolved gates.
[Physical record](../validation/Y2-CPU-IDLE-COMPLETION-PHYSICAL.md), private raw
`out/cpu-idle-completion-physical/20261003T130848Z-candidate/`.

## CPU idle candidate physical admission — 2026-10-03 13:13 UTC

Owner reports installing the sealed CPU idle BOOTIMG/Y2ROOT and explicitly
requests hardware qualification. Fresh authorized USB SSH verifies actual
Linux3dfb5f5/Rebornb71b4688, kernel6.18.0-y2linux-cpu-idle-01,
root2025.02.18-platform-v1.18/releasecpu-idle-completion-candidate.1,
boot `e0eb2f36-7c1f-4007-99e2-81d2eae047d4`, taint0; all identity axes match the candidate manifest.
Raw admission/status/health/dmesg/timer/IRQ receipts: `out/cpu-idle-completion-physical/20261003T130848Z-candidate/`.
Active issues#16/#27/#28/#29/#31/#32/#33/#34 are refreshed read-only; remain OPEN.

| Coverage/dependency | Real current evidence | Qualification boundary |
| --- | --- | --- |
| GPT6/GPT4/PPI29/highres/NO_HZ | Admitted13MHz/per-core errors0, no critical dmesg lines | Fresh bounded continuity and four-core wake |
| C1/C2/parking | Registered; natural CPU0/secondary bits0, prior baseline passes historical | Fresh counters/residency/restore and supported hotplug |
| Display/INFRA owner fixes | Current screen off: DISP0/1 blockers0, INFRA blockers0 | Screen restore/playback/USB regressions |
| C3 | Disabled, real entries0; current frequency/radio/USB/PERI/policy guards reject | Resolve through real supported owners; one RGU-backed entry only after prerequisites, then20 only on success |
| Storage/DVFS/thermal | Mounted eMMC/SD ext4 counts0, temperatures42.8–42.9°C | Fsynced integrity/runtime PM/all five OPP readbacks |
| Full system suspend/unrelated release gates | Historical failures and unqualified scope retained | Excluded from this CPU qualification; no promotion |

Pause/session state is recorded before normal playback stop. This pass uses the
prepared durable device-side SSH harness with independent Wi-Fi observation,
supported radio/USB/CPU controls, normal-policy parking, failure quarantine and
owner-setting restoration. Ordinary C3 is retained only after repeated bounded
success and the remaining regressions. No guards are weakened, no userspace
MMIO or live-media reset, no flash/push, no memory/partition/calibration change,
no unchanged ROM/recovery recapture or external milestone closure. The physical
phase is admitted; acceptance is pending fresh evidence. Preserve the preexisting
owner documentation edits and exact Hardware02 fallback.


## CPU idle owner-flash boundary — 2026-10-03 11:07 UTC

Repeated standing gap audit uses final current-boot receipts21–23 and read-only
GitHub receipt24, alongside the complete CPU evidence and the fresh committed
build pair. Installed baseline remains Linux131ee462/Reborn7f9df397, boot
cee326c1-a5b0-447a-8eb0-dc3f39f7e2c2, taint0. C1 is WORKING on all four cores.
Natural CPU0 parking plus C2 adds7182 entries/50.763741s in60.404076s; controlled
30-second enabled/disabled/re-enabled windows add25.023/0/25.016s SLIDLE,
respectively. eMMC/inserted SD ext4 errors remain0. Owner screen/radio/DVFS/USB
policy is restored; the temporary owned Wi-Fi observer is stopped.

| Coverage/dependency | Current classification | Next boundary |
| --- | --- | --- |
| C1/timer foundation, #28 | WORKING on installed baseline; frozen algorithm | Candidate regression through single automated SSH harness |
| C2/parking/MMC, #28/#33 | WORKING after legitimate quiet/pressure hold; storage integrity/restore pass | Preserve guards; qualify unchanged paths with new clock owners |
| C3/clock/context/CIRQ, #28/#31/#34 | SOFTWARE_READY_NEEDS_NEW_FLASH; no real entry observed | Owner installs one preserving candidate; one guarded entry, then20 only after success |
| Active DVFS/hotplug, #28/#29 | WORKING in bounded baseline tests, all five OPPs/three cycles | Candidate pre/post-dormant voltage/topology/thermal regression |
| Display/USB owner dependencies, #34/#27 | SOURCE_IMPLEMENTED and SOFTWARE_VALIDATED; baseline USB0 leak observed | Real screen-off clock release and USB runtime restore after owner flash |
| Full-system suspend, audio/radio/GPU/storage ceiling/endurance | Prior narrow evidence and unresolved gates retained | No unrelated closure or promotion from CPU idle evidence |

Fresh clean paired source 3dfb5f5/b71b4688 produces kernel6.18.0-y2linux-cpu-idle-01
and root2025.02.18-platform-v1.18. Kernel/config/DT/modules/ABI, fresh Buildroot/
Reborn ARM, production regression364 cases (five locked-host prerequisites
covered by25 packaging-host cases), Reborn238 host cases/clippy/fmt, installed
ARM/QEMU,383 ELF files/1403 dependency edges and release inventory/legal-info
complete. Software evidence is not current-candidate hardware acceptance.
Preserving-package/root/composition/checksum validation and final hash seal pass;
owner-flash handoff is ready. No reuse-userspace.

Active issues#16/#27/#28/#29/#31/#32/#33/#34 remain OPEN; no external status is
changed. CPU work maps to existing#28, owner dependencies to#27/#31/#33/#34.
The next phase is software-ready, physically unqualified until owner flash.
Recovery/fallback uses the exact Hardware02 BOOTIMG/Y2ROOT pair. Memory, ROM/
recovery provenance, partition layout, factory/calibration protection and Y2DATA
preservation remain unchanged. No hardware limitation is inferred from an
untested prerequisite. No milestone is closed, no default-on C3 patch, no flash
or push. See [implementation](../validation/Y2-CPU-IDLE-COMPLETION.md) and
[physical record](../validation/Y2-CPU-IDLE-COMPLETION-PHYSICAL.md).



## CPU idle completion admission — 2026-10-03
### CPU idle dependency audit — 2026-10-03 08:05 UTC

Current-boot receipts04–11 now physically close C1 and C2 on the installed
baseline: four-core WFI counters/residency and timer continuity pass; natural
parking reaches owner0xe/CPU0; C2 adds7182 entries and50.763741 seconds residency
in60.404075502 seconds (84.0%), with restore failures0. Eight uncached, fsynced
SHA256 cycles per eMMC/SD medium pass in each radio-off run, ext4 counts0.
Three hotplug cycles and all five guarded OPPs pass. C3 remains unentered.
Live screen-off still owns OVL/RDMA/COLOR/DSI and32-kHz mutex references;
inherited AFE clocks remain enabled with zero references. This new evidence
supersedes the admission's pending C1/C2 observations, not unrelated gates.

Exact d53dd75c/3be93a68 MT6582 `dpidle_condition_mask` requires DISP0 0x7ffff
and DISP1 0xf clock quiescence, rather than unconditional DISP-domain power-off.
CPU completion therefore includes the existing Reborn DRM master's screen-sleep
lifecycle, balanced shared display/audio clock ownership, and read-only checks
for safely retiring inactive loader clocks. Active or unknown engines remain
blockers. This dependency is within the owner's explicit CPU clock/wake scope;
it authorizes no new display feature, arbitrary power-domain shutdown, DMA reset,
MMIO userspace access, memory layout, partition, flash or push. Existing GPU
and radio ownership and active performance ceiling remain required regressions.



Owner explicitly authorizes an autonomous CPU/idle implementation and physical
qualification pass on installed `y2linux-baseline-candidate`, with one fresh
`y2linux-cpu-idle-completion-candidate` preserving BOOTIMG/Y2ROOT package.
Only the owner flashes; no push, protected-partition or Y2DATA payload is admitted.

Read-only SSH verifies Linux `131ee4621cd583c955182f994adac5a594fb823f`,
Reborn `7f9df397ab3809d52e2f1073ca93a305246e0c3a`, kernel
`6.18.0-y2linux-baseline-01`, root `2025.02.18-platform-v1.17`, release
`1.0.0-baseline-candidate.1`, boot `cee326c1-a5b0-447a-8eb0-dc3f39f7e2c2`,
taint0, matching the sealed package. This supersedes installed Candidate4
identity only; retained Fix02 passes/failures remain historical evidence.
Private receipts: `out/cpu-idle-completion-physical/20261003T074026Z/`.

Initial current coverage: GPT6/GPT4/PPI29/CNTFRQ13MHz/highres/NO_HZ admitted;
all five guarded OPPs admitted; CPU0-only coordinator-owned topology observed.
C1 registered/enabled with accumulated residency, fresh continuity qualification
pending. C2 registered/enabled but entries0, live APDMA/BTIF blockers while Wi-Fi
runs; bus rejection0 and MMC runtime gating intact. C3 registered/default-off,
entries0; current frequency, domains and clocks reject its read-only preflight.
No new idle-state pass is claimed. Existing storage tuning remains Candidate4.

GitHub active issues refreshed read-only: #16/#27/#28/#29/#31/#32/#33/#34 OPEN;
CPU/timers/idle map to #28 and storage/radio/GPU interactions to #33/#31/#34.
Their older descriptions do not override current source or hardware receipts.
No external issue or milestone is closed or changed. Every unrelated hardware,
audio/distribution/endurance gate retains its existing classification.

Scope: characterize C1 then freeze its proven architecture; decode/fix actual C2
clock/device ownership; resolve every source-backed C3 topology/deadline/CIRQ/PCM/
context prerequisite, without masking blockers or undocumented register writes.
Bounded existing runtime controls and workload/storage/network regression tests
are authorized, with safe abort and state restoration. First C3 entry requires
all real prerequisites; no repeated hammering of a failed deep path. Preserve
GPT6/GPT4/PPI29, schedutil/QoS/thermal, hotplug, all guarded OPPs, exact memory/
partition layout and Hardware02 fallback. Full system-suspend acceptance remains
a separate gate. Coherent source fixes and fresh software/package qualification
precede owner flash and current-candidate physical acceptance.

Track implementation and physical results in
[CPU idle completion](../validation/Y2-CPU-IDLE-COMPLETION.md) and
[physical qualification](../validation/Y2-CPU-IDLE-COMPLETION-PHYSICAL.md).

### USB clock dependency audit — 2026-10-03

Current-boot receipt12 uses supported USB role `none` over the verified Wi-Fi
observer. MUSB changes active ->suspended and returns active on restoring
`device`; same boot, no source/partition changes. PERI USB0 bit10 nevertheless
remains active in the exact C3 blocker mask. Source confirms the glue adopts
that gate without a CCF consumer. The MT6582 d53 USB PHY's usb_enable_clock
explicitly owns MT_CG_PERI_USB0 through enable_clock/disable_clock.

This CPU completion boundary now admits a balanced USB0 CCF consumer and
MUSB runtime save/gate/enable/restore hooks, preserving attached sessions,
DMA/IRQ refusal, PHY sequencing and ordinary role-based detach/reconnect.
No blocker bit is removed. Only the existing controller is modeled; no host,
VBUS, PHY-retune, power-domain, memory, partition or flash scope is added.
Fresh USB regressions and same-boot role recovery are required. The next C3
harness may intentionally release USB through the existing role interface
while its durable device-side job runs, then restore transport. Radios and
USB may both legitimately prevent C3 until their owners have quiesced.

### CPU idle source/build boundary audit — 2026-10-03

Fresh same-boot receipts04/10/11/12/16/17 now establish four-core C1, useful C2,
physical secondary hotplug, all five OPPs, the USB0 ownership leak, actual44.1-kHz
playback with PlaybackNormal demand/all-core restoration and screen-off/on
userspace transitions. Taint0; mounted eMMC/SD errors0; C3 has not entered.
The source pass preserves the working timer/C1/C2/MMC/DVFS/parking algorithms
and repairs the exact C3 clock/context/deadline/CIRQ/PCM prerequisites. Targeted
native fault tests and fresh Reborn host/clippy checks pass; full fresh software
and package qualification is next. This crosses a software build boundary only:
no new hardware/memory/partition scope, milestone closure, flash or push.
All unrelated full-suspend/storage-ceiling/audio/distribution gates remain open.
One coherent CPU idle preserving candidate and its automated20-cycle harness
are the next deliverables. CPU hardware completion remains pending owner flash.


**Current scope, 2026-10-01:** See [platform state](../CURRENT_PLATFORM_STATE.md)
and the [Fix02 physical result](../validation/Y2-CPU-FINAL-FIX02-PHYSICAL-QUALIFICATION.md).
The owner-flashed Fix02 qualifies the awake CPU platform, but fails acceptance
on full-suspend resume completion, SLIDLE and loaded USB upload. One Fix03
correction candidate is built for the owner's next run; it is not a release.
This roadmap keeps earlier Platform v1/Hardware Final/CPU Final boundary
decisions as history. Their "ready" and "next" language belongs to those
earlier packages.

## CPU Final Fix03 — single correction candidate, 2026-10-01

The [implementation boundary](roadmap-gap-audit.md#cpu-final-fix03-implementation-boundary--2026-10-01)
implements exactly the Fix02 handoff batch. It adds timed resume-window marks
up to `pm_suspend` exit, with the decoded RGU reset cause. It reconciles the
stock bus-DCM baseline for SLIDLE and deep idle, makes the USB storm guard
progress-aware (the Fix02 storm was legitimate throughput), and fixes three
policy defects: wake escalation, SD identity and the DVFS label. The output is
one preserving `out/y2linux-cpu-final-fix03-candidate/` BOOTIMG/Y2ROOT package
with the exact Hardware02 fallback; no flash or push. Next: the owner's single
Fix03 run, then one targeted correction for whichever resume call the timed
ring names.

## CPU Final Fix02 — single correction candidate, 2026-09-28

[Implementation boundary](roadmap-gap-audit.md#cpu-final-fix02-implementation-boundary--2026-09-28)
implements the Fix02 plan without restarting the CPU architecture: MSDC runtime
clock ownership, sustained-idle parking and removal of the recurring
screen-off background work, reconciled PWRAP readiness for 1196/1300 MHz,
awake-proven retained PM diagnostics with a warm-reset backstop, charger-state
discrimination and attributed USB transport faults. One preserving
`out/y2linux-cpu-final-fix02-candidate/` BOOTIMG/Y2ROOT package with the exact
Hardware02 fallback; no flash/push. Next: the owner's single Fix02 run.

## CPU Final Fix01 physical sweep — 2026-09-27

[Admission audit](roadmap-gap-audit.md#cpu-final-fix01-physical-admission--2026-09-27)
records exact already-flashed runtime identity through USB SSH, taint0. Owner
authorizes one observation/stress sweep using the existing harness, automatic
independent continuation and bounded C3/suspend only after prerequisites.
No implementation source edits, builds, flashes or pushes. The observation sweep
is now **COMPLETE at safe physical-test scope, FAILED for CPU Final Fix01
acceptance**. See the [physical report](../validation/Y2-CPU-FINAL-FIX01-PHYSICAL-QUALIFICATION.md)
and [closing audit](roadmap-gap-audit.md#cpu-final-fix01-physical-closing-boundary--2026-09-27).
GPT6/GPT4/PPI29/highres/NO_HZ and all real QoS producers pass. Conservative
OPPs/hotplug/WFI/playback/thermal authority remain functional. SLIDLE stays blocked
by retained MMC runtime clocks; automatic parking is not observed. High OPPs now
pass selector-mode recognition but fail PWRAP readiness. First devices-stage
request loses recovery and SRAM does not identify the kernel boundary; later
USB stress loses connectivity while UI remains usable. Two owner restarts occur,
not demonstrated spontaneous resets. C3/full RTC/Power wake remain NOT_TESTED.
No whole-platform promotion or external epic closure. Preserve exact fallback.

Next smallest boundary is one coherent **Fix02 plan**: prove early retained PM
diagnostic safety, resolve actual PWRAP readiness, correct MMC runtime ownership
without changing card rails/storage safety, reassess sustained-idle coordination,
and fix the captured loaded USB recovery defect. The plan is not a new build,
flash, memory-layout change or architecture workstream authorization.


## CPU Final Fix 01 — focused implementation, 2026-09-27

[Admission audit](roadmap-gap-audit.md#cpu-final-fix-01-implementation-admission--2026-09-27)
activates the owner's single correction batch using completed physical qualification.
Fix GPT6/GPT4/PPI29 admission, complete Reborn QoS writes, decoded clock ownership
and useful SLIDLE eligibility, actual Y2 PMIC selector mode, full suspend/wake and
persistent stages. Preserve conservative paths and bounded CONSYS retry. Build one
fresh `out/y2linux-cpu-final-fix01-candidate/` preserving BOOTIMG/Y2ROOT package with
exact fallback; no flash/push. No hardware acceptance or external epic closure.

Implementation now reaches the single-candidate build boundary: timer ownership,
Reborn/worker leases, clock lifecycle/coordinator, both legitimate PMIC banks,
persistent suspend stages and clean charger unwind have focused passing tests.
Proceed with fresh broad software qualification; next physical run alone may
accept highres/NO_HZ, actual SLIDLE/high OPP entry and same-boot RTC/Power wake.


Fresh software qualification is complete for runtime Linux0de6e95/Reborn36db186:
215 locked passes plus12 host native tests,192 Rust passes/fmt/strict Clippy,
ARM/QEMU,382 ELF files/1396 dependency edges and105 pinned source packages.
Seal one preserving BOOTIMG/Y2ROOT package with the exact Hardware02 fallback;
physical acceptance remains pending the one SSH qualification sequence.


COMPLETE at software-candidate scope: one coherent CPU Final Fix01, BOOTIMG82b38fd3
and Y2ROOT26aa01ce, preserving validation/exact fallback PASS. See the
[implementation/validation receipt](../validation/Y2-CPU-FINAL-FIX01.md).
Close this correction pass before owner-controlled flashing. Highres/NO_HZ,
SLIDLE/high OPP entry, full RTC/Power wake and SRAM retention remain physical
checks; no epic, automatic sleep or C3 qualification is promoted.



## CPU Final physical qualification — 2026-09-27

[Closing boundary audit](roadmap-gap-audit.md#cpu-final-physical-qualification-boundary--2026-09-27)
and [physical report](../validation/Y2-CPU-FINAL-PHYSICAL-QUALIFICATION.md) record
COMPLETE observation sweep, FAILED CPU Final acceptance. Exact flashed pair
ff586df/afcf9ff matches; lower OPPs/schedutil/hotplug/WFI/kernel QoS, native silent
playback and USB/WLAN data work. New GPT6/GPT4/PPI29 admission fails, highres/NO_HZ
are inactive, Reborn device hint writes fail, high-bin voltage admission is gated,
SLIDLE remains blocked and dormant/deadline/CIRQ entry unqualified.

Freezer and battery devices/platform/processors/core pass. Quiescent MUSB resume
avoids storm; isolated CONSYS retry succeeds after a captured mandatory-STP
failure. First full RTC suspend never returns; loss of USB/Wi-Fi recovery ends
risky testing, owner restart restores clean filesystems and baseline runtime.
No same-boot Power/RTC full wake or full device-restoration pass. Keep existing
suspend qualification/C3-default-off controls, retain fallbacks, address the one
prioritized fix batch. No source fixes/build/flash/push or external epic closure.

## CPU Final candidate boundary — 2026-09-27

[Boundary audit](roadmap-gap-audit.md#cpu-final-candidate-boundary--2026-09-27)
records completed CPU Final runtime source Linux `ff586df` / Reborn `afcf9ff`.
Fresh kernel/config/modules, Buildroot/ARM, DT/BOOTIMG/memory, 205 locked regressions
plus 12 native dependency tests, 189 Reborn tests/fmt/strict Clippy, ARM/QEMU,
ELF/dependency and pinned source inventory pass. The DT validation-only correction
`3f86af2` enforces CIRQ/MCU context resources and changes no compiled runtime code.
COMPLETE at software-candidate scope: `out/y2linux-cpu-final-candidate/`. The local
package/data/exact-Hardware02-fallback checks pass; no new hardware qualification.
C1/C2/C3, stock-bin automatic DVFS, QoS, timer/context/CIRQ, suspend/device restore,
diagnostics and recovery options are implemented. C3 remains experimental/default
off, CPU0 with other cores physically off; the exact source report records all
preflight/default limits. Retained hardware receipts and qualifications remain
separate, with no new physical campaign, flash or push.

## CPU Final — implementation pass, 2026-09-27

[Admission audit](roadmap-gap-audit.md#cpu-final-implementation-admission--2026-09-27)
uses retained real Hardware02 receipts. Owner authorizes complete CPU/timer/DVFS/
cpuidle/dormant/CIRQ/GPT/suspend software, Reborn semantic hints, bounded fallback
and one fresh `out/y2linux-cpu-final-candidate/` BOOTIMG/Y2ROOT package. Preserve
Hardware Final work and Y2DATA; no flash/push. Implement and source-test independent
paths before full build/package validation. Physical qualification stays independent;
no long qualification campaign or hardware evidence acquisition is part of this pass.

## Hardware Final — single campaign, 2026-09-27

The [activation audit](roadmap-gap-audit.md#hardware-final-campaign-activation--2026-09-27)
confirms installed Hardware 02 (Linux `76bc822` / Reborn `95747e0`) through real
USB SSH evidence. This owner direction supersedes Hardware 03/04/05 and older
per-batch build/flash sequencing. No final capability is inferred from availability.

1. Complete Hardware 02 physical census and broad bounded measurements.
2. Investigate CPU/power/battery, storage/USB, radio/audio and recovery in parallel;
   implement all independently justified corrections on `hardware-final`.
3. Collect required physical actions while independent work continues. Preserve
   truthful electrical/calibration blockers and bounded fallbacks.
4. Audit accumulated changes, run complete current-source host/ARM/image checks,
   and create one `y2linux-hardware-final-candidate` preserving Y2DATA.
5. Stop for one owner installation/qualification cycle; destructive root OTA
   application requires explicit approval. Long endurance and unperformed physical
   cases remain explicit pending evidence, never assumed passes.

[Campaign](../validation/Y2-HARDWARE-FINAL.md),
[capability ledger](../validation/Y2-HARDWARE-FINAL-CAPABILITIES.md), and
[qualification](../validation/Y2-HARDWARE-FINAL-QUALIFICATION.md) own the new scope.
Existing epics remain open. No intermediate image is justified at entry.

## Active hardware capability ceiling campaign — 2026-09-26

Current boundary: [Hardware 02 candidate handoff](roadmap-gap-audit.md#hardware-02-candidate-handoff--2026-09-26).
[Concrete package](../validation/PLATFORM-V1-HARDWARE-02.md) is built and release
validated from Linux `76bc822` / Reborn `95747e0`: source-backed SDR steps,
Inventra DMA, GPT6/PPI29 local timer, tested Reborn sink repair, bounded failure
handling and diagnostics. It is ready for one preserving owner flash with the
exact installed Physical 01 pair as fallback. No new mode is physically accepted.

Recovered suspend evidence proves freezer/devices/platform/processors return,
including CPU3/2/1; USB overflow and radio timeout prevent full restore. A fresh
Power Menu reboot restored pinned SSH and clean storage/taint. RTC standard
UTC write/read/ticking now pass; automatic-write policy, retention and alarm
remain separate gates. Ordinary DNS works while one transient Wi-Fi observer
timeout remains. Post-flash regression and measured 25→50 MHz qualification
precede any capability promotion. Older notes retain their historical boundaries.

The owner's platform-wide ceiling request supersedes incremental feature
bring-up. [Fresh entry audit](roadmap-gap-audit.md#hardware-capability-ceiling-entry--2026-09-26)
binds the work to installed Physical 01 (Linux `198fa7c`, Reborn `feb530f`,
candidate.3, boot `4991ba2c-0571-4649-9f7c-9e0318abb952`). Exact versions match;
IOS reads eight-bit eMMC and four-bit SD at 13 MHz/legacy, taint and ext4 error
counters are zero. The initial regression gate passes, including checked Wi-Fi
DNS/TCP, owner-confirmed wired audio and clean replacement-card workloads.
Physical 01 is the working baseline; no final ceiling or freeze is accepted. Earlier current-target descriptions below are historical.

Continuation receipts `17`–`49` separate storage/query performance, identify
stock CPU table 0 and physically prove GPT6/PPI29. The bounded probe restored
controls and normal reboot returned taint 0. Reborn's PCM metadata repair passes
host regression and temporary ARM SBC playback; owner confirms play/pause.
Original binaries/session were restored. Receipt 49 supersedes the incomplete
SSH-stream interpretation: CPU3/2/1 restart works, while USB and radio restore
fail. The recovered unit is healthy after a confirmed fresh reboot; core/SPM
entry remains unproven. Advance future suspend stages only after local and host
recovery checks.

1. Retain the verified Physical-01 regression baseline; complete matched library,
   RAM/storage transfer, lifecycle and endurance measurements.
2. Investigate all hardware ceilings against retained stock/vendor and current
   physical evidence before selecting changes; preserve conservative fallbacks.
3. Hardware Batch 2: supported eMMC/SD timing, USB transfer/DMA, working local
   timer and observation. Hardware Batch 3: core/idle/suspend/RTC dependencies.
4. Coherent power, radio and audio batches follow their measured prerequisites:
   meter/pack evidence; stable SBC; real DL1 bit transport before higher rates.
5. Build/validate each coherent batch, then one owner flash and broad regression.
   Root OTA/rollback and endurance follow stable storage/power/core. Freeze only
   after the final report distinguishes qualified ceilings from evidence gates.

The owner confirmed menu, clean wired/AirPods tones and a known-good replacement
SD card; Reborn AirPods play/pause works by owner report. No external USB meter
is available presently.
The [ceiling report](../validation/PLATFORM-V1-HARDWARE-CEILING.md) tracks these
limits. Independent investigation continues. Existing issue owners
remain; historical closed #30 does not mean physical power qualification.

## Active physical bring-up campaign — 2026-09-26

The owner's complete campaign supersedes the earlier passive qualification
scope. The [entry audit](roadmap-gap-audit.md#complete-physical-bring-up-campaign-entry--2026-09-26)
reconciles current Telemetry 01 hardware evidence with every coverage area.
Order: broad bounded census → root-cause clusters → coherent fixes → host/ARM/
candidate validation → necessary owner installation → broad regression; repeat.
Independent tests continue after ordinary failures. Only the named safety and
recovery conditions stop all hardware work. No planning row authorizes guessed
SPM, storage clocks, charger limits, VBUS or audio formats.

Current physical target: Linux userspace `814c2f3`, kernel/base `d04b95a`, Reborn
`1556083`, Telemetry 01; newer repository heads and original candidate are distinct.
All historical failures and limited passes below remain evidence, with old
authorization restrictions superseded only by this new explicit owner scope.
The [bring-up report](../validation/PLATFORM-V1-PHYSICAL-BRINGUP.md) and
[master issue table](../validation/PLATFORM-V1-PHYSICAL-ISSUES.md) track progress.
Existing issues stay open; no platform/endurance acceptance is declared.

The [Fix Batch 1 admission audit](roadmap-gap-audit.md#physical-fix-batch-1-admission--2026-09-26)
refreshes all coverage after the broad real-device census. Implement the measured
Wi-Fi checksum, legacy storage-width, benchmark/PSS, library query and Bluetooth
pair/connect fixes as one candidate, then regress the whole baseline. Stock Y2
evidence supports eight/four data lines at the existing storage clock; higher
clocks, timer/SPM activation, charger changes, USB VBUS and wider audio remain
gated. The removed inconsistent FAT card stays isolated. Physical SBC pairing
diagnosis is deferred at the owner's request and the Pairable window is closed;
no pass is inferred from ACL Connected alone.

The [packaging boundary audit](roadmap-gap-audit.md#physical-fix-batch-1-packaging-boundary--2026-09-26)
refreshes all areas with receipts through 56. Linux `198fa7c` / Reborn `feb530f`
compile as one candidate; verify final host/ARM/package receipts and the exact
running-image fallback before owner handoff. The new UI distinguishes a link,
saved bond and audio readiness. Nine decoder suites also pass at the existing
frequency caps, with original policy restored; this does not qualify sustained
playback or justify governor changes. No milestone is closed.

2026-09-23. Owner-authorized software implementation, local commits only; no
physical-device access or flashing. This roadmap does not close hardware epics.
Entry revisions: Linux `5f6b4468fb43605ca1da679420823afa72cea73f`, Reborn
`d9ba0549e6b34eff029bd13f7c33a491fee09e66`.

Software freeze: built pair `d04b95a` / `6c8aa12`, following the
[final boundary audit](roadmap-gap-audit.md#platform-v1-software-freeze-boundary--2026-09-23-utc).
All implemented target contracts below now have final current-source ARM/image
receipts. Host orchestration/inventory tools remain host tools. No row has current
candidate physical or endurance qualification. Optional hardware gates remain
explicit and do not prevent this bounded software candidate.

Read the [entry audit](roadmap-gap-audit.md#platform-v1-completion-entry--2026-09-23)
and [completion ledger](../validation/PLATFORM-V1-COMPLETION.md). Evidence levels
are independent: IMPLEMENTED, HOST_TESTED, ARM_BUILT, IMAGE_VALIDATED,
PHYSICALLY_QUALIFIED, ENDURANCE_QUALIFIED. Workflow labels never imply a higher
evidence level.

| Order | Workstream / tracking | Software boundary | Hardware boundary |
| --- | --- | --- | --- |
| 1 | Telemetry, health, capability/readiness, boot history (#16/#28/#32) | Implemented/host tested; phase-1 and phase-3 ARM/QEMU passed: bounded JSON observation, honest missing data, retained boot stages | Collection overhead, current devices, reset-cause retention |
| 2 | Space, SD, scratch/SQLite/library benchmarks (#28/#29/#33) | Implemented/host tested: stable mount identities, budgets, scratch/library/resource and syscall-fault tools; phase-2 ARM/QEMU including installed 1k benchmark passed | SD loss, I/O tails, electrical durability, 1k/10k/20k target performance |
| 3 | Shutdown, low battery, clock/entropy, Wi-Fi (#28/#30/#31) | Implemented/host tested: independent bounded shutdown, disabled thresholds, DHCP/DNS and clock/entropy; phase-3 ARM/QEMU passed | Thresholds/reserve, charging, RTC retention, DHCP/DNS/reconnect |
| 4 | Bluetooth baseline, AVRCP, codec policy (#31) | Implemented/host tested: observed PCM, bounded reconnect, AVRCP, explicit qualified Auto; fresh ARM/production/QEMU passed | SBC peers/coexistence; optional codecs need distribution and peer evidence |
| 5 | USB transfer/reliability/host feasibility (#27/#28/#32) | Implemented/host tested: authenticated USB-only SFTP, reserve and verified publication; ARM/protocol faults passed | Reconnect/PC sleep, transfer performance; host VBUS/role wiring |
| 6 | Signed staged root updates/recovery, reset/backup (#32/#33) | Final ARM/image validated: signed staging/key policy/offline backup-write-readback/restore, reset/export, ARM fault cases and installed production signature/payload verification | Interruption/recovery; automatic BOOTIMG writes excluded |
| 7 | CPU/idle/suspend and high-resolution audio (#28/#29/#34) | Implemented/host tested queries, precision fixtures and default suspend refusal; wider hardware paths explicitly gated | No speculative SPM/voltage/packing; exact clock/sample/resume proof |
| 8 | Release/security/endurance/candidate (#32/#33) | DONE_SOFTWARE / HOST_VALIDATED / ARM_VALIDATED / IMAGE_VALIDATED: exact paired candidate, source/hash/license inventory, passive endurance tools and owner sessions | PHYSICAL_GATE: owner sessions A–G and qualified operating envelope |

Stock preloader/LK, BOOTIMG rescue, root/data split, protected partition policy,
reserved memory, driver/CONSYS ownership, ALSA/DRM/evdev, FFmpeg, SQLite's single
writer, BlueZ/BlueALSA and bounded workers remain authoritative. No new hardware
or memory scope is authorized by this document. Advance each boundary only after
updating the audit using retained physical evidence; do not repeat unchanged
ROM/recovery provenance. Existing #30 acceptance is preserved, with later adverse
suspend evidence and unqualified charger changes explicitly retained.

## Reborn UI v1 application work — 2026-09-24

The owner now authorizes Reborn UI implementation against the frozen Platform v1
contract before physical platform acceptance. The [activation audit](roadmap-gap-audit.md#reborn-ui-v1-application-activation--2026-09-24)
retains every physical gate and issue status above. Work order: design/input and
navigation; native music/queue screens; service-backed connectivity/system/advanced
observations; focus/error/visual tests; fresh host/ARM/package validation; preserving
UI candidate for owner review. No platform architecture or hardware expansion.
Candidate creation and closure require another evidence audit. No flash.

The [software candidate handoff audit](roadmap-gap-audit.md#reborn-ui-v1-software-candidate-handoff--2026-09-24)
records the final native previews, current-source host/ARM checks and preserving
root package in `out/y2linux-reborn-ui-v1-candidate/`. Application implementation
is complete at software-candidate scope. Platform v1 remains physically unqualified;
the next boundary is owner Session A and the UI qualification checklist, then the
relevant platform sessions. No installation or hardware scope is activated here.

The [bounded graphics memory review](roadmap-gap-audit.md#reborn-ui-v1-bounded-graphics-memory-review--2026-09-24)
permits a shared 4 MiB application font atlas and bounded collection-art worker,
cache and texture. Target RSS/PSS, navigation latency and audio coexistence remain
owner qualification items. Platform reservations and hardware gates are unchanged.

The [UI candidate admission audit](roadmap-gap-audit.md#reborn-ui-v1-candidate-build-admission--2026-09-24)
authorizes fresh ARM application compilation and preserving root packaging after
host checks. Final image and runtime receipts remain required. Platform v1
compiled identities remain distinct from the application/package review heads.

## Physical qualification entry — 2026-09-25

The owner identifies `out/y2linux-reborn-ui-v1-candidate/` as the installed and
intended qualification package. Authenticated versions metadata matches Linux
`d04b95aaff713edf943042d97a4c6134ca19fc24` / Reborn
`155608393f6acfc6657f2f2e23cd07d0533479c6`; local package integrity passes.
The [entry audit](roadmap-gap-audit.md#platform-v1-physical-qualification-entry--2026-09-25)
retains all hardware gates and the original plain-candidate identity mismatch.
The [stop audit](roadmap-gap-audit.md#platform-v1-physical-qualification-stop--2026-09-25)
records **Session A FAIL; B/C/D NOT_TESTED**. Health CLI dispatch crashes and
status observation blocks on wakeup_count; retained kernel warnings trigger the
owner's stop rule. Narrow root/data identity, clean counters, first-frame/main
menu and USB SSH observations are retained in the
[physical qualification report](../validation/PLATFORM-V1-PHYSICAL-QUALIFICATION.md).
Next boundary: separately authorized corrective telemetry work and warning
review, followed by Session A with the exact resulting installed identity.
No production code/build/policy changes, flash, long unattended run or Session
E/F/G operation is authorized by this entry; no capability is promoted.

## Telemetry repair — 2026-09-25

The owner authorizes the [focused repair](roadmap-gap-audit.md#platform-v1-telemetry-repair-entry--2026-09-25):
health CLI dispatch, bounded wakeup-counter observation, targeted regressions,
warning review and a preserving root-only candidate. Flashing remains prohibited.
Reborn, kernel, hardware policy and all physical acceptance gates remain unchanged.
Next handoff requires host/ARM userspace and root-image preservation receipts;
the installed candidate's failed qualification record remains historical evidence.

The [candidate handoff audit](roadmap-gap-audit.md#platform-v1-telemetry-candidate-handoff--2026-09-25)
now records 67 host and 5 packaged ARM/QEMU passes, exact five-file image changes,
filesystem/scatter/inode preservation and the original UI fallback. Source
`814c2f3`, release `1.0.0-candidate.2`, is packaged at
`out/y2linux-platform-v1-telemetry-01-candidate/`. The
[warning review and repair report](../validation/PLATFORM-V1-TELEMETRY-01.md)
retains unresolved timer/cache/hardening and physical gates. Nothing was flashed;
the installed image and its qualification status remain unchanged. Next is the
separately authorized installation/Session A boundary, with stop rules intact.

The owner subsequently reports installing Telemetry 01. The
[physical re-entry audit](roadmap-gap-audit.md#telemetry-01-physical-re-entry--2026-09-25)
resumes identity/baseline verification under the original test limits. Software
receipts do not pre-approve the installed image or exempt existing warnings.

Fresh [physical baseline evidence](roadmap-gap-audit.md#telemetry-01-physical-baseline-stop--2026-09-25)
now verifies exact Telemetry 01 metadata/nine file hashes and both telemetry
repairs on boot `3194fa9d-2dee-4105-86f0-4021580bf8d0`. Status/health return and no
wakeup reader remains. Root/data counters are clean and the owner sees the main
menu with normal warmth. The retained kernel warning rule stops Session A;
Wi-Fi health is FAILED/DEGRADED, so **A FAIL; B–D NOT_TESTED** remains the current
session result. No platform physical/endurance acceptance; next resolve warning
and Wi-Fi readiness gates without assuming new hardware or source authorization.

Read-only follow-up confirms no configured Wi-Fi networks, a responsive
supplicant, and the existing 100 Hz periodic broadcast timer configuration.
Intermittent Wi-Fi query delay remains unresolved. These findings refine the
warning/readiness diagnosis without authorizing a build or changing acceptance.

The owner subsequently connected to their router and requests the
[narrow connection check](roadmap-gap-audit.md#owner-requested-wi-fi-connection-check--2026-09-25).
Association/IP/route/DNS and bounded router reachability may be observed without
advancing the full qualification sessions or changing the network configuration.

That check now observes WPA2 association, a DHCP address/default route, 5/5
router and 3/3 Internet-IP ping replies. DNS queries for two hostnames fail;
the platform observer also retains intermittent supplicant_unavailable. Wi-Fi
coverage is PARTIAL with DNS FAIL, not Online or full Session C acceptance.
Next diagnose DNS/readiness; no network settings or production code were changed.
