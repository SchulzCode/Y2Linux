# Y2Linux host test profiles

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
In particular, `test_baseline*`, `test_dev*`, and profile-named hardware
contracts preserve evidence for their named source/profile boundaries.

Host prerequisites are explicit: pinned source/header inputs, Buildroot's ARM
rootfs and `qemu-arm`, and the production test environment marker. A missing
prerequisite is an environment result, not a passing test and not a reason to
silently skip the associated assertions. Physical-device tests are not part of
this runner.
