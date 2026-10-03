# Y2 C3 UART candidate physical record

## UART01 sealed software handoff — 2026-10-03

**C3 SOFTWARE_READY_NEEDS_NEW_FLASH. New candidate physical NOT_RUN.**
Owner flash is the remaining boundary. C3 has never been entered on the
connected candidate02; neither hardware failure nor hardware impossibility
is established. C1/C2 physical working results are preserved.

Candidate: `out/y2linux-cpu-c3-uart-candidate/`.
Build `Y2LINUX-CPU-C3-UART-01`, kernel `6.18.0-y2linux-cpu-c3-uart-01`,
root `2025.02.18-platform-v1.20`, release
`1.0.0-cpu-c3-uart-candidate.1`. Frozen Linux
`e9e8d63f9c94232c2b6627881e0967583e202dac`, Reborn
`b71b468860233faa0a42b8448ec5777fa952b8e3`. Source commits `7ef608a` and `e9e8d63`
implement the contract and required kernel errno include; subsequent docs are
receipts, not a different installed/build source.

| Payload | Bytes | SHA256 |
| --- | ---: | --- |
| BOOTIMG.img | 7208960 | `a35e7ccb11dfcc642089e422d48a274ab2c700dbba15d00b21cce7b463e4006a` |
| Y2ROOT.img | 536870912 | `d3a8282a9f5d1b0046283bce1b595eb5797b62c2bff90a62260e80bbfafb5d98` |

Fresh kernel/Buildroot/Reborn ARM and config/DT/modules/ABI checks pass.
Production suite375 cases: five minimal-host dependency skips are covered by
native filesystem/GIO/ALSA tests and ARM compilation/query. Extended source
contracts109, native packaging/filesystem/GIO27, ALSA evidence3 and Reborn238
cases pass; formatting/Clippy, Cortex-A7 QEMU, installed ARM helpers and ELF
checks pass. Preserving package, source seal (445 source/license files) and
sealed-image validation pass. Collector limitations remain recorded; this is
an owner-local package, not public distribution approval.

Receipts: `out/cpu-c3-uart-validation/`; all integrated exit codes0.
Manifest SHA256 `a43eb82521df9d94ec26b5b1cd46bd0234ff0807ffdcab7fedaf951deb04d48a`;
checksum inventory SHA256
`c3041b0a848b2ce8a0a991238cee3433a43b33bf9e0654cd92e4c0bc9c945c49`. Source archives match the frozen pair.
Only BOOTIMG/ANDROID payloads and accepted candidate02 fallback BOOTIMG/ROOT
are packaged; no Y2DATA, preloader/LK/NVRAM/PROTECT/calibration/table payload.

Owner action: use `MT6582_preserve_data_scatter.txt`, Download Only, selecting
BOOTIMG.img and ANDROID/Y2ROOT.img. Preserve existing Y2DATA. Boot normally,
then confirm installation; the prepared SSH harness will verify identity and
preserved regressions, attempt one25ms RGU/SRAM/budget1 timer wake, and allow20
further cycles only after real UART ACK and all same-boot restore checks pass.
The packaged `CPU-C3-UART-OWNER-HANDOFF.md` gives the exact invocation.
C3 stays default disabled/budget0 until the guarded physical test. No flash or
push was performed; six preexisting owner doc edits remain unstaged.

Final read-only SSH receipt:
`out/cpu-c3-uart-physical/20261003T164546Z/read-only-close.json`.
Installed identity remains Linuxdb0234e/Rebornb71b4688, kernelidle-02/rootv1.19,
boot `f155196b-64d4-45a4-88b3-27755a1a8926`, C3 disabled/budget0/entries0,
expected prior probe taint4096. It contains no new UART handshake or deep wake.

2026-10-03: new C3 UART candidate **NOT_RUN**; C3
**SOFTWARE_READY_NEEDS_NEW_FLASH**. No hardware ACK or dormant entry is claimed.

Installed candidate02 remains physically C1/C2 WORKING. C3 has never been
entered. Its settled blocker is hardware UART1 PERI bit17 at0x11003000,
not the Linux UART0 console. Existing physical evidence remains authoritative
in [Y2-CPU-IDLE-COMPLETION-PHYSICAL.md](Y2-CPU-IDLE-COMPLETION-PHYSICAL.md).
Fresh targeted read-only admission is under
`out/cpu-c3-uart-physical/20261003T164546Z/`: Linuxdb0234e/Rebornb71b4688,
kernelcpu-idle-02/rootv1.19, boot
`f155196b-64d4-45a4-88b3-27755a1a8926`, C3 disabled/budget0, prior probe
taint4096. No runtime controls, register writes or C3 attempts were made.

[The source contract and implementation](Y2-CPU-C3-UART.md) explain why an
eligible active UART clock can defer to the stock global request/ACK rather
than being forced off. The next result must capture the real ACK and return,
not merely an improved preflight. Owner flashing is required to install those
changes. A fresh normal boot removes the earlier probe taint; the harness
requires taint0 and exact package identity before guarded tests.

Run the packaged host harness after owner confirmation. It checks preserved
regressions, arms one bounded25ms timer wake with budget1/RGU/retained stages,
then20 further checked cycles only after success. Require unchanged boot,
new attempts/entries/resumes/successes and UART ACK, positive residency and
all timer/CIRQ/context/clock/media restores. Missing ACK must record UART_BUSY,
request cleanup and no dormant entry. Any failed trial disables C3 and stops
repetition. Store new raw SSH evidence under `out/cpu-c3-uart-physical/`.

Do not overwrite the candidate02 counters or promote C3 physical/release status
until those observations exist. Full-system suspend and electrical battery
measurement are outside this narrow pass.
