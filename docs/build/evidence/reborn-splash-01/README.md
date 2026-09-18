# Reborn splash receipts

These receipts concern the targeted REBORN-SPLASH-01 startup update described in
[the deployment record](../../reborn-splash-01-deployment.md). Source in the
package manifest is the code actually built; later evidence commits do not
change the binary identity.

- `package-manifest.json` is the original preinstallation build receipt; its
  physical-pending field is historical. `physical-inspection.json` records the
  subsequent owner installation report, SSH handoff and bounded radio tests.
- `package-SHA256SUMS` covers the retained matched images, fallbacks and package
  metadata. Images are intentionally not committed to Git.
- `boot-preservation.json` verifies unchanged kernel/display/charging bytes,
  the exact early-archive differences and D08/BOOTIMG layout limits.
- `root-changes.json` and `e2fsck.txt` record the allowed root differences and
  raw filesystem check. `arm-tests.json` uses the installed target executables
  under QEMU and makes no hardware claim.
- `rust-tests.log`, `clippy.log`, `host-daemon.json`, `protocol-tests.log` and
  `boot-tests.log` retain the build-side tests summarized in
  `validation-summary.json`. Full offline build logs remain under
  `out/reborn-splash-01-build/splash-offline-rootfs.log` and
  `splash-boot-validation.log`.
- `loading.png` and `failure.png` come from the production C draw routine in its
  host test. They are previews, not photographs of the panel. The drawing test
  checks the allocation guard, expected colors and text coverage.

No Wi-Fi credentials, Bluetooth bonds, private SSH keys, raw calibration or
private media inventory are included. Full bounded radio inspection captures
remain in the ignored Y2Reborn output directory noted by the summary.
