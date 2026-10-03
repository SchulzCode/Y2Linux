# Y2 CPU idle completion physical qualification

Current installed baseline was qualified on 2026-10-03. Private raw evidence:
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
