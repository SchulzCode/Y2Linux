# Y2 CPU idle completion physical qualification

## Candidate01 hardware qualification — 2026-10-03

**C1 WORKING; C2 WORKING; C3 BLOCKED BEFORE PHYSICAL ENTRY.** The owner installed
the sealed images and authorized this hardware run. The automated overall
qualification **FAILS** because C3 cannot pass its real clock prerequisites.
Independent regressions completed; no reset, corruption or forced entry occurred.
Earlier baseline results below are historical and do not substitute for this run.

Private evidence: `out/cpu-idle-completion-physical/20261003T130848Z-candidate/`.
Admission, source/boot-tagged SSH command/stdout/stderr/exit receipts, durable
qualification job and result, post-run state and summary are retained. Installed
identity matches every candidate axis: Linux
`3dfb5f5cc731ec5dac88a778290819c23dba6067`, Reborn
`b71b468860233faa0a42b8448ec5777fa952b8e3`, kernel
`6.18.0-y2linux-cpu-idle-01`, root `2025.02.18-platform-v1.18`, release
`1.0.0-cpu-idle-completion-candidate.1`, build
`Y2LINUX-CPU-IDLE-COMPLETION-01`. Boot
`e0eb2f36-7c1f-4007-99e2-81d2eae047d4` stays unchanged; taint0 throughout.

### C1, parking and C2

All four WFI states are present and enabled. During the bounded qualification:

| CPU | Additional entries | Residency, us | Rejections | Median / p95 wake, ms |
| --- | ---: | ---: | ---: | --- |
| 0 | 1271 | 11972491 | 0 | 1.083999 / 1.095614 |
| 1 | 1050 | 12207881 | 0 | 1.084691 / 1.110306 |
| 2 | 990 | 12337826 | 0 | 1.084306 / 1.093922 |
| 3 | 612 | 12244397 | 0 | 1.084306 / 1.091383 |

Maximum sampled1ms wake1.706998ms; MONOTONIC/RAW drift-9575ns in12.068182s.
Three supported hotplug cycles pass all18 transitions and both physical power
copies, CPU3→2→1 off and1→2→3 on. Automatic parking subsequently honors its
pressure hold: first CPU3 parked at252s, CPU0/owner0xe at262s. Ten-second observer
sampling does not individually time CPU2/CPU1; supported hotplug/source tests
prove their exact physical bits and one-core policy. Screen wake and real
PlaybackNormal demand restore all four cores without ownership errors.

Screen off and radios runtime-off make C2 naturally eligible. SLIDLE adds
**7,040 entries and49,674,049us in60.047964546s (82.72%)**, with WFI4.061876s.
SLIDLE failures0, bus rejections0, clock restore failures0, final blocker0 in
this window. Active APDMA/BTIF are legitimate blockers when Wi-Fi returns;
MMC request blockers appear during real I/O and disappear afterward.

| Window | Seconds | CPU utilization | CPUs | CPU0 WFI, s | CPU0 C2, s | IRQ/s | Context switches/s |
| --- | ---: | ---: | --- | ---: | ---: | ---: | ---: |
| Screen on idle | 20.076549 | 3.58% | 0–3 | 18.649760 | 0 | 452.82 | 529.22 |
| Screen off before parking | 20.070139 | 3.41% | 0–3 | 19.450031 | 0 | 351.67 | 403.19 |
| Screen off, parked, C2 enabled | 60.047965 | 9.47% | 0 | 4.061876 | 49.674049 | 187.72 | 343.06 |

C2-window MONOTONIC/RAW drift218003ns. CPU utilization is aggregated over online
cores; this is not an electrical comparison. Temperatures stay below the
75°C test guard (observed CPU about38–44.7°C). No measured battery-life claim.

### C3 preflight failure and exact remaining owners

Normal controls stop playback, turn radios runtime-off, detach USB through its
role owner, cap747.5MHz, use automatic CPU0-only topology and arm the existing
10s RGU backstop with budget1. Preflight then reports:

- Power copies0x314c/0x314c; secondary mask0, blocked domains0.
- PERI blockers**0x02020000 = UART1 bit17 + SPI0 bit25**.
- INFRA blockers0, DISP0/1 blockers0, bus0.
- Every other exposed static prerequisite category passes; only clocks fails.
- Runtime PCM480words, originMT6582_d53dd75c_dpidle; CIRQ ready and inactive.
- Dynamic architectural/GPT4 deadline and actual context/vector programming
  were not exercised: no actual dormant call was admitted.

