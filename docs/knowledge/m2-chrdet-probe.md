# M2-CHRDET-01 — PMIC charger-presence observation

2026-09-09, localized #23/#27 prerequisite after the
[same-boot PHY wake result](m2-phy-wake-hardware-result.md). USB is attached but
DEVCTL remains 80. Read the independent PMIC indication before considering the
stock internal VBUS-force sequence. No electrical cable/connector assumption.

## Exact source and access contract

The retained FM upmu_get_rgs_chrdet at c04b8c38 calls pmic_read_interface at
c04b8c68 with address **0x0000**, mask **1**, shift **5**. The pinned MT6582
upmu_hw.h independently identifies CHR_CON0=0x0000, RGS_CHRDET mask 1/shift 5.
This is the ordinary status accessor, not interrupt status or an acknowledge.
Focused disassembly is retained privately as fm-chrdet-accessors.txt; reuse the
existing FM provenance and pinned source d53dd75c3ff77cac3f5be58fddfe660e94f94d64.
FM charging_get_charger_det_status at c04cb0b0 calls this same accessor twice,
corroborating ordinary repeatable status reads (fm-chrdet-charging.txt).
The vendor usb_cable_connected also checks role and charger type; CHRDET alone
does not distinguish a data host, charger, usable data wires or USB readiness.

Extend the established PWRAP whitelist with exactly this one 16-bit PMIC read,
after successful CID/VUSB transactions and the MT6323 CID guard. Its AP WACS2
read command is **0x00000000** at AP+9c; bit31 remains clear. Use the existing
bounded completion/idle waits, acknowledge only this completed response and
never retry, clear stale responses or issue a PMIC write. Failure stops further
clock/USB probing. Preserve partial validity: CID=1, VUSB=2, CHR_CON0=4; full=7.

Snapshot grows four bytes to 260, shared by kernel and native PID1. Display
`CHR:<raw four hex digits> D:<bit5>` when valid, otherwise `CHR:---- D:?`.
Both CHRDET 0 and 1 are successful observations; do not turn a low status bit
into a transport error. The rest of the PHY wake probe is unchanged, including
its one guarded 6a write and calibration checks. No new hardware write beyond
the PWRAP read request/ack, no charger policy, interrupt enable, clock, VBUS-force,
MAC connection, DMA or calibration change. Everything remains cached once/boot.

## Validation and hardware boundary

This is a localized extension of the hardware-proven read transport, not a new
subsystem or memory/DMA/packaging policy. One clean build, affected PWRAP/clock/
state/wake/PID1 tests, linked transaction review, standard artifact checks and
D08/current-memory/BOOTIMG size/hash validation. No duplicate build or unchanged
source/ROM/recovery audit. Test both detection states and third-transaction
timeout, lost-init, invalid-FSM and acknowledge-to-idle failures.

Stop at BOOTIMG for the owner test with the USB data cable attached to the PC.
Return same-boot readable photos near BEAT 10 and 50/STOP showing CHR/PW/CLK,
USB and all four PHY rows, uptime/IRQ and LAST ERR. Retain the established
maximum 60 seconds from power-on and BOOTIMG-only recovery boundaries.
Enumeration/ttyACM is not expected; #23/#27 and M2 remain open.
