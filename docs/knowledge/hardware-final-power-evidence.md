# Hardware Final power evidence and remaining physical gates

2026-09-27. This source review accompanies the coordinated campaign; it does
not close power qualification. Private current-device receipts are under
`out/hardware-final/20260927T122702Z-hardware02/`. Historical contradictory stock
battery service output is not used as calibration.

## CPU, timers and idle

The stock-bin frequency/voltage investigation and guarded implementation are
documented in [the DVFS admission](hardware-final-dvfs.md). New 1196/1300-MHz
OPPs are physical-pending. Default qualification ceiling remains 1040 MHz;
all four cores remain the product policy until power benefit and mixed-workload
hotplug stability are measured. CPU0 is the boot CPU and cannot be offlined.

Receipt `12-timer-continuity` observes `arch_sys_timer` on all four CPUs and
PPI29 counts on all four. Each CPU performs 100 requested 1-ms sleeps: median
durations are 1.110188, 1.099996, 1.113497 and 1.102803 ms respectively. The
bounded interval spans 340.482819 seconds of monotonic time, 340.482477 seconds
of raw monotonic time and 340 whole RTC seconds. It crosses a 13-MHz counter's
32-bit low-word rollover. This establishes useful sub-tick wakeups and internal
continuity; monotonic versus raw time is not an independent frequency reference
and does not certify long-term clock drift or tickless residency.

The userspace power coordinator previously woke four times per second even
though it sampled at 1 Hz. It now waits for the next observation or shutdown
deadline; socket requests and acknowledgements still wake immediately.
Deadline and failed-shutdown regression tests pass. Physical wakeup and power
benefit must be measured on the final userspace build.

[Idle source reconciliation](hardware-final-idle-evidence.md) distinguishes
stock slow bus-clock idle from context-losing deep idle. The latter still needs
runtime-specific PCM, CIRQ, GPT4 deadline handoff and context/clock ownership.
The system-suspend PCM must not substitute for that runtime contract.

## Suspend and RTC

The exact BSP explicitly says its suspend PCM includes the normal runtime
loop after successful wake. Therefore retaining that PCM on successful resume
is not by itself a source bug. Current code restores the separate normal PCM
on aborted entry. Historical USB overflow and WMT restore timeout occurred
before any proven SPM entry; they do not establish a defective deep PCM.
New staged receipts must distinguish freezer/device/processor/core progress,
SPM entry count, real `cpu_resume`, wake source and same boot ID. Returning
from a `pm_test` stage does not qualify deep wake. The helper now preserves a
radio restoration failure instead of returning success.

Receipt `05-rtc-ntp-sync` enables the existing qualified write policy and
records a successful NTP-to-RTC update. RTC reads then match consecutive
two-second system-time intervals. Reboot retention, full power-off retention
and same-boot alarm wake are separate outstanding physical tests.

## Charger, battery and truthful telemetry

The exact stock charger contract is already reconstructed in
[M4 charging](m4-charging.md) and its later
[end-user power corrections](m4-end-user-power.md). Current source retains
BC1.1 source classification, 70/450/650-mA bounded selectors, 4.175-V CV,
watchdog, fault latching, termination/recharge and active-charge suspend refusal.
Configured current and source allowance are never net battery current.

Receipt `04-load-charge-observation` initially observes 3540454 µV and later
3574291 µV over 1121.57 seconds, while status reports Charging and the USB-host
selector is 450000 µA. Workloads changed during this interval. A positive
voltage trend is useful evidence, but cannot quantify current into the pack,
prove load compensation or calibrate usable capacity. The same receipt reaches
67500 and 60333 m°C in its two die thermal zones, above the earlier 60°C test
stop envelope. Those values are not pack temperature; higher-clock qualification
requires a cool baseline and a bounded thermal stop.

| Requested measurement/policy | Exact established evidence | Remaining requirement |
| --- | --- | --- |
| Hardware fuel gauge/coulomb count | Exact stock `fgauge_read_current` at `0xc04caa04` and `fgauge_read_columb` at `0xc04caa2c` return zero without filling a measurement; the current MT6323 driver has no proven FG register path | A documented, wired measuring path; the stubs cannot certify one |
| Battery current | Real ISENSE/BATSNS ADC paths exist. Stock computes their difference using an assumed 68-milliohm resistor | Board resistor value/topology and offset calibration; prove whether the resistor carries total battery current or only charger-path current |
| Pack temperature | BATON/TDET are present; 120 prior samples around raw 10387–10388 show acquisition, while normal closed-case handling showed no resolved response. Exact stock public temperature functions return constant25 | Trace the pack NTC/resistor network or demonstrate a repeatable response at independently measured equilibrated temperatures; a fixed reading does not prove no thermistor |
| SOC/percentage | Stock uses software voltage/resistance tables; these are not this pack's calibrated load/rest curve | Controlled charge, rest and discharge observations, pack bounds and load-sag/hysteresis calibration; no fabricated percentage or 1% precision |
| Low-battery thresholds | Existing platform mechanism implements hysteresis/debounce, app checkpoint acknowledgement and bounded clean shutdown | Measured warning/reserve/shutdown/recovery thresholds, then attended physical low-battery closure; do not infer safe deep-discharge bounds from boot voltage |
| Charging balance/full/recharge | Real PMIC voltage/configuration/fault observations and bounded source policy exist | External USB voltage/current alongside controlled loads, long trend, completion/recharge and connector cycles; USB input current remains distinct from battery current |

These telemetry gaps are `BLOCKED_BY_HARDWARE_EVIDENCE`, not unsupported claims
about unseen board wiring. The installed ADC channels can collect the missing
curves and thermistor evidence without another kernel flash. No pack temperature,
net current, SOC or shutdown thresholds were invented by this source pass.