Boot warnings identify both as retained active/unknown loader engines. CCF
summary shows zero enable/prepare references and no consumer for both. Only
UART0 at0x11002000 is Linux's serial owner; UART1's sampled window is0x11003000,
SPI0's0x1100a000. Installed diagnostics do not expose the boot-time raw operands,
so the exact UART busy field and observed SPI status value remain **unknown**.
No userspace MMIO, register forcing, driver reset, guard bypass or fake counter.

SPM dormant attempts/entries/resumes/successes/residency remain0. CIRQ entries/
flushes and local timer context saves/restores remain0; no error in those counters
is proof of dormant restoration. Near-boundary and20-cycle trials correctly do
not run after failed preflight. C3 remains disabled, budget0, backstop disarmed.
This is a software/clock ownership gap, **not evidence of a hardware limit**.

Source follow-up corrects the SPI predicate that treats the vendor's nonzero
idle indication as busy, distinguishes UART timeout metadata from real DMA,
retains error/unknown/aliased UART states, and adds boot-time read-only operands
and reason names. See [the source record](Y2-CPU-IDLE-COMPLETION.md). A follow-up
candidate must physically prove these decisions before any C3 success claim.

### Independent regressions and restored state

Both eMMC and inserted SD pass8 fsynced, uncached128KiB SHA256 rounds in each
storage phase (16 per medium total). Ext4 errors remain0 for current root/data
`mmcblk0p7`/`mmcblk0p5` and SD`mmcblk1p1`; driver errors/mismatches0. Runtime
suspend/resume/retained-context counts advance and clocks gate afterward. SD
absent is not observed. No mounted media reset or calibration access.

Both OPP sweeps reach598/747.5/1040MHz at1.15V,1196MHz at1.20V,1300MHz at1.25V,
with original schedutil598000–1300000 policy restored. The second sweep follows
C3 **preflight**, not dormant wake. GPT6 free-running13MHz, GPT4 sole broadcast,
architectural PPI29/highres/NO_HZ and all four CNTFRQ readbacks remain intact;
per-core timer errors0 and bounded continuity pass. Actual dormant timer handoff
and CIRQ/GIC/MMU/VFP/cache restoration remain unobserved.

The real Reborn/FFmpeg/ALSA silent44.1kHz stereo test adds356,352 decoded frames,
with decode/filter/playback errors0 and xruns0. Screen restore/all-core demand
wake pass. Three1MiB USB bidirectional transfers match SHA256 on host/device;
the separately pinned Wi-Fi observer responds on the same boot. Automatic
USB→Wi-Fi fallback was not required during sparse polls; both may intentionally
be unavailable while legitimate radio-off/USB detach controls run locally.

Finally screen off/Wi-Fi online/Bluetooth off/coordinator enabled/original DVFS
and USB role are restored. Temporary test files and owned Wi-Fi listener are
removed. The initial paused selection referred to an already deleted historical
qualification fixture (track414), so it cannot safely be resumed; playback is
left stopped. No new filesystem/IRQ fault, same boot, taint0. Full-system suspend
is excluded because its historic resume failure remains a separate gate.
No flash or push occurred.

## Historical installed baseline qualification

The previous installed baseline was qualified on 2026-10-03. Private raw evidence:
`out/cpu-idle-completion-physical/20261003T074026Z/`. Every SSH receipt includes
host UTC time, exact command, output, exit code, compiled source identities and
boot identity. The automated candidate qualification is a separate run after
owner flash; no candidate hardware result is claimed here.

Installed Linux `131ee4621cd583c955182f994adac5a594fb823f`, Reborn
`7f9df397ab3809d52e2f1073ca93a305246e0c3a`, kernel
`6.18.0-y2linux-baseline-01`, root `2025.02.18-platform-v1.17`, release
`1.0.0-baseline-candidate.1`, boot `cee326c1-a5b0-447a-8eb0-dc3f39f7e2c2`,
taint0. Runtime identity matches the sealed baseline BOOTIMG/Y2ROOT metadata,
rather than being inferred from repository HEAD. The authorized USB SSH path
was used, with a same-boot Wi-Fi observer verified before radio-off testing.

## C1: WORKING

Receipt `04-c1-all-cores`: all four CPUs online through the supported coordinator
control, 100 one-millisecond sleeps pinned to each CPU, then a bounded 12-second
idle window. Original coordinator policy was restored in finally.

