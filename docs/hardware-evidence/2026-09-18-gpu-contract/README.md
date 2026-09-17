# GPU clock and stock-bin contract, before GPU-01

This is read-only hardware evidence, **not a Lima/rendering qualification**.
The physical device remains on CONNECTIVITY-10. Captures were made on
2026-09-17 UTC / September 18 local time, using the existing pinned SSH identity.
No BOOTIMG/Y2ROOT deployment, GPU power-on, register write, reboot, calibration
acquisition or user-state change occurred.

The always-on TOPCKGEN/APMIXED snapshot establishes MFG selector 1 and MMPLL
500.5 MHz through the existing stock-derived PLL decoder. The exact retained
stock Mali driver tests devinfo word 3 bit 19 before selecting UNIVPLL instead.
A second bounded read of the already reserved LK ATAG RAM finds that bit **0**
on this Y2 (tag `0x41000804`, 25 words, 22 devinfo entries). Only that one flag
is printed; no device identifiers or other devinfo contents are collected.
This closes the stock branch ambiguity without guessing from the MT6582 name.

Both tiny GPL observation modules have their complete source here. They print
once and deliberately return `-EAGAIN`; insmod's nonzero exit is expected and
neither module remains resident. The first module reads six always-on clock
registers; the second reads only bounded reserved RAM at `0x80000100`.
Neither maps the GPU. Loading external modules sets taint **4096 (O)** for this
inspection boot. A freshly deployed GPU-01 must start with taint 0.

Build: the existing isolated kernel builder, CONNECTIVITY-10 kernel output,
`make -C /src O=/build/kernel M=<observation-module-directory> modules`.
Transport: SSH with `BatchMode=yes`, `StrictHostKeyChecking=yes` and the existing
private host-key pin; copy the module to `/tmp`, insmod, collect its one log
line plus existing SPM state/taint/boot ID, then remove the temporary file.
Module hashes, capture completion timestamps and expected outcomes are in
[receipt.json](receipt.json). Sources and reviewed text captures are covered
by [SHA256SUMS](SHA256SUMS); firmware and stock kernel binaries are not published.

SPM remains `broken=0`, both power status words `0x3f4c` (MFG off), and the Linux
boot ID is unchanged. The post-inspection radio status retains controller
`6582/8a01/8a00`, calibration, radios off, and **zero errors/recoveries**.
Running-system charging remains active with no fault and watchdog service.
Those observations preserve the existing M4/M5 baseline; they do not qualify
GPU coexistence. Remaining M5 connection/audio tests stay pending by owner choice.

Exact GPU silicon revision, MMU behavior, rendering, power transitions and
resume remain for the first normal Lima probe and coherent GPU-01 procedure.
See [the implementation contract](../../knowledge/gpu-platform.md) and its
source receipts for stock register/IRQ/domain evidence and licensing.
