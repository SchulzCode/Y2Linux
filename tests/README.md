# Y2Linux host test profiles

<!-- knowledge-base-scope: maintained-test-guide; baseline02-sync 2026-10-03 -->

## Retained Baseline02 validation

Fresh kernel/config/DT/module/ABI, Buildroot ARM and Reborn ARM checks pass.
The integrated suite has **391 cases: 386 passed and 5 dependency skips**;
27 native filesystem/GIO and 3 ALSA checks plus installed ARM checks cover the
missing minimal-environment dependencies. All **125 focused source tests** and
**238 Reborn tests**, formatting/lint, Cortex-A7 QEMU, installed ARM applications,
ELF/dependencies and preserving-package checks pass. These are retained build
receipts, not tests rerun by this documentation update.

The boot-policy fault tests are `tests/test_cpu_idle_boot_policy.py`; exact
MT6582 UART admission/ACK cases are in `tests/test_cpu_c3_uart.py`. Existing CPU,
cpuidle, timer, hotplug, CIRQ/SPM, clock, MMC and USB guards remain in their
production/source suites. No implementation tests are rerun for documentation
alone: use `python3 tools/development/check-docs.py`, JSON/matrix/manifest consistency
checks and `git diff --check` for this synchronization.

Physical acceptance is separate. UART01 already passed 21 guarded real C3
reset-and-return trials and normal-policy residency. The packaged SSH harness
must verify Baseline02's exact identity and automatic cold-boot receipt before
foundation checks and bounded C3 trials. See
[Baseline02](../docs/validation/Y2-BASELINE-02.md) and
[UART01 hardware](../docs/validation/Y2-CPU-C3-UART-PHYSICAL.md).


`tools/production/tests.sh` is the release-facing production-profile runner.
Run it inside the locked, device-isolated environment with
`tools/build/prepare.py` and `tools/build/run.py`. It generates the pinned
Linux UAPI headers needed by native host checks, runs the curated production
manifest/storage and source-contract tests, then checks ARM shell scripts and
the target ABI with Buildroot's ARM tools. Its QEMU checks do not access a Y2.

The explicitly listed `tests.test_*` modules are the current production gate;
the module list is intentional and new correctness tests belong there when
they validate the package path. The retained top-level test modules outside
that list are subsystem or historical/profile-specific regression tests. They
remain available for focused runs, but are not treated as a current production
qualification result merely because broad unittest discovery collected them.
`test_system_update_fallback` uses a real temporary ext4 image to check the
root-overlay fallback's kernel, filesystem, version and preservation contract.
It requires native e2fsprogs and blkid on the packaging host. The locked kernel
environment lacks these tools; its explicit skip must be accompanied by a
passing packaging-host run, plus validation of the actual fallback image.
In particular, `test_baseline*`, `test_dev*`, and profile-named hardware
contracts preserve evidence for their named source/profile boundaries.

Host prerequisites are explicit: pinned source/header inputs, Buildroot's ARM
rootfs and `qemu-arm`, and the production test environment marker. A missing
prerequisite is an environment result, not a passing test and not a reason to
silently skip the associated assertions. Physical-device tests are not part of
this runner.