| CPU | New WFI entries | New residency, us | New rejections | Median / p95 wake, ms |
| --- | ---: | ---: | ---: | --- |
| 0 | 1123 | 12028655 | 0 | 1.071535 / 1.290226 |
| 1 | 916 | 12137106 | 0 | 1.082920 / 1.096997 |
| 2 | 649 | 12236436 | 0 | 1.087458 / 1.091458 |
| 3 | 1214 | 11744082 | 0 | 1.087227 / 1.095612 |

MONOTONIC versus MONOTONIC_RAW difference changed -39154 ns. Same boot, taint0,
all cores returned. C1 architecture is frozen.

## C2: WORKING

Receipts `07`/`08` first establish legitimate APDMA/BTIF blockers with Wi-Fi on,
and no such blockers with radios off. An initial 60-second observation did not
produce C2 while the coordinator was honoring a four-minute pressure hold.
That is a policy hold, not evidence of broken MMC clock gating.

Receipts `09`/`10` allow the existing policy to settle without bypassing its
hysteresis. A radio restore/start-up burst escalated the hold to 480000 ms.
Thirty-second observations show CPU3 parked at `settle-16` and CPU2/CPU1 parked
by `settle-17`. Final owner mask0xe, CPU0 only, quiet139040 ms, sustained load129
millicores and high-frequency residency148 per mille. No manual offline was
used to make this C2 window eligible.

During the 60.404075502-second CPU0-only, screen-off/radios-off window:

- SLIDLE entries2657 ->9839: **+7182**.
- SLIDLE residency18394631 ->69158372 us: **+50763741 us**, or50.763741 seconds,
  approximately84.0% of wall time.
- CPU0 WFI entries+1584, residency+3901404 us; DORMANT entries0.
- SLIDLE failures0, clock restore failures0, brokenN, bus rejection0.
- SLIDLE clock blockers0; both MMC controllers gated before and after.
- CPU utilization8.5795%, IRQ168.250/s, context switches338.785/s.
- MONOTONIC versus MONOTONIC_RAW difference changed -64809 ns.

The clock transaction restores the exact inherited bus configuration; no bit
was removed from a blocker mask. The source change from Fix03 is present in the
baseline and physically resolves the historical reset-default bus rejection.
Preserve it rather than redesigning working C2 or storage.

## Storage after C2

Both eMMC and inserted SD were mounted and usable. In each of the two radio-off
runs, eight fsynced, uncached SHA256 readback cycles per medium passed. Temporary
random test files were removed. `mmcblk1p7`, `mmcblk1p5`, `mmcblk0p1` ext4 error
counts remained0. Runtime suspend/resume and retained-context counters advanced
on both controllers, with mismatches0/errors0 and both clocks gated afterward.
Request refusal counts increased during real I/O, as required; DMA/controller/
FIFO/IRQ busy refusals remained0. No reset of mounted media was attempted.
SD-absent operation is **not observed**: physical card removal requires the
owner. The post-flash harness records whichever media are actually present.

## Hotplug, DVFS and timers

Receipt `11-hotplug-opps`: three complete supported hotplug cycles, offline
CPU3 ->CPU2 ->CPU1 and restore CPU1 ->CPU2 ->CPU3, 18 transitions total. SPM power
copies were captured at each step, proving physical secondary power-off rather
than only scheduler exclusion. All CPUs then passed pinned one-millisecond
sleeps (median about1.084 ms, maximum p95 about1.111 ms).

All five guarded OPPs were actually reached under bounded min=max policy:
598000,747500,1040000 kHz at1150000 uV;1196000 at1200000 uV;1300000 at1250000 uV.
Voltage faultN, PWRAP request/readback error0. Original limits, schedutil and
coordinator policy were restored. CPU temperature44–45.1 C, PMIC42.204–42.789 C.
Same boot, ext4 counts0.

GPT6 free-running13 MHz, GPT4 broadcast, architectural physical timers/PPI29,
high-resolution timers and NO_HZ are observed in initial raw state and bounded
continuity receipts. No timer foundation was replaced.

## C3: SOFTWARE_READY_NEEDS_NEW_FLASH

The installed baseline has C3 registered but default disabled and never
physically entered. It was not forced past current live prerequisites. Live
preflight identifies frequency, powered radios, active/inherited display and
peripheral clocks; the old unconditional DISP-power predicate additionally
contradicts the exact MT6582 vendor clock-based condition. Screen-off currently
blanks the backlight but keeps DRM scanout running.

