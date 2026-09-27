# Y2Linux

Linux 6.18 and Buildroot platform for the Innioasis Y2, paired with the native
[Y2Reborn player](https://github.com/SchulzCode/Y2Reborn). Both repositories'
`main` branches contain the latest published development source and documents.

**CPU Final Fix01 has been physically tested and fails normal-platform
acceptance.** GPT6/GPT4/PPI29, actual highres/NO_HZ, all eight real workload
classes, conservative 598/747.5/1040-MHz scaling, hotplug and bounded native
playback pass. SLIDLE remains blocked, high OPPs fail PWRAP readiness, a staged
suspend loses recovery, and USB stress loses connectivity. No release or full
same-boot wake acceptance follows from merging source to main.

Start with the [current platform state](docs/CURRENT_PLATFORM_STATE.md),
[Fix01 physical report](docs/validation/Y2-CPU-FINAL-FIX01-PHYSICAL-QUALIFICATION.md),
and [documentation index](docs/README.md). These supersede older pages' rolling
"current image", "next test" and pending-implementation statements.

| Boundary | Latest exact evidence |
| --- | --- |
| Physically tested Linux / Reborn | `0de6e951` / `36db1869` |
| Kernel / rootfs | `6.18.0-y2linux-cpu-final-fix01` / `2025.02.18-platform-v1.7` |
| Candidate | `out/y2linux-cpu-final-fix01-candidate/`, already owner-flashed before qualification |
| Acceptance | FAIL; Fix02 is one coherent correction plan, not a new candidate |
| Preserved fallback | Hardware02 `6.18.0-y2linux-hardware-02` / `2025.02.18-platform-v1.4` |

Later documentation commits do not change the compiled identities. The
[candidate index](docs/knowledge/candidate-index.md) carries full commits/hashes.

The platform provides internal root/data and SD ownership, ALSA/ASoC/CS43131,
DRM/Lima, native radio services, authenticated USB SSH/SFTP, telemetry, scoped
maintenance, root-only OTA and rescue contracts. Implemented, enabled, admitted
and physically qualified are separate states. S16 stereo 44.1/48 output is
enabled; high-rate sources convert to supported output. Native wide/high-rate
output remains gated. Optional Bluetooth encoders exist in the private experiment
build but their normal runtime endpoints are disabled. The enabled low-voltage
guard and estimated SOC are provisional, not calibrated pack measurements.

Useful read-only installed diagnostics:

```sh
y2-status system --json
y2-platform status cpu
y2-platform capabilities
y2-health --full --json
y2-platform boot-evidence
```

For API/build/safety boundaries, use the [Platform API](docs/architecture/platform-api-v1.md),
[source reconstruction](docs/build/platform-v1-reconstruction.md),
[hardware gates](docs/knowledge/platform-v1-hardware-gates.md),
[roadmap](docs/planning/platform-v1-roadmap.md) and
[standing milestone audit](docs/planning/roadmap-gap-audit.md#standing-milestone-boundary-rule).
No automatic deep-suspend/C3, high-OPP or USB-host qualification is implied.

Preserve stock preloader/LK, reserved memory, rescue access, protected partitions,
calibration/NVRAM and Y2DATA. The candidate installs BOOTIMG/Y2ROOT only.
Reviewed reports belong in Git; private raw evidence, keys, owner firmware,
generated images and caches remain local. This is a development source repository,
not an approved public firmware release.
