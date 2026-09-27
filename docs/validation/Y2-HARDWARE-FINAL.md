# Y2 Hardware Final

**Paused at owner request,2026-09-27.** The [handoff](Y2-HARDWARE-FINAL-HANDOFF.md)
supersedes provisional progress below and records restoration, completed fixes,
owner-confirmed volume repair and all outstanding validation. No final image.

ACTIVE engineering campaign, 2026-09-27. One final candidate is authorized;
Hardware 03/04/05 are consolidated. No final candidate or installation is yet
claimed. [Entry audit](../planning/roadmap-gap-audit.md#hardware-final-campaign-activation--2026-09-27).

Installed baseline: Hardware 02, Linux `76bc8229580ec8d101008c5bad47419f2c110eae`,
Reborn `95747e0a36c7b27beb44b8cdd54feda1813f2f3a`, candidate.4, kernel
`6.18.0-y2linux-hardware-02`, boot `b93cda16-4e08-4ead-a74a-c85f4cccaa11`.
Receipt00 matches exact metadata, pinned SSH, taint0, all ext4 counters0 and
quick health OK. Source entry Linux `c53963b`; both repositories use local
`hardware-final` branches. Pre-existing documentation/assets are preserved.

Private raw commands/results are under
`out/hardware-final/20260927T122702Z-hardware02/`; these include device identifiers
and must not be copied wholesale into public docs or the candidate. Numerical
summaries and receipt digests belong in the public evidence ledger.

## Physical evidence so far

- MMC IOS8/4-bit, high-speed SDR,3.3V and actual24,999,971Hz at entry. Requested
  clock50MHz is distinct from actual clock. Bounded13/25/50 steps are running
  with guarded scratch, direct random/sequential readback and durable workloads.
- GPT6 calibrates13MHz; all four CPUs have arch_sys_timer and arch_sys_counter
  is active. timer_list reports hres_active/highres/nohz1 and idle_sleeps on
  every CPU. Long continuity, effective idle and hotplug tests remain explicit.
- USB reports Inventra DMA, zero DMA errors at entry. Directional counters
  are added to final source; programmed bytes are not successful delivered bytes.
- RTC standard UTC write/read/ticking pass again after checked NTP. Existing
  RTC synchronization policy is enabled with receipt05; reboot/off retention
  and same-boot alarm wake are separate unperformed cases.
- AirPods Pro2 bond reconnects with actual SBC stereo44.1kHz. Owner confirms
  clean sound in both ears and working stem Play/Pause during receipt11.
  Volume keys cause a roughly one-second restarting-audio interruption; this
  is an observed product bug, not an SBC acceptance waiver.
- Idle thread observation found control~98, audio~99, Bluetooth~51, main~65
  voluntary context switches/s. Source fixes remove empty-control100Hz polling,
  idle-audio100Hz polling and power-daemon4Hz polling. Physical A/B still pending.
- Owner has no USB power meter. No input-current, net-battery-current or charge
  energy claim is made. Pack thermistor/current calibration/SOC gates remain.

## Consolidated implementation and boundaries

[Capabilities](Y2-HARDWARE-FINAL-CAPABILITIES.md) distinguish narrow prior physical
results from final-source availability and blockers. [Qualification](Y2-HARDWARE-FINAL-QUALIFICATION.md)
records the outstanding execution order. Higher stock OPPs require the guarded
[DVFS contract](../knowledge/hardware-final-dvfs.md), default1040MHz cap, runtime
opt-in and physical qualification. No overclock or automatic high-voltage default.
Codec libraries are owner-private source/build experiments with explicit normal
SBC fallback and no unqualified Auto preference. Public distribution is separate.

No intermediate firmware is justified: temporary userspace experiments can run
on Hardware02; new kernel paths can be gated in the one final candidate. Deep
suspend, VBUS, pack calibration and wider DL1 are not fabricated from build flags.
Protected regions, loaders and Y2DATA remain unchanged. Actual root OTA apply
requires explicit owner approval after a concrete package and stable core.
