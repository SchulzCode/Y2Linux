# Baseline02 integrated preserving candidate

Owner requested a new latest-source baseline on2026-10-03 after UART01
hardware qualification. Candidate preparation is authorized; flashing and
pushing are excluded. All current Linux changes and Rebornb71b4688 are included;
there is no new CPU architecture, memory/layout or data-schema change.

Build `Y2LINUX-BASELINE-02`; kernel `6.18.0-y2linux-baseline-02`;
root `2025.02.18-platform-v1.21`; release `1.0.0-baseline-candidate.2`.
Only BOOTIMG and ANDROID/Y2ROOT are payloads; preserve Y2DATA and all protected
partitions. Fallback retains the exact physically qualified UART01 pair.

C1, C2 and C3 architecture is physically qualified on the preceding UART01
image: four-core WFI, C2+6971entries/84.35%,21 bounded reset-and-return C3
trials and+3624 normal-policy entries/65.25%, all checked context/timer/CIRQ/
clock/storage restores clean. See [authoritative hardware record](Y2-CPU-C3-UART-PHYSICAL.md).
Read-only admission reconfirms same boot/taint0/3647 C3 returns. This evidence
is inherited architecture evidence, not physical acceptance of new images.
The new package remains PHYSICAL_NOT_RUN until owner installation and checks.

C3 retains its boot default-off/budget0 policy. Existing normal guarded runtime
control remains available; the packaged harness enables it only after verified
identity, foundation regressions and21 bounded checked C3 returns. This does
not qualify full system suspend, battery-life improvement or wider endurance.

Fresh build/validation and seal receipts will be recorded under
`out/baseline-02-validation/`. Final package:
`out/y2linux-baseline-02-candidate/`. Existing historical candidate directories
are immutable. No source or userspace build outputs are reused.
