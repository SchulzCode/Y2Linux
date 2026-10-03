# Baseline02 integrated preserving candidate

Owner requested a new latest-source baseline on2026-10-03 after UART01
hardware qualification. Candidate preparation is authorized; flashing and
pushing are excluded. All current Linux changes and Rebornb71b4688 are included;
the only new production behavior is the owner-selected automatic C3 boot policy.
There is no new CPU architecture, memory/layout or data-schema change.

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

Owner selects automatic qualified C3 after boot. Kernel registration still
starts quarantined/default-off/budget0. `S05y2-cpu-idle` applies the qualified
policy once after static identity/SPM/CIRQ/context/timer foundations pass:
budget-1, then CPU0 state2 enable, both read back. A failed write rolls back.
All dynamic topology/OPP/screen/workload/radio/USB/clock/deadline/UART ACK and
restore-failure guards remain unchanged. No active device or clock is forced.
Repeated startup respects a later manual disable. Recovery boot options remain
`y2.deep_idle=off` and `y2.cpu_safe=1`; `y2-platform cpu-idle-policy stop` closes
C3 for this boot. Status CPU exposes the boot-policy result/rejection.

The packaged harness first disables C3 before foundation qualification and
uses21 bounded checked reset-and-return trials before runtime observation.
New persistent cold-boot behavior is NOT_RUN until owner installation. This
does not qualify full system suspend, battery life or wider endurance.

Fresh build/validation and seal receipts will be recorded under
`out/baseline-02-validation/`. Final package:
`out/y2linux-baseline-02-candidate/`. Existing historical candidate directories
are immutable. No source or userspace build outputs are reused.

Automatic policy source tests and live ARM control/readback transaction pass.
Live test uses the same qualified UART01 boot and restores original budget-1/
CPU0 enable; it is not a new-image cold-boot test. `/run` evidence is transient;
no boot-time polling, persistent data record or radio/OPP/hotplug change is added.
Failed activation rolls back, failed foundations stay quarantined and subsequent
start cannot replace an owner's runtime control. Foundation and runtime behavior
remain distinct: normal activity can legitimately reject individual C3 attempts.
