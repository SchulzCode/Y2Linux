# M2 integrated baseline — physical results, 2026-09-10

M1 COMPLETE; M2 ACTIVE. Owner-tested **M2-BASELINE-01**, source checkpoint
`7c34d35`, reports LK logo only and host USB disconnect after about five minutes.
The compiled identity is confirmed in both captures; the owner did not separately
report the flashed file hash or Android restoration. No flash was performed by
the development agent.

## Capture integrity and limits

[Machine-readable evidence](../build/results/m2-baseline-01-captures.json) records
hashes, contiguous byte chunks and key counts. Original directories remain under
`out/m2-baseline-01/`; copies are retained in ignored
`evidence-private/20260910-baseline-hardware/`.

| Capture | Bytes / SHA-256 | Kernel sequence / PID1 |
| --- | --- | --- |
| capture-01 | 78,903 / `502006206f4dd109d99fc7a2293f45b44be961f54cb3a858f475009e7264626c` | All 0–574; heartbeat 71 |
| capture-02 | 79,568 / `4bfb2e04a121513f9886e2d2468063166825d3d7e05965464907b0ee41b3b7ad` | All 0–575; heartbeat 68 |

Both commands used the old **45-second default**, not the documented 180 seconds.
`status: timeout`, exit0 means the capture duration elapsed with the expected
LOG1 header, not a kernel timeout. No relay GAP/ERR/input-error markers or kernel
sequence gaps occur. These retain the complete initial kernel records plus the
captured runtime, **not five minutes of runtime evidence**. Different early kernel
timestamps and fresh USB lifecycles show separate boots, not a reconnect test.

## What this establishes on this unit

| Subsystem | Observation / interpretation |
| --- | --- |
| SMP/GPT | All four CPUs online; timer broadcasts and cross-CPU IPIs advance, IRQ error count zero. Release and basic scheduling work; stress/coherency/PM qualification remains open. |
| RAM | D08 retained; MemTotal 19,708kB with the larger kernel. No captured OOM/oops/panic. No expanded-RAM or sustained working-set claim. |
| CCF/pinctrl/PWRAP | Providers probe; inherited AXI reports 273MHz. PWRAP CID2023, upstream MFD/regulators and backlight probe. No full clock-rate/rail-policy qualification. |
| Navigation | capture-02 has three press/release pairs each for codes 105,158,106,164,28. Five navigation channels work through GPIO/pinctrl/evdev. Exact physical label order was not separately recorded. |
| Volume and Power | Volume−114: six pairs; Volume+115: four pairs; Power116: four pairs. Keypad has 20 IRQs; PMIC EINT25 and nested key IRQ each have eight. This confirms the canonical PWRAP/MFD/EINT25 power-key path. |
| Wheel/I2C | Wheel device registers, but no scroll events, EINT55 IRQs or I2C0 transfers occur. Rotation attempt not explicitly reported. Registration alone proves neither controller response nor scroll function. I2C1 has no clients. |
| Display | Permanent DSI probe failure `-EBUSY`: **unrecognized live DSI PLL** (record505). BASELINE-01 wrongly assumed LK used the planned Linux PCW/dividers. It refuses before parking the running stream, consistent with the unchanged LK logo. Actual inherited PLL words were not logged; panel identity/timings remain untested. |
| Backlight | Inherited four-channel ISINK PWM, duty32/32 and current step5 accepted. Illumination persists; brightness transitions and Linux panel operation untested. |
| microSD | Native MSDC identifies an SDXC card; CID/CSD recorded, <=400kHz/one-bit policy. No MMC block driver or disk mount/write path. No throughput/filesystem qualification. |
| eMMC | CMD2/CMD9 identity responses obtained. CMD6 intentionally rejected by write policy; `-EROFS`/-30 and final initialization failure are expected, not evidence of bad pins/clock. |
| USB | Two high-speed ACM boots retain complete initial kernel logs under shared providers. No captured runtime USB errors. One-detach/reconnect and end-of-window behavior remain unqualified. |
| Power status | Regulator selectors are readable; early unplugged power_supply ONLINE=0 agrees with boot sequencing. No charging, battery voltage, thermal or runtime USB-presence transition qualification. |

58 key events in capture-02 are 29 balanced press/release pairs; none come from
the scroll device. Remaining low-priority messages: absent disabled RTC/LED/PWRC
DT children, unimplemented cache hierarchy description, deliberately disabled
kernel memory protection, unsupported VCAMA/VRTC `get_mode` during the read-only
regulator summary. No rail write failure is inferred from those mode warnings.

## Next combined iteration

BASELINE-02 separates inherited PHY power adoption from the Linux cold PLL
configuration. It reads/logs the safe PHY status words and validates power/LDOs,
leaves a live PLL untouched while DSI parks, then retires inherited power without
inventing CCF references. Normal modesetting subsequently owns the cold 338MHz
configuration. No PLL_TOP read, guessed live frequency or live PLL retune.
Incomplete power still fails safely. Add fault tests for the actual PHY callbacks.

Host capture defaults to 180 seconds for either baseline identifier, preserving
45 seconds and the earlier 60-second ceiling for older experiments. The USB
295-second disconnect and owner **300-second maximum from power-on** stay intact.
The reported five-minute disconnect matches that programmed cutoff, but neither
45-second capture observes the cutoff itself. USB detach does not power off the
Y2; end the run using the established owner power/recovery procedure. No automatic
shutdown, charging experiment, memory expansion or milestone boundary change.
