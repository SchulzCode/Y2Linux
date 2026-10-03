# Y2Linux

Linux 6.18 and Buildroot platform for the Innioasis Y2, paired with the native
[Y2Reborn player](https://github.com/SchulzCode/Y2Reborn).

**Baseline02 is the newest sealed integrated candidate.** It includes automatic,
guarded CPU0 C3 activation after boot. **C1 WFI, C2 SLIDLE and C3 DORMANT have
physically passed on the preceding UART01 image**; qualification of Baseline02's
new images and automatic cold boot is pending.

| Field | Sealed Baseline02 |
| --- | --- |
| Build | `Y2LINUX-BASELINE-02` |
| Candidate | `out/y2linux-baseline-02-candidate/` |
| Kernel | `6.18.0-y2linux-baseline-02` |
| Rootfs / release | `2025.02.18-platform-v1.21` / `1.0.0-baseline-candidate.2` |
| Compiled Linux | `8584ccd85f052f351fe51650b2348c83ca894ed4` |
| Compiled Reborn | `b71b468860233faa0a42b8448ec5777fa952b8e3` |
| ABI / feature contract / layout / data | 1 / 2 / 1 / 1 |
| Software / package | PASS |
| New-image hardware / automatic cold boot | PHYSICAL_NOT_RUN |

Start with the [current platform state](docs/CURRENT_PLATFORM_STATE.md),
[Baseline02 receipt](docs/validation/Y2-BASELINE-02.md),
[UART01 hardware qualification](docs/validation/Y2-CPU-C3-UART-PHYSICAL.md)
and [documentation index](docs/README.md).

The UART01 run observed C2 at **84.35% residency** with zero clock restore
failures, 21 bounded C3 reset-and-return trials, and **3,624 C3 entries / 65.25%
residency** in a 60-second normal-policy window. The final same-boot count was
3,647 real C3 returns. Timers, hotplug, all five 598–1300 MHz OPPs, bounded
storage integrity, playback and USB/Wi-Fi regressions passed. Those results do
not establish full system suspend, endurance or measured battery savings.

Useful installed diagnostics:

```sh
y2-status system --json
y2-platform status cpu
y2-platform capabilities
y2-health --full --json
y2-platform boot-evidence
```

The platform provides internal root/data and SD ownership, ALSA/ASoC/CS43131,
DRM/Lima, native radio services, authenticated USB SSH/SFTP, workload QoS,
thermal authority, updates and rescue. Enabled, admitted and physically qualified
remain separate. Wired hardware output supports S16 stereo 44.1/48 kHz;
higher-rate sources convert to that sink. Optional Bluetooth encoders belong to
the private experiment profile and require separate peer/distribution acceptance.
Battery SOC and low-voltage policy remain provisional.

Only **BOOTIMG and ANDROID/Y2ROOT** are installation payloads. Preserve Y2DATA,
preloader/LK, NVRAM/PROTECT, calibration and factory/table partitions. The package
retains the exact physically qualified UART01 fallback pair. Only the owner
flashes; this repository is not an approved public firmware release.

For exact hashes, limits and the next owner boundary, use the
[candidate index](docs/knowledge/candidate-index.md),
[Platform API](docs/architecture/platform-api-v1.md),
[source reconstruction](docs/build/platform-v1-reconstruction.md),
[hardware gates](docs/knowledge/platform-v1-hardware-gates.md),
[roadmap](docs/planning/platform-v1-roadmap.md) and
[standing audit](docs/planning/roadmap-gap-audit.md#standing-milestone-boundary-rule).
Local candidate source commits may be newer than published `main`; later
documentation commits are not the revisions compiled into the images.
