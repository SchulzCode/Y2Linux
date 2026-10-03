# Y2 C3 UART candidate physical record

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
