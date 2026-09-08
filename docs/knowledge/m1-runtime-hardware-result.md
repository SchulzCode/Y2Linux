# M1 — Continued native PID1 execution on the physical Y2

Recorded 2026-09-08 from the owner's explicit report of today's real Y2B-245
diagnostic hardware test. This is owner-observed hardware evidence, not a host
fixture, synthetic preview or a device observation made during this documentation
session.

## Confirmed in the owner report

- Linux version displayed: `6.18.0-y2-m1`.
- Native initramfs PID1 running.
- BEAT increases; sleep works; uptime increases.
- `/proc` and `/sys` mounted.
- Framebuffer diagnostics work; watchdog reports stopped.
- CPU0 online; static D08 RAM visible.

**M1's core objective is achieved: Linux 6.18, initramfs and native PID1 are
running on the physical Y2, with observed periodic execution.** The earlier
sleep -38 report remains historical evidence of a separate failed iteration,
not the current result. This observation is consistent with the time32 fix in
Y2B-250 #21; the owner calls the successful test Y2B-245. Do not silently relabel
the flash or assert an exact binary identity from the version string.

## Scope and diagnostic oddities kept separate

The report does not supply the flashed BOOTIMG hash, numeric counters, full
screen text/photo, FRAME progression, timer IRQ count/rate, precise MemTotal,
elapsed duration, terminal 50-beat STOP stage, reset history or this trial's
restore outcome. These are unreported, not failed checks. No new oddity beyond
the historical ENOSYS failure is invented; any other diagnostic discrepancy
needs its exact screen field/value before diagnosis.

The retained Y2B-250 offline candidate is 1,093,632 bytes, SHA-256
`074c3e6946e6e8ac61bc1df0da4fa53807071dd50ae3db4def97563c9207afce`.
That is a candidate identity, **not an independently verified identity of today's
flashed bytes**. The previous linked-ELF proof of the time32 fix remains in
[y2b250-result.md](../build/y2b250-result.md).

Working sleep/uptime establishes timer-backed progress as observed, not timing
accuracy or a stress/stability certification. D08 visibility establishes this
map boots, not safety of the rest of the 1 GiB or all inherited DMA. The watchdog
status concerns the reviewed AP_RGU check, not all potential reset sources.
Inherited RGB565 display output is not native panel/DRM/backlight support.
Physical UART remains unverified and is not required to acknowledge this result.

## Session boundary and next work

Only evidence and planning were updated today. No device access, implementation,
Linux build, BOOTIMG creation or flash was performed in this session. No M2
execution has begun. [Exactly five queued foundation research issues](../planning/next-five-platform-foundations.md)
prepare later implementation one evidence boundary at a time; the native player,
audio, networking and persistent rootfs remain outside this wave.
