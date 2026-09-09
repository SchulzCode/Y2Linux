# M2 PWRAP/VUSB prerequisite — bounded read-only PMIC probe

2026-09-09. #23 supplies the immediate prerequisite for blocked USB logging #27.
M2 activation baseline `ebeed72`; latest physical evidence is the existing
owner-reported M1 heartbeat result. No new hardware success is claimed.

## Decision and smallest next experiment

Implement one cached PID1-triggered diagnostic read of PMIC CID and VUSB status
through already initialized PWRAP WACS2. This establishes whether normal LK
handoff leaves a usable AP read transport and enabled USB supply. It does not
initialize PWRAP, implement a regulator framework or enable USB. A failed gate
or timeout returns to the existing framebuffer heartbeat without retries.
Only after an identified BOOTIMG trial may these inherited states be treated as
observed. This is narrower than bringing up the entire controller on assumptions.

## Evidence contract

Existing immutable FM kernel/symbol/LK identities are in
[initial-ram-map.md](initial-ram-map.md) and [D13](risk-accepted-diagnostic.md).
New focused source/disassembly hashes: [m2-pwrap-sources.json](m2-pwrap-sources.json);
private files under `evidence-private/20260909-m2-usb/`. No full ROM re-audit.

| Item | Evidence and limit |
| --- | --- |
| USB controller vs PHY | FM `usb_phy_recover` at `0xc04d9cac` uses virtual `0xf1210000 + 0x800`; LK `0x81e0928c` onwards uses physical `0x112108xx`. Controller remains `0x11200000`. They are separate windows. |
| USB clock | FM `usb_enable_clock`, `0xc04d9b24..0xc04d9b48`, sets UNIVPLL_CON0 bit 26 then enables clock ID 10. Pinned `mt_clkmgr.h` identifies PERI_USB0=10. No clock write is in this probe. |
| Supply call | FM `mt_usb_init`, `0xc04d8038..0xc04d8044`, calls `hwPowerOn(1,3300,"VUSB_LDO")`, corroborating vendor USB init. Supply is internal VUSB, not permission for host VBUS sourcing. |
| PWRAP transport | FM `pwrap_wacs2_hal` at `0xc04b23b0` uses virtual `0xf000d000`; LK `0x81e11fc4` uses physical `0x1000d000`. Both encode reads as `(address >> 1) << 16`, write bit 31 clear. CMD/RDATA/VLDCLR offsets are `0x9c/0xa0/0xa4`. |
| Guards | Pinned MT6582 `pwrap_hal.h`: MUX_SEL `0x00`, WRAP_EN `0x04`, HIPRIO_ARB_EN `0x50`, WACS2_EN `0x94`, INIT_DONE2 `0x98`. RDATA init bit 21, sync-idle bit 20, request bit 19, FSM bits 18:16 (idle 0, complete 6). Require wrapper mux, enabled channel/init/arbitration (WACS2 bit 3), idle with no request before commands. |
| CID | FM `upmu_get_cid`, `0xc04bb518..0xc04bb558`, reads PMIC `0x0100`, mask `0xffff`, shift 0. Upstream MT6397 core uses the same MT6323 address and low-byte chip ID `0x23`. Return raw CID; refuse VUSB access if family differs. |
| VUSB | FM setter `0xc04c3988` changes `0x0502` bit 14; getter `0xc04c39b8` reads bit 15. Vendor `show_LDO_VUSB_STATUS` also reads this ordinary status/control register; it is not clear-on-read. Upstream MT6323 regulator describes fixed 3.3 V and enable bit 14. No setter is called. |
| Upstream compatibility | v6.18 has no established MT6582 PWRAP match. Its mt2701 WACS2 offsets agree, but mt8135/mt8173 differ. Offset agreement does not authorize a sibling compatible or full provider initialization. |

## Runtime boundary

The temporary board diagnostic exclusively claims `[0x1000d000,0x1000d0a8)`;
no PWRAP/MFD/regulator/clock driver is enabled. CPU0 and the original DT are
unchanged. At most two commands: CID `0x00800000`, then VUSB `0x02810000` after
matching CID. The only other write is WACS2 VLDCLR=1 to acknowledge **this
probe's** completed read. Do not clear a stale response at entry or reset/reinit
on timeout. Do not write any PMIC register, charge policy, rail enable, efuse,
calibration, USB controller/PHY/clock, loader or persistent storage.

Poll loops are fixed-budget with 10 microsecond delays; error exits remain
bounded even if timer bookkeeping stops. An MMIO bus fault/hang itself cannot
be timed out in software; retain the visible pre-probe stage and manual recovery
boundary. No IRQ, DMA or resource-owner takeover is added. Other inherited
wrapper masters keep their current arbitration; WACS2 is the AP software channel
shown in both FM and LK. If the resource is claimed, state is busy/stale or
initialization absent, return evidence and issue no command.

Trigger once after the first successful heartbeat and visible READ PWRAP stage.
Cache the result even on failure; subsequent file reads never repeat hardware
transactions. Export a fixed-size PID1-only binary snapshot through the existing
local diagnostic character device. Show raw gate registers, pre/post WACS state,
CID/VUSB, return code and valid mask on screen. Existing frame guard, timing,
mount/memory/CPU0 checks and 50-beat/60-second limit remain.

## Validation and hardware decision

Use one clean build in the locked environment, the relevant broader new-subsystem
review, production state-machine tests with denied/busy/stale/timeout/identity/
completion faults, ARM PID1 success/error fixtures, framebuffer/watchdog and
full D08/artifact/package checks. Verify linked code only emits the two read
commands and completion acknowledgements; no new syscalls or DT providers.
Retain exact final size/hash, source snapshot and previous candidate.

Hardware evidence required: selected artifact SHA-256, a readable complete screen
at early heartbeat and terminal stage (or exact last stage), raw PWRAP rows,
BEAT/FRAME, MemTotal, timer IRQ and host elapsed time. A successful read is
transport evidence only. VUSB bit 15 reports PMIC enable state, not a measured
3.3 V supply, trustworthy battery/charger telemetry or USB enumeration. #23 and
#27 remain open until their full criteria are satisfied.