Source fixes, candidate artifacts and the next qualification command are
recorded in [the implementation record](Y2-CPU-IDLE-COMPLETION.md). This label
is the software handoff category, not a claim of a successful DORMANT entry.
Receipt12 additionally proves a USB0 clock ownership leak: normal role none
changes MUSB active ->suspended, but PERI bit10 stays active; restoring device
returns MUSB active on the same boot. No unsafe MMIO interface was used.

The new candidate must verify one bounded real CPU reset/resume, then at least
20 bounded cycles with unchanged boot identity, timer/IRQ/context continuity,
restore counters, storage and interactive workload regressions. A failed first
attempt quarantines C3; it is not automatically hammered again.

No electrical energy measurement was made. Residency is demonstrated; battery
life improvement is not claimed. No flash or push occurred.

## Playback and demand wake

Receipt16 creates, scans and plays a temporary silent44.1-kHz stereo WAV through
the actual Reborn/FFmpeg/ALSA pipeline. Eight seconds add352256 decoded frames;
xruns, recoveries and playback errors add0. All cores online, a real
PlaybackNormal lease active. Playback stopped and the fixture removed/rescanned.
Receipt14 found no old retained fixtures and is explicitly not a playback pass.

Receipts17/18: normal Reborn screen-on/off transitions return on the same boot;
three bidirectional1MiB USB SHA256 rounds pass with independent Wi-Fi SSH ready.
These awake results do not qualify the candidate's new bus-clock gate yet.

## Controlled residency comparison

Receipt21 completes a device-side comparison without host polling during each
window; initial screen/radio/C2 policy restored in finally, same boot, taint0.
No C3 control was changed. Frequency residency and temperatures are retained
alongside each raw before/after snapshot.

| Condition | Wall seconds | CPU utilization | Online CPUs | CPU0 WFI, s | CPU0 SLIDLE entries / seconds | IRQ/s | Context switches/s |
| --- | ---: | ---: | --- | ---: | --- | ---: | ---: |
| Screen on idle | 20.083 | 4.25% | 0–3 | 19.084 | 0 / 0 | 444.75 | 518.34 |
| Screen off before parking | 20.070 | 3.08% | 0–3 | 19.500 | 0 / 0 | 315.69 | 397.46 |
| Parked, C2 enabled | 30.062 | 9.39% | 0 | 1.942 | 3448 / 25.022727 | 168.42 | 337.24 |
| Parked, C2 disabled | 30.048 | 8.80% | 0 | 27.109 | 0 / 0 | 166.97 | 335.10 |
| Parked, C2 re-enabled | 30.048 | 9.23% | 0 | 1.964 | 3437 / 25.015727 | 167.96 | 338.02 |

SLIDLE occupies83.2% and83.3% of the enabled windows; disabling it shifts
residency to WFI and re-enabling restores real deeper residency. CPU utilization
is aggregated across the online cores, so the four-core and one-core percentages
are not an energy comparison. The raw OPP time-in-state deltas show the unchanged
schedutil policy, including short active high-frequency bursts. No new
cpuidle rejection appears. MONOTONIC/RAW drift is between-74211 and-124163ns.
CPU/PMIC temperatures are roughly39.9–45.1°C; conditions include external USB
power and radio state changes. No battery-life or electrical-power claim follows.

Receipt22 final status/health/dmesg confirms the original installed identity,
USB device role, schedutil598000–1300000kHz, C2 enabled, C3 disabled, same boot,
taint0 and all three mounted ext4 error counts0. Normal Wi-Fi is online; the
original screen is off and policy owns parking. Receipt23 stops only this pass's
verified temporary Dropbear listener; ordinary USB SSH remains available.

The exact physical hotplug power mapping in receipt11 is CPU1=0x800,
CPU2=0x400, CPU3=0x200 in both status copies. CPU3-off produces0x3d4e,
then CPU2-off0x394e, then CPU1-off0x314e; restoration reverses these values.
The kernel already used this descending MT6582 mapping. The new harness now
checks these exact operands and has a regression fixture for a mismatched
second status copy. An earlier unsealed package was rejected before handoff
while that harness correction was incorporated; no device flash occurred.

Receipt25 runs the final candidate harness against this still-installed baseline.
It rejects mismatched kernel, Linux, Reborn, rootfs, release and build identities
before any device mutation; only two read-only SSH operations occur. Receipt26
reconfirms same boot/taint0, C2 enabled, C3 disabled, normal owner policies and
mounted ext4 error counts0 at handoff.
