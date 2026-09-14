# M4 charging candidate: build receipt

Image source commit: `53aa9a6f86e024ae9ad54889facaeb5e24df3783`.
Kernel: `6.18.0-y2linux-m4-charge-01`.
One fresh production kernel output tree; a missing OF header was corrected and
that same build resumed. No intermediate charging BOOTIMG was produced or flashed.
Logs retain the initial compiler error and the successful completion.

BOOTIMG: `/home/luca/Dokumente/Code/Y2Linux/out/y2linux-m4-charge-01/BOOTIMG.img`.
**5361664 bytes**, SHA256
`1c144874577e5cc103ee88aea346ece1e2a20d0b521054560acb7b84d0c4c706`.

Fallback: `out/y2linux-m4-charge-01/fallback/BOOTIMG-previous.img`.
Owner-installed M4-ADC-01, **5355520 bytes**, SHA256
`c3c9778e396413fced69364190eb892456dd71c66a0c23fa650be2dcd30e7461`.
Fallback charging remains inhibited.

No rootfs/Buildroot or data update. Existing production userspace binaries are
hash-verified and reused. Retained Y2ROOT image identity is
`814a5b2543931e01cee2eb6f641c3b6e02317bd6308d2d663aea78f618bd554f`;
this is the original installed-image reference, not a hash of its live mutable filesystem.

Fourteen targeted tests pass with none skipped: actual charger sequence/worker
IO failures and recovery restrictions, watchdog strobes/deadlines, timed
termination/recharge, USB budget callbacks, PM ordering, ADC errors/calibration,
shared CPU transitions, MFD wake masks, retained eMMC suspend state, emitted DT
mutation rejection, PWRAP write firewalls and USB teardown/reconnect regression.

Production config, ARM kernel/module build, ARM EABI self-test, rescue ABI,
DT, internal Storage06 addressing/protected spans, memory/layout, MTK/Android
packaging, partition bounds, fallback compatibility and package hashes pass.
`artifact-validation.log` retains the validator's generic M4-01 status label;
the actual release, build commit and image identity are recorded above and in
`versions.json`/the package manifest. The package is the charging candidate.

[Stock charging contract and remaining limits](../../../knowledge/m4-charging.md).
[Exact owner flashing and attended SSH qualification](../../y2linux-m4-charge-01-deployment.md).

**Physical charging success remains untested.** M4 stays ACTIVE/PARTIAL.
Admission requires a configured 500 mA USB host and battery at least 3.4 V.
BATON temperature, calibrated current/SOC, wall charging and deeply depleted
recovery are unresolved. Seventy mA must be shown to exceed system consumption;
watchdog/protection probes alone do not establish a positive battery charge.
Screen-off/WFI should retain charging; active charging intentionally excludes
s2idle. No claim of pack hot/cold protection or measured full capacity is made.
The next action belongs to the owner: manual BOOTIMG-only deployment.
