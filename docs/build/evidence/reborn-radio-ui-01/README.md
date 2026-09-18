# Radio UI correction receipts

See [deployment](../../reborn-radio-ui-01-deployment.md). The package manifest
identifies the committed code actually cross-built. Later evidence commits do
not change that binary identity.

`rust-tests.log`, `clippy.log`, `host-daemon.json`, `tooling-tests.log` and
`arm-tests.json` retain host/emulation results. `e2fsck.log`, `changed-paths.json`,
`package-manifest.json` and `package-SHA256SUMS` retain root packaging checks and
exact payload/fallback identity. `validation-summary.json` records scope and
counts. Full offline build output remains in
`out/reborn-radio-ui-01-build/radio-offline-rootfs.log`.

Wi-Fi tests use a fake supplicant on real local Unix datagram sockets. Bluetooth
worker tests use a private dbus-daemon and fake BlueZ adapter, never the host
system bus or physical radios. Native ARM tests run inside the existing isolated
build environment. These tests do not claim a physical scan or pairing result
for the new image. The source inspection uses the previously installed system's
SSH evidence, retained in the Reborn inspection report and ignored output area.

No network credentials, bonds, private SSH key, raw calibration or private media
inventory is included. Installation remains an owner operation.
