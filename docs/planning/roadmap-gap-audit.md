# Y2Linux roadmap and gap audit

## CONNECTIVITY-10 physical Wi-Fi result — 2026-09-17

Owner-installed -10 exposes stable wlan0/cfg80211, and two scans return 23 and
12 BSS entries. Runtime Wi-Fi restart and BlueZ power-on with Wi-Fi active
succeed. Core/transport errors and recoveries remain 0, kernel taint is 0,
and no diagnostic module or runtime correction is used.
[Physical result](../hardware-evidence/2026-09-17-m5-connectivity10/README.md).

The missing completion callback on regulatory TX power command `0x38` was
the exact -09 blocker. Its targeted correction is now physically verified.
The [installed BOOTIMG and -09 fallback](../build/y2linux-m5-connectivity-10-deployment.md)
retain -07 root/data, Bluetooth fixes and M4. Both radios and saved preferences
are returned off. No network is configured; association/DHCP/data and broader
Bluetooth/coexistence qualification remain open. M5 #31 is ACTIVE / WI-FI AND
BLUETOOTH ADAPTERS VERIFIED / CONNECTION AND RADIO QUALIFICATION OPEN.
This remains an interface checkpoint within M5, with no major boundary crossed.

## Wi-Fi exact regulatory failure / CONNECTIVITY-10 correction — 2026-09-17

Targeted -09 SSH diagnostics show Wi-Fi firmware ready and successful RF/BB
capability queries. The exact failure is the regulatory TX power OID (`0x38`):
successful transmission frees the no-response command without completing its
waiter because the producer omits its callback. The subsequent timeout causes
EIO recovery and interface teardown. [Physical evidence](../hardware-evidence/2026-09-17-m5-wifi/README.md).

[CONNECTIVITY-10](../knowledge/m5-connectivity10-corrections.md) supplies that
command's standard completion callbacks, with unchanged wire format, power
policy and transport. Sixteen targeted checks pass, including old-source
failure replay. Build BOOTIMG-only, retain -07 root/data and working -09
Bluetooth fallback, then verify after manual owner installation. No new
hardware/memory scope or major milestone transition. M5 #31 remains ACTIVE /
BLUETOOTH ADAPTER VERIFIED / WI-FI OID CORRECTION / QUALIFICATION OPEN.
Both radio preferences remain off; BlueZ power-on/off is rechecked after
diagnostic cleanup. Accepted M4 remains intact.


## CONNECTIVITY-09 physical result / permanent BlueZ adapter — 2026-09-17

Owner-installed -09 reaches standard hci0/BlueZ on normal boot. Both patch
downloads/resets, the 630-byte RF calibration result and HCI initialization
succeed. Standard runtime BlueZ power-on/off works with zero core errors,
transport errors or recoveries. No diagnostic module or runtime correction is
used; kernel taint is 0. [Physical result](../hardware-evidence/2026-09-17-m5-connectivity09/README.md).

The original EPROTO was rejection of the successful RF result. The later
`0x1004` page-2/status-`0x30` defect is corrected only for `6582/8a01/8a00`
using the existing Linux quirk and verified page limit. The
[-08 trace and controlled test](../hardware-evidence/2026-09-17-m5-connectivity08/README.md)
remain the causal evidence. Its Wi-Fi test reached function enable and Wi-Fi
Start but recovered with EIO before wlan0; that exact next failure remains open.

CONNECTIVITY-09 is installed from `f55fff4`, BOOTIMG 6150144 bytes, SHA256
`200346dff20ea1ffe769278bc8c52a19402d850a380c2054f6e93d1ef167de7a`.
Fifteen targeted checks and emitted artifact/package checks pass.
[Deployment receipt](../build/y2linux-m5-connectivity-09-deployment.md).
Retain -07 root/data and -08 BOOTIMG fallback. Both radios and saved preferences
are returned off. Accepted M4 is intact; pairing/audio/coexistence remain open.
M5 #31 is ACTIVE / BLUETOOTH ADAPTER VERIFIED / WI-FI STARTUP FAILURE /
QUALIFICATION OPEN. No major milestone transition or broad audit is implied.
Installation-pending descriptions below are historical.

## CONNECTIVITY-07 exact protocol failure / CONNECTIVITY-08 correction — 2026-09-17

Live SSH identifies MT6582/HVR `8a01`/FVR `8a00`. A stopped-buffer snapshot
proves full STP negotiation, both patch downloads and both resets succeed.
The next RF calibration event is 630 bytes with valid checksum/CRC and success
status; the native 256-byte reply limit rejects it as EPROTO. This supersedes
the provider/TX-pending descriptions below. Factory acquisition, DMA/IRQ and
identification are settled and are not repeated.

[Physical evidence](../hardware-evidence/2026-09-17-m5-protocol/README.md) and
[the scoped correction](../knowledge/m5-connectivity08-corrections.md) preserve
strict framing/status checks and accept only this controller's extended RF
result during its matching command. Build CONNECTIVITY-08 BOOTIMG-only, retain
CONNECTIVITY-07 root/data and its BOOTIMG fallback, then use the established
manual installation boundary before live BlueZ/cfg80211 checks. M5 #31 stays
ACTIVE / PROTOCOL CORRECTION / PHYSICALLY UNQUALIFIED. M4 remains accepted.
No milestone transition, RAM/platform expansion, GPU/Reborn or broad audit.

CONNECTIVITY-08 is now built from `8b18ca3`: BOOTIMG 6150144 bytes, SHA256
`5b8e333cdb9fff7bf314abf905ad11a7f26d23e8dab4fd4a4cf78a0bd58b0d78`.
Fourteen targeted connectivity checks, original-failure replay, emitted artifact
and BOOTIMG-only package validation pass. [Ready manual handoff](../build/y2linux-m5-connectivity-08-deployment.md).
New hardware progress remains pending installation; no radio readiness is inferred.

## CONNECTIVITY-06 physical result / mounted-eMMC correction — 2026-09-16

Internal eMMC is mmcblk1 on this owner-installed boot. Root/data work; the
provider's hardcoded mmcblk0 fails before activation. TX rearm is not physically
exercised yet. [Evidence](../build/evidence/y2linux-m5-connectivity-06/README.md).
CONNECTIVITY-07 identifies the mounted root/data parent and retains strict MMC,
partition, node and geometry checks. Rebuild the production helper/Y2ROOT with
Y2DATA preserved. M5 remains ACTIVE / PROVIDER STARTUP FAILURE / UNQUALIFIED.
No protected writes, memory/platform expansion or repeated entry audit.

## CONNECTIVITY-05 physical result / CONNECTIVITY-06 correction — 2026-09-16

Owner-installed CONNECTIVITY-05 completes MD calibration and stays responsive,
but its first WMT command leaves two trailer bytes in TX DMA; no TX IRQ is serviced.
[Physical result](../hardware-evidence/2026-09-16-m5-connectivity05/README.md).
CONNECTIVITY-06 restores the stock per-transfer IRQ rearm/submission order,
verified by a regression reproducing that exact stall. It is a BOOTIMG-only
correction retaining root/data, memory layout and accepted M4. M5 #31 remains
ACTIVE / TRANSPORT STARTUP FAILURE / PHYSICALLY UNQUALIFIED. No entry audit is repeated.

## CONNECTIVITY-04 physical result — 2026-09-16

The owner installed `ac833c8`. SSH confirms CONNECTIVITY-04 kernel, retained
CONNECTIVITY-03 root, successful MD completion (`FS=841`) and verified CONN
remap/chip ID `6582`. Linux stays responsive through 813 seconds with the same
boot ID and no observed Oops/panic, including one bounded runtime recovery.
The first WMT command times out; no usable Wi-Fi/BlueZ adapter exists.
[Physical evidence](../hardware-evidence/2026-09-16-m5-connectivity04/README.md)
and [85 build checks / 14 package rejection cases](../build/evidence/y2linux-m5-connectivity-04/README.md).

M5 #31 remains ACTIVE / WMT STARTUP TIMEOUT / PHYSICALLY UNQUALIFIED. The next
correction uses mandatory STP framing from the first BTIF command, as stock
requires. Preserve the remap fix, corrected helper, M4, Y2ROOT and Y2DATA.
No scope/memory expansion or repeated entry audit.

## CONNECTIVITY-03 startup crash — 2026-09-16

Early SSH verifies CONNECTIVITY-03 kernel/root and the corrected calibration
helper. This unit now completes the MD gate: `stage=2 FS=841 restore=1 open=0
result=0 poweroff=0`. Immediately afterward the owner observes kernel faults
on idle CPUs in interrupt processing, and SSH stops responding. This is not a
connectivity pass; #31 remains open. CONNECTIVITY-03 is not a working rollback.

Stock FM disassembly confirms a wrong native CONN EMI register offset:
INFRACFG + `0x1310` instead of `0x310`. The targeted
[CONNECTIVITY-04 correction](../knowledge/m5-connectivity04-corrections.md)
fixes the offset and requires readback before CONN power-on. Build a production
BOOTIMG retaining the corrected CONNECTIVITY-03 root and Y2DATA. No RAM/layout
expansion, factory write, M4 redesign or repeated entry audit. Physical crash
elimination and the remaining radio qualification are still required.

## CONNECTIVITY-02 physical result — 2026-09-16

The owner installed the targeted BOOTIMG correction (`dd84462`). SSH confirms
53 completed calibration FS exchanges (previously zero), followed by an MD_EX
firmware exception before readiness. MD shutdown reports success; Wi-Fi and
BlueZ remain unavailable. The early regulatory-data error is fixed. Internal
mounts/USB SSH work; PC charging shows fault 0 and voltage-hold recovery.
[Physical evidence](../hardware-evidence/2026-09-16-m5-connectivity02/README.md)
and [build checks](../build/evidence/y2linux-m5-connectivity-02/README.md).
M5 stays ACTIVE / FIRMWARE EXCEPTION / PHYSICALLY UNQUALIFIED. The next work is
exception/FS-response diagnosis, not a broad entry audit or M4 redesign.
The image is already installed; earlier awaiting-installation text below is history.


## First M5 physical inspection — 2026-09-16

The owner installed CONNECTIVITY-01. Read-only SSH confirms the M5 kernel/root
markers, internal p5/p7 mounts and wired SSH. Radio initialization fails at the
MD calibration handshake (`stage=1 FS=0 result=-71`), with successful reported
MD shutdown and no usable BlueZ adapter or Wi-Fi interface. Early regulatory
database loading also precedes rootfs availability; the clock is unset (2022).
[Physical snapshot and limits](../hardware-evidence/2026-09-16-m5-connectivity01/README.md).
M5 remains ACTIVE / STARTUP FAILURE / PHYSICALLY UNQUALIFIED. Make targeted
corrections from these observations, then continue the coherent qualification.
No new broad audit or M4 redesign is called for by this inspection.

## M5 candidate handoff — 2026-09-16

CONNECTIVITY-01 is built from `022c701010c467904ab6025cd98535d3b861c771`.
The integrated kernel/rootfs passed 81 production/M4/connectivity tests,
eight isolated ARM userspace checks, emitted memory/DT/BOOTIMG validation,
rootfs validation and 16 package rejection cases. The preserving package
contains new BOOTIMG/Y2ROOT and the accepted POWER-03/previous-root fallback;
there is no Y2DATA payload. No assistant deployment or protected write occurred.

[Complete manual handoff, image hashes and physical procedure](../build/y2linux-m5-connectivity-01-deployment.md).
M5 #31 is ACTIVE / AWAITING OWNER INSTALLATION / PHYSICALLY UNQUALIFIED.
Stop for manual installation, then perform one coherent SSH qualification.
Silicon stepping, own calibration completion, radio functionality, EDR/BLE,
coexistence, recovery and M4 regression remain physical gates. FM is excluded
on this older board. This is a build/implementation update using the completed
entry audit below, not another broad entry audit or a milestone closure.

## M5 implementation authorized — 2026-09-15

The owner reports that POWER-03 works and explicitly accepts M4, then requests
M5 implementation. This is owner acceptance, not a new agent-run measurement
series. It supersedes the pending-deployment/M4-only instructions below.
POWER-03 (`16875c4`, BOOTIMG SHA256
`66b6ecd5ef54da6f3ea07be2c9deda284d0a7636e9c6da4c5d72d84eca5fc010`)
is the accepted power-platform baseline and M5 fallback.

Use the completed entry audit at `67cbe8f` without repeating it, as explicitly
directed by the owner. M5 #31 is ACTIVE / IMPLEMENTING; native radio behavior
is not yet physically qualified. Implement factory-data decoding, one shared
connectivity owner, AHB Wi-Fi, BTIF/STP/HCI and persistent standard userspace.
Preserve M4, the RAM exclusions, protected partitions and Y2DATA. Build one
integrated BOOTIMG/Y2ROOT candidate and stop for manual owner installation.
Older-board FM, GPU, Y2PlayerNative and OTA implementation remain out of scope.

Native implementation now includes the shared CONSYS/MD calibration owner,
BTIF/AP_DMA/STP/HCI, cfg80211 AHB fullmac, read-only factory provider, verified
owner firmware loading, standard Wi-Fi/BlueZ/BlueALSA userspace, persistent
preferences and a preserving BOOTIMG+Y2ROOT package. See the
[implementation and qualification contract](../knowledge/m5-connectivity-implementation.md).
Source/host checks do not satisfy physical M5 gates; #31 stays open for the
manual candidate deployment and coherent physical session. The accepted M4
image remains the fallback; no new M4 physical measurements are claimed.

## Focused M4 completion — 2026-09-15

The owner's latest direction supersedes M5 implementation scope: **finish M4,
do not start M5**. The completed entry audit below is retained; no broad audit
is repeated. Entry `67cbe8f` is clean. Targeted SSH still finds the same voltage
fault with the battery now about 3.74 V. The existing SPM, storage, audio and
minimal offline architecture remain the production foundation.

[POWER-03 source corrections](../knowledge/m4-end-user-power.md#power-03-correction-of-observed-failures--2026-09-15)
address charge cutoff/hold, the stock OVP setting, first-fault evidence, RTC
epoch, word-width thermal efuse acquisition and MUSB runtime ownership.
POWER-03 is built from `16875c4` and passes 74 production/M4 tests, ARM rescue
checks, package validation and 12 rejection cases. The
[BOOTIMG-only handoff](../build/y2linux-m4-power-03-deployment.md) now stops for
owner manual deployment. M4 #30 remains ACTIVE/PARTIAL;
the full physical exit criteria remain open. No memory/layout/current ceiling
expansion, guessed DVFS voltage, protected write or radio implementation.

## M5 fresh-session entry audit — 2026-09-15

**Entry audited; integrated M5 implementation/deployment BLOCKED.** The owner
authorizes a complete production Wi-Fi/Bluetooth audio platform on the current
internal-root architecture, with manual deployment only. That supersedes issue
#31's deferred planning wording. The requested qualified M4 foundation is not
established: charging/offline/deep-suspend/RTC/thermal gates remain substantial.

Entry HEAD `bbbbbf20e96860089b015c19501b7d39638c5419` equalled origin/main,
with prior POWER-02 USB/evidence changes, now preserved in `ef285f1`. Read
current history/handoff, all eight open issue summaries, relevant epic bodies,
current DT/config/BOOTIMG, Buildroot, power/audio/data architecture, stock
ramdisk/firmware, own stock radio metadata and pinned donor/RE evidence.
Reassessed every coverage row below; settled M1/M2/M3 work remains intact.

[Fresh authenticated hardware evidence](../hardware-evidence/2026-09-15-m5-entry/README.md)
confirms complete image-length POWER-02 BOOTIMG identity, internal p5/p7 ext4
read-write, owner-key SSH, four CPUs and Y2Audio. Strict host-key access matched
the exact previously retained September 14 fingerprint; default trust/account
settings were unchanged. MUSB is `on/active` with the preserved workaround.

**New material failure:** at uptime 747.328296 s charging reached 4.200073 V
and latched `Y2_FAULT_VOLTAGE=0x8`; it remains off at the fresh inspection.
Source explains the 4.2-V software guard, not the analog cause or the separate
September 14 OVP `0x10`. SPM has entries=0/resumes=0 on this boot; RTC reports
2082 and thermal accuracy is unresolved. #30 stays ACTIVE/PARTIAL. The earlier
short no-fault interval and pending-deployment wording do not override this.
The power platform is preserved, not rebuilt under M5.

[The connectivity audit](../knowledge/m5-connectivity-entry.md) records own
CONSYS_MT6582/BTIF/DMA/IRQ evidence, older-board FM exclusion, five-file stock
firmware hashes/header order, AHB Wi-Fi and STP/HCI architecture, userspace,
Y2DATA ownership and hard qualification gates. Own NVRAM/PROTECT_F/PROTECT_S
were read with bounded normal block reads after identity/geometry checks.
Raw data and identifying hashes stay private. Record decoding, actual silicon
HVR/FVR and valid addresses remain unproved; external calibration cannot fill
that gap. No native radio operation is inferred from source metadata.

Next: resolve #30's charging/qualification failure within M4; reconcile own
controller/calibration for #31 before an integrated candidate. No M5 kernel,
DT or rootfs change, build, flash, radio enable, protected write or power-state
change. No new issue shell or closure. Existing RAM exclusions, storage layout,
M3 residuals, recovery and later gates remain. Host firmware extraction reused
established ROM provenance; no unchanged full source/ROM/recovery audit.

## Charging-test USB blocker — 2026-09-15

At `bbbbbf2` with the existing September 14 evidence edits preserved, the owner
requests a focused fix for the first cable reconnect because it prevents M4
wall-charger observation. Current POWER-02 SSH samples show PC charging active,
fault=0 and about 4.13–4.15 V; this narrow result does not resolve the earlier
OVP failure or qualify wall charging. Two inspected boots have different uptime
histories, and the wall-session log was lost across reboot. An owner-operated
five-second removal/reconnect again leaves the Y2 absent from host USB while
the owner reports it remains powered on. The saved device log subsequently
confirms successful re-entry followed by an IRQ-storm shutdown (`-EOVERFLOW`,
-75), rather than a PHY admission failure.

Reviewed every coverage row, current source/retained hardware evidence, and the
eight open issues #16/#27–33. The existing #27/#28 reconnect gap now blocks #30;
this is a localized repair within the documented M4 test-blocker exception,
not a new milestone, hardware subsystem or expanded charging policy. A bounded
recorder now saves existing USB/charging diagnostics to production Y2DATA so a
required restart need not erase the failure. A standard MUSB `power/control=on`
workaround then passes two physical reconnects on one boot, including a DCP wall
session of 12.96 seconds with no charging fault. The exact POWER-02 startup
workaround is installed and its readback matches source; a fresh boot of that
startup edit is not tested. This is a tested workaround, not a kernel fix or
full reconnect/charging qualification. Extra idle power is unmeasured. See
[the hardware result and limits](../hardware-evidence/2026-09-15-usb-reconnect/README.md).
All other coverage classifications and phase gates retain their prior limits;
M4 stays ACTIVE/PARTIAL. No unchanged source/ROM/recovery audit or broad
qualification is repeated.

2026-09-14 post-deployment read-only result at repository `bbbbbf2`:
[M4-POWER-02 charging fault](../hardware-evidence/2026-09-14-m4-power02-charge-fault/README.md).
The owner reports wall-charge animation without recovery, then stock recharge
and Linux reinstall. USB/SSH identify POWER-02; no physical BOOTIMG hash was
taken. In the current normal PC-connected boot, SDP/500-mA allocation works,
but 450-mA charging stops after about one second with latched OVP fault 0x10.
The earlier wall session is not retained. This supersedes pending-deployment
statements, not the limits of prior acceptance. M4 #30 remains ACTIVE/PARTIAL;
the next blocker is the first OVP trigger and stock/Linux protection settings,
before further charging qualification. Other power, thermal, storage and later
phase gates are unchanged. This inspection makes no milestone transition or
hardware-scope expansion; no build, flash or device-state write was performed.

2026-09-14 candidate boundary review: M4-POWER-02 from `83d475e` passes 72
production/M4 checks, ARM rescue ABI and package identity/rejection checks.
BOOTIMG is 5378048 bytes, SHA256
`1852dfc995953f86ef5c47349515e8d6f06478daa6c1a89efe0f223501c226c9`.
The [manual qualification sequence](../build/y2linux-m4-power-02-deployment.md)
and verified prior charging-kernel fallback are prepared. Reconsidered every
M4 exit row against the latest failed 70-mA baseline, source reconstruction and
new host results: all new charging/offline/deep-suspend/RTC/poweroff physical
gates remain open. Root/data layout, calibration and stock loaders remain
unchanged. No M4 closure or M5 activation follows from this candidate.

2026-09-14 implementation audit addendum: actual FM Y2 CPU-domain shutdown,
restart-vector ownership and upstream 32-bit ext4 inode initialization are now
reconciled in [the active M4 contract](../knowledge/m4-end-user-power.md#cpu-domain-and-suspend-integration-audit-before-implementation).
The owner-confirmed device is off; the 70-mA net-gain baseline failed. CPU hotplug,
deep suspend and the new charging policy remain unqualified. This authorizes
no milestone closure or M5 activation; the current owner request supplies the
production hardware scope.

Original audit date: **2026-09-09**. Documentation/planning only. No implementation, build,
BOOTIMG creation, device access or flash. Repository baseline `00b4afa`; GitHub
issues #1–#27 and M0/M1 inspected before adding five deferred epics #28–#32.
Y2PlayerNative has no open application issues and its checked-in content remains
blueprint/planning material; no native platform readiness is inferred from that.

## M4 end-user completion scope — 2026-09-14

Fresh-session audit at local `aacb9a5`, clean at entry, five commits ahead of
the remotely verified `444e7ed`. Re-read the handoff, M4 code/docs and retained
stock/ADC/charging evidence, all open issue summaries and epic bodies. The
owner's physically observed active M4-CHARGE-01 test supersedes the older
inhibited/pending-deployment statements below. The live USB device independently
identifies that release; the owner confirmed the changed host key and strictly
pinned SSH observation succeeded. The same-boot 70-mA trace then demonstrated
falling voltage until the 3.4-V guard stopped charging. The owner switched the
Y2 off. This supersedes the initial active-charge report; it is no gain pass.

The owner explicitly authorizes one coherent production completion and physical
qualification pass, including stock-evidenced normal charging currents,
PC/wall/power-bank source detection, low-battery recovery, **offline charging in
the production rescue/initramfs**, deep suspend/same-session resume, device
restore, RTC, reboot and actual poweroff. Offline charging includes timed native
UI, status versus deliberate Power-on, removal-to-off, and poweroff-with-charger
returning offline. This supersedes the earlier fixed-70-mA, configured-host-only
scope and the former allowance to close M4 with merely bounded charging.

Reassessed every coverage row: internal root/data, Linux/SSH and the M3 audio
foundation remain confirmed within their retained limits; active charging is
owner-confirmed but energy gain and final source policies remain unqualified.
Thermal/frequency/RTC/PM implementations are not physical acceptance. Deep SPM,
offline-charge routing and early recovery require integrated physical acceptance. No memory
or storage-layout expansion, loader/calibration changes, M5 activation, or
broader qualification claim follows from this audit. #30 remains ACTIVE/PARTIAL;
#29 residuals, #33 wider qualification, #27 reconnect and M0/M6 risks remain.

The [current scope and physical exit matrix](../knowledge/m4-end-user-power.md)
is authoritative over historical checkpoints below. First observe the installed
70-mA/4.175-V profile without changing current, finish source/live investigation,
then build one integrated candidate with preserved fallback. Hardware closure
requires the complete owner acceptance, including offline charging and genuine
deep standby. Percent/current/pack-Celsius may be explicit optional follow-ups
under the owner's conditions; the hard power gates may not.

## M4 integrated power activation — 2026-09-13

### Stock-equivalent charging scope — 2026-09-14

The owner now explicitly authorizes reconstructing the actual stock Y2 charging
contract and implementing one conservative production candidate at 70 mA /
4.175 V. This supersedes treating missing BATON Celsius as an unconditional
gate: an evidenced alternative stock hardware-protection architecture may
support charging, while BAT0/temp and current/SOC remain unavailable. It does
not authorize disabling or guessing protections, higher current/CV, live
charging before the candidate, or assistant flashing.

Entry `d1ac549` is clean. SSH confirms the installed M4-ADC-01, BAT0 present /
Not charging, USB online and 3441137 uV at uptime 1243.93 s. The prior 120-sample
sensor result is retained, not repeated. Reconsidered all coverage rows and
phase gates against this owner correction, current production source, retained
stock binary/BSP and all eight open GitHub issues (fresh read, no state change).
M4 #30 stays ACTIVE/PARTIAL; M3/storage residuals, deferred USB reconnect and
M5/M6/player gates are unchanged. No memory or storage-layout expansion.

The next implementation gate is the real stock charger enable/watchdog,
OVP/thermal/presence, USB input and bounded termination contract. Production
code must check the preserved protections, fail closed on critical faults,
coordinate USB/PM lifecycle and retain the shared PWRAP/MFD owner. Build one
BOOTIMG-only charging candidate if that contract is established; physical
charging success remains pending the owner's manual deployment and monitored
voltage-trend/removal/reconnect test.

Stock reconciliation now establishes the CID2023 PMIC thermal-shutdown init,
explicit BATON hot-detector disable, charger enable/WDT sequence, 4050 mV
stock top-off threshold, 3 h/24 h budgets and 4110 mV recharge. Production code
implements fixed 70 mA/4.175 V, configured USB budget gating, checked hardware
protections, fault inhibition, manual standard power_supply policy and PM ordering.
The latest inherited battery OVP selector remains unchanged. No pack temperature,
measured current or SOC is claimed. [Contract and limitations](../knowledge/m4-charging.md).

Reconsidered the coverage matrix before creating the integrated candidate:
charging under load, cable reconnect, physical protection behavior, CHRDET wake,
off-state insertion and recovery below the conservative 3.4 V start floor remain
unqualified. Source and fault-injection tests do not advance those hardware gates.
M4 remains ACTIVE/PARTIAL; no new milestone, memory/storage scope or player work.

The final M4-CHARGE-01 candidate is built from `53aa9a6`, reusing Storage06
userspace with one kernel compilation/header correction and no diagnostic image
series. Fourteen targeted checks, actual DT/config/layout/rescue ABI and final
BOOTIMG-only package validation pass. [Build receipt and hashes](../build/evidence/y2linux-m4-charge-01/README.md).
No hardware acceptance status changes: the next boundary is owner manual flash
and an attended 70 mA voltage-trend, removal/reconnect and PM regression session.

### ADC owner boot and charging gate — 2026-09-14

Source `bbd0664dcbc1190a5e16e3b3f5889f2154cb04fd` builds one production
Y2LINUX-M4-ADC-01 update with unchanged charging inhibition. The owner manually
booted it before the packaging handoff. Authenticated SSH confirms the release,
all four CPUs, internal p5/p7 and the added IIO channels. A clean kernel build,
eight targeted production checks and DT/config/BOOTIMG/package validation pass;
root/data are reused. [Artifact receipt](../build/evidence/y2linux-m4-adc-01/README.md).

Reconsidered the coverage matrix and phase gates against this new physical
checkpoint and today's existing issue audit. 120 bounded samples plus the
owner's normal-handling observation establish raw BATON1/ISENSE acquisition,
but no resolved BATON temperature response. Both pack-temperature conversion
and sense-resistor/current calibration remain unvalidated. Charging stays
blocked at pack thermometry; no production charging policy or final charging
candidate is claimed. [Measurements and exact next gate](../knowledge/m4-battery-acquisition.md#owner-deployed-adc-result--same-day).

M4 #30 remains ACTIVE/PARTIAL, matching the existing open GitHub epic (read
again after the samples); no milestone/epic state transition is required.
No closure, new phase, radio/player scope,
storage-layout or memory expansion follows. Display/ASoC/PM implementations
are unchanged; broader physical qualification and existing ext4 boot warnings
remain tracked. The next sensor experiment can use the installed ADC kernel;
no further diagnostic image is required to collect readings. The agent did not
flash, enable charging, change current/CV/protections, or suspend/reboot/poweroff.

### Charging completion entry — 2026-09-14

Owner explicitly requests one conservative production charging completion pass,
with BATON/ISENSE acquisition initially inhibited and a manual BOOTIMG boundary.
Current local HEAD is `444e7ed`, clean at entry. Fresh SSH confirms the already
owner-deployed `6.18.0-y2linux-m4-01`, continuing uptime, BAT0 present/Not charging,
USB ONLINE=1 and configured 70 mA / 4.175 V. The preceding same-device battery audit
establishes the raw BATON1/channel 5 and ISENSE/channel 6 paths, but neither had a
ready result. Stock compiled NTC/current constants are hypotheses requiring
physical validation; PMIC die calibration does not calibrate the pack.

Reconsidered every coverage row below against the current owner scope, current
M4 source, retained M2/M3/Storage06 evidence, live M4 bindings/DT/logs and all
eight active GitHub issues (#16/#27–33, read 2026-09-14). #30 remains ACTIVE/PARTIAL;
#33 wider storage and #29 audio residuals remain open. No phase closes, no radio
or player starts, and unchanged provenance/recovery/stress gaps stay open.
Current M4 boot, BAT0 and die telemetry supersede the earlier pending-deployment
description; suspend, physical poweroff and whole-platform qualification are
not inferred from those reads.

The immediate acquisition blocker is concrete: M4-01's built-in PWRAP policy
admits only request bits 3/7 at 0x076e (`0x88`), and the ADC driver exposes only
those channels. BATON1/ISENSE need bits 5/6. A loadable consumer would still be
rejected by that same transport guard. PWRAP has no unbind operation, regmap
debugfs is read-only, and KEXEC/KPROBES are disabled. No raw WACS client, guard
patching or live kernel memory modification is authorized as a substitute.

The source change is the same production ADC owner exposing BATON1 and
ISENSE raw/scale channels with scoped request permissions and propagated
conversion errors. Charging enable and all current/CV/watchdog/protection writes
remain prohibited until the physical temperature and policy prerequisites are
established. This is a prerequisite, not a completed charging implementation.
A production BOOTIMG-only sensor update, Y2LINUX-M4-ADC-01, is the minimum
supported deployment needed to obtain that evidence. It keeps charging
inhibited and is explicitly not Y2LINUX-M4-CHARGE-01. The owner's single final
charging-candidate objective cannot be completed on the current guarded boot
without this prerequisite. No assistant flash or live kernel patch is performed.
The physical NTC connection/conversion is the first unresolved charging gate;
input-budget/protection/termination qualification follows it.

[The exact-device evidence, implemented channels and smallest experiment](../knowledge/m4-battery-acquisition.md)
record this bounded change. Source/transport failure checks and the actual ARM
ADC object compile pass before packaging; emitted configuration/DT/BOOTIMG checks
apply to the sensor update. Existing M4 physical acceptance remains open.

### Candidate boundary re-audit — 2026-09-14

Reconciled the coverage matrix and milestone gates again before handing off
Y2LINUX-M4-01. There is no new physical M4 result: the latest actual hardware
remains the authenticated Storage06 baseline and retained M2/M3 evidence.
Source commit `f2d2ac641ee67cd94cd2c57c9f5201b6e78841fc` builds one integrated
5355520-byte BOOTIMG. 66 final regressions, ARM ABI/shell checks, emitted
DT/config/memory/BOOTIMG validation and 12 package rejection cases pass.
[Build evidence and limitations](../build/evidence/y2linux-m4-01/README.md) ·
[Owner deployment and qualification](../build/y2linux-m4-01-deployment.md).

The implemented power scope uses standard frameworks; battery pack temperature,
measured current, capacity and full-charge determination remain unsupported.
Charging is inhibited and inherited limits/protection retained. Safe shared
OPPs, calibrated die zones, WFI/s2idle, Power/RTC wake and RTC/reset/poweroff
are candidate capabilities, not physical acceptance. Current internal p5/p7,
M3 startup fix, input/display/USB/audio and Buildroot contracts remain the
regression baseline; no root/data update is packaged. Storage06 is the fallback.

M4 #30 stays ACTIVE/PARTIAL, every physical checkbox open. #33 wider storage,
#29 narrow M3 residuals, #28 broader stability, #27 deferred reconnect, M0
backup/recovery/calibration residuals and M5/M6/application deferrals retain
their prior status. No memory expansion, boot-chain/table/calibration change,
new radio or application scope. Stop for the owner's manual BOOTIMG deployment;
do not cross into physical PM qualification until that deployment is reported.

### Entry audit

Owner explicitly authorizes a complete generic power-platform implementation,
one integrated candidate and read-only live SSH inspection, then STOP for manual
BOOTIMG deployment. Entry is local `ae5819f`, ten commits ahead of origin/main.
Fresh authenticated SSH confirms Storage06, four CPUs, MemTotal 954376 KiB,
internal p5 root/p7 data and no SD block device. The owner explicitly confirmed
preserving that current layout; the initial SD-root wording is superseded.

Reviewed the tracked source/config/DT/Buildroot/build/test inventory, local Git
history, all open issues #16/#27–33, Storage06 boot/SSH evidence, M2 core evidence,
M3 results and the complete coverage matrix below. GitHub's Storage03/SD-root
headers are stale compared with local hardware evidence. Canonical ALSA retains
the tested period-notification fix `137b3f2`; no solved audio path is repeated.
M3's remaining 48 kHz/LR/repeat and wider storage acceptance stay open, but the
owner now authorizes M4 progression. Reconnect #27 remains explicitly deferred.

[Current observations and architecture](../knowledge/m4-power-platform.md).
Only USB presence is currently a power_supply; thermal, cpufreq, cpuidle, RTC
and suspend interfaces are absent. Regulator/clock summaries report inherited
VPROC 1.15 V and ARMPLL 1.04 GHz. Source-reviewed, bounded regmap reads show
charging disabled, configured 70 mA/4.175 V and enabled PMIC thermal calibration;
these are not measured battery current or a validated charging policy.

**M4 #30 ACTIVE, entry PARTIAL / ready for implementation.** One shared PWRAP
owner, upstream MFD/regulator/RTC/reset/PM frameworks, checked ADC/thermal and
charging semantics, safe OPPs/WFI and suspend-to-idle are the coherent scope.
Unknown calibration/pack/OPP facts restrict the affected feature; they do not
justify fabricated telemetry or aggressive settings. No hardware success is
inferred from compiling. M1 complete, M2 narrow successes, all unchanged coverage
rows, M0 backup/recovery/calibration residuals, M5/M6 gates and application
deferral are retained. No physical flash or protected-partition write.

## Storage06 owner boot and SSH correction entry — 2026-09-13

### Completed owner flash and authenticated acceptance

At source `dcb7f45fd7fbe0adffc10195239f8df934491c8b`, the corrected initial Y2DATA
package passed exact existing-public-key validation, ext4 integrity/identity,
actual ARM blkid and eleven isolated package rejection checks. The owner then
reported the new image flashed. [Authenticated SSH evidence](../hardware-evidence/2026-09-13-storage06-owner/ssh-result.json)
confirms the requested identity now works, both internal filesystems mount
read-write, p5/p7 retain exact stock geometry, and no removable SD block device
is present. A complete 5193728-byte BOOTIMG read matches the Storage06 package
SHA256; all three 512-byte stock table hashes match. Buildroot handover on this
boot is at 8.728982 seconds. The live public-key file matches the local `.pub`
exactly. No private-key contents were inspected, copied, packaged or committed.
The regenerated server key required replacing the host's stale 10.42.0.1 public
known_hosts entry with the verified new key. The normal SSH invocation then
succeeds; strict host-key checks and all other saved host entries are preserved.

Reassessed every coverage row and phase gate against this new physical result,
the earlier ACM/readback/source evidence, the retained firmware/data package
checks and today's existing #16/#27–33 snapshot. Internal boot, normal block
reads and owner-key SSH are confirmed. Automatic root journal recovery and a
new persistent server host key are observed; deliberate scratch-write,
protected-region write, stress and power-fail qualification were not attempted.
The two synthetic ext4 inode warnings remain recorded; filesystem checks finish
cleanly. No table or existing Y2ROOT rewrite, alternate hardware path or further
kernel build occurred in the SSH correction.

[The deployed artifacts and limits](../build/y2linux-production-v1-r6-deployment.md)
replace the pending manual-flash boundary below. This integrated pass is done;
no further flash is required. #33 stays ACTIVE for its wider qualification,
so its existing GitHub/milestone status needs no transition and no GitHub write
was made. M1/M2 history, narrow M3 audio, deferred reconnect, backup/calibration
gaps, M4/M5 not started, M6 planned and application deferral remain unchanged.
No later phase starts automatically. The pre-initialization entry follows.

The owner has already flashed Storage06, reports that it works, and requests
SSH access/correction in this same production pass. The host observes Linux
6.18.0-y2linux-storage06 over USB ECM/ACM. After assigning10.42.0.2/24 to the
active USB connection, Dropbear at10.42.0.1 rejects the explicitly selected
existing owner key with `Permission denied (publickey)`. This establishes normal
root/service progression beyond the former rescue blocker; full authenticated
runtime/storage acceptance is still pending. The renewed ACL allowed [ACM capture](../hardware-evidence/2026-09-13-storage06-owner/result.json):
sector0/EBRs55aa, exact partition sizes, root/data mounted ext4rw and switch_root
at7.878653s. Four CPUs,954376KiB RAM, no removable block device in the snapshot.
No authenticated scratch-write or repeat/stress acceptance yet.

Read-only comparison proves that the original Y2DATA template contains a different
public key. The owner explicitly selects ~/.ssh/y2linux_ed25519.pub, forbids
reading/copying/packaging the corresponding private key, and confirms that only
initial system state exists on Y2DATA. Prepare a corrected data-initialization
template/profile; keep the successful BOOTIMG and existing Y2ROOT. No new kernel,
DT, transport, console or alternate hardware path is required. Authorization
stays in persistent Y2DATA, so ordinary system updates preserve it. This explicit
data initialization replaces initial state and lets per-device host keys be
generated again; it is never a normal preserving update or OTA action.

Reassessed all coverage rows and phase gates against this owner result, USB/TCP
evidence, the completed54-test/ARM/BOOTIMG checks and the current #16/#27–33
snapshot. Internal boot is newly observed; authenticated reads/bounded-write
acceptance remains pending. #33 remains ACTIVE; M1/M2 history, narrow M3 audio,
M4/M5 not started, M6 planned, deferred reconnect, backup/calibration gaps and
application deferral remain unchanged. The next boundary is one manual
USRDATA-only initialization, then SSH using the owner's existing identity.

## Stock address compatibility correction entry — 2026-09-13

Entry revision `d7ee4f2`. The owner's corrected readback completed at
21:10:43 CEST: all six samples match, including Storage05's BOOTIMG control,
the complete stock MBR/EBRs and both flashed ext4 identity prefixes.
[Evidence and limits](../hardware-evidence/2026-09-13-storage05-owner/readback-corrected-analysis.json).
The independently retained **actual FM kernel binary**, unlike the public donor
source previously reviewed, adds `(0x1400000 - other_hardware_capacity)/512`
to user-area block commands. DA reports 0x880000 bytes of other hardware areas,
so the missing translation is **23552 sectors**, with a 15203328-sector stock
disk view. [Source proof and production correction](../knowledge/storage06-addressing-correction.md).
This supersedes the missing-sector-evidence blocker and the assumption that the
public donor's zero offset describes this device. Tables must not be repaired.

Reassessed every coverage row against the new readback, actual stock binary,
Storage05 ACM/failure photo, historical DEV-02/AUDIO-02, current owner scope and
all eight open GitHub issues (#16/#27–33, read again today). Physical internal
boot remains failed; no capability is promoted from offline evidence. #33 and
milestone 3 already say ACTIVE/manual acceptance pending; no GitHub status
change is needed. M1/M2 historical core success, narrow M3 44.1-kHz audio,
deferred reconnect, backup/calibration gaps, M4/M5 not started, M6 planned and
application deferral retain their classifications and gates.

The owner-authorized integrated production pass now has concrete evidence for
one corrected BOOTIMG: restore the stock logical-to-native address translation
and visible capacity, enforce the same logical write spans before translation,
preserve real EXT_CSD bytes, and reuse the installed root/data images. No DT,
hardware/memory scope, separate diagnostic path, table write or next-phase
activation. The next blocker is validation of that correction, followed by one
owner manual BOOTIMG-only flash/no-SD boot; hardware acceptance remains pending.

## Storage05 readback observation update — 2026-09-13

The owner completed the prepared 13824-byte batch at 21:00:03 CEST. The
[retained result](../hardware-evidence/2026-09-13-storage05-owner/README.md)
shows that the assistant supplied physical offsets to a tool path operating
in legacy global DA mode (`NUTL_ADDR_LOGICAL`, `EMMC_PART_UNKNOWN=0`). All six
intended-prefix checks fail; the supposed Y2ROOT prefix exactly matches stock
SEC_RO at its global address. The BOOTIMG identity gate fails. This establishes
an acquisition-coordinate error, not intended table corruption or a Linux
offset defect. Current internal boot is still failed and ext4 contents on the
device remain unqualified.

The next smallest blocker is the same six samples at the
[corrected addresses for the observed mode](../build/storage05-readback.md).
Original bytes and logs are retained privately. No firmware build, source/DT
change, assistant device operation, GUI change, table repair, scope change or
milestone transition occurred. #33 stays ACTIVE and later gates are unchanged.
This is an observation update, not new hardware or repair authorization.

## Storage05 ACM observation update — 2026-09-13

Owner-established host access now permits [Storage05 LOG1 capture](../hardware-evidence/2026-09-13-storage05-owner/README.md).
578653 bytes retain contiguous kernel sequences 0–2735. The production eMMC host
reports sector addressing, 15269888 sectors and PART_CONFIG=0x48 (user-area access),
then CMD18 sector 0/signature0000 with successful SBC/R1. No eMMC guard rejection
or reported transport error occurs in this interval; rescue fails at 40.853244
seconds. Internal-root/write acceptance stays failed; physical MBR contents are
still unverified. This supersedes host-absence/access-pending statements below.

Existing LOG1 cannot supply raw sectors. The already identified independent
read-only evidence boundary is now a concrete [six-sample SPFT Readback plan](../build/storage05-readback.md),
13824 bytes including a known BOOTIMG address control and ext4 identity samples.
It is prepared only. No readback, firmware build, physical assistant block write,
reboot, flash, table repair, architecture change or milestone transition occurred.
All coverage classifications, #33 ACTIVE and later gates remain unchanged. This
is an observation update, not new hardware or repair authorization.

## Storage05 owner result boundary — 2026-09-13

The [owner's Storage05 photo](../hardware-evidence/2026-09-13-storage05-owner/README.md)
arrived after building the single candidate and supersedes the pending-flash
boundary below. Reassessed all coverage rows against it: CMD18 sector0 still
returns signature0000 with4096 reported bytes, SBC8/00000900 and R1=00000900;
normal internal root/data boot fails. Software SBC sequencing alone did not
correct the physical problem. No metadata corruption, on-device ext4 integrity
or whole-image readback is established. No physical status is promoted.

Source/ARM defects and offline results stand, with their limits retained. #33
and its existing GitHub status remain ACTIVE; every later gate is unchanged.
The next smallest blocker is independent read-only sector evidence, with address
semantics checked against a known-content control. Host currently has no Y2
USB/ACM/network device; the owner was asked to connect the running rescue for
existing LOG1 logs. This does not authorize table repairs or a speculative second
candidate. No further firmware build, physical assistant write or phase activation.

## Storage05 manual BOOTIMG boundary — 2026-09-13

Reassessed every coverage row against the same owner-observed Storage04 ACM
capture, retained working SD AUDIO-02, current owner scope and open #16/#27–33.
No newer physical result exists. The single [Storage05 production candidate](../build/y2linux-production-v1-r5-deployment.md)
is built from `5401007791dc2d915c95baac31e91fd062c8edc0`. Source/ARM execution
reproduces broken one-block CMD25 encoding, falsely successful DMA fault handling
and the fatal rescue `set -i`. The exact physical cause of sector-zero `0000`
remains unproved; neither table corruption nor an AutoCMD23 silicon defect is
established. Explicit software SBC, corrected error handling and the valid exec
handover are the integrated production correction, awaiting hardware acceptance.

52 regression tests, actual ARM ABI/shell checks, kernel/module/archive/layout
validation, ext4 identity/integrity and BOOTIMG manifest/bounds/hash checks pass.
Eight isolated package mutation cases are refused. The 71-node hardware DT and
reservations match Storage04 except calculated initrd-end; kernel configuration
differs only in LOCALVERSION. Exact stock MBR/EBR geometry, root/data bytes,
runtime write spans, module ownership and y2-platform-v1 stay unchanged.
No root/data build, new diagnostic firmware, physical assistant write or SPFT
operation occurred. Unchanged ROM/recovery provenance was not repeated.

#33/milestone 3 stays ACTIVE, as already recorded on GitHub; no status update or
new epic is needed. Native internal read/write and no-SD Buildroot acceptance are
still pending. M1/M2 historical core results, narrow M3 44.1-kHz audio and every
later power/radio/GPU/reconnect/stability/application gate retain their status.
The next boundary is one owner manual BOOTIMG-only flash and one no-SD boot.
Fallback restores the observed Storage04 rescue/ACM, not a working internal root.
Stop here; no further build or phase activation follows this documentation audit.

## Integrated production MMC correction entry — 2026-09-13

Owner authorizes one production fix pass and one candidate, stopping before a
manual flash. Entry source is `37e4a3f` plus the retained live-evidence/build-input
changes already in the workspace. Reassessed every coverage row below against
the [Storage04 ACM capture](../hardware-evidence/2026-09-13-storage04-live/README.md),
the last working SD-backed AUDIO-02, current source, and open GitHub issues
#16/#27–33 (read again today). #33/milestone 3 remains ACTIVE; the GitHub status
already matches. M1/M2 historical successes and M3's narrow 44.1-kHz result stand;
all power/radio/GPU/reconnect/stability/application gates remain unchanged.

Storage04 identifies 15269888 sectors, four CPUs and 954380 KiB RAM, but returns
sector-zero signature `0000`, exports no partitions and stays in rescue. This is
not proof of erased tables. ACM is absent at this pass's host inspection; its
existing LOG1 protocol provides logs, not arbitrary remote block reads. No new
physical write or new diagnostic firmware is authorized or performed.

The next boundary is a BOOTIMG-only production correction: audit the actual MMC
request/response/DMA path, retain normal bounded root/data writes and all protected
regions, execute the stock parser against the retained tables, and check the
existing ext4 identities and internal-only rescue handover. Preserve layout 1,
the exact stock ANDROID/USRDATA geometry, existing root/data bytes, DT hardware,
module ownership and y2-platform-v1. No parser replacement or MBR/EBR rewrite
without evidence. Offline tests must distinguish reproduced source defects from
the still-unverified physical cause. Re-audit at the manual-candidate boundary.

## Storage04 live observation — 2026-09-13

Read-only [hardware inspection](../hardware-evidence/2026-09-13-storage04-live/README.md)
after the owner's r4 flash establishes Linux storage04, CPUs 0–3, MemTotal
954380 KiB, initial USB composite enumeration and bounded ACM LOG1 reception.
Internal MMC is detected at 15269888 sectors, but no partitions are exported.
The driver observes sector-zero signature `0000`; rescue explicitly reports
missing/invalid internal root/data at 41.163161 seconds. Normal Buildroot/SSH
and internal-root/data acceptance remain blocked. Actual table bytes/addressing
are not independently read back; do not infer physical erasure or authorize table
repair from this signature alone. USB Ethernet and reconnect remain unqualified.

Owner attributes the earlier off-state failure to an empty battery. Available
telemetry reports USB ONLINE only; no battery voltage or charging measurement.
Charging management remains absent. Storage #33 remains ACTIVE and later gates
remain unchanged. This is an observation update, not a milestone-boundary audit,
scope expansion, implementation authorization or M4 activation. It supersedes
older pending-flash/no-USB statements for the observed Storage04 boot.

## Single production platform correction audit — 2026-09-13

Owner manual Storage03 flash completed, but the newest rescue photograph shows
missing internal Y2ROOT/Y2DATA, no partition rows, USB preflight -19, zero polls,
and recovery written=0. This supersedes the pending-flash entry below. Linux and
rescue/display run; normal internal Buildroot and USB are **not qualified**.
Cable-dependent/off-state response was reported before Storage03 too. Battery
voltage is unknown; Y2Linux has no implemented/qualified charging manager.

Rechecked the coverage matrix against this result, DIAG01, Storage02, and retained
AUDIO-02 results. Storage #33 remains ACTIVE. M1/M2 and clean S16/44.1-kHz audio
stand as historical physical results; 48 kHz, repeat/LR, USB reconnect, stability,
power and radios retain their gaps. No M4/M5/application activation or closure.

The owner explicitly authorizes replacing parallel bring-up hardware modes with
one production platform. Active drivers, DT and root resolver must be shared by
normal/rescue/diagnostic userspace. Preserve permanent hardware support and
bounded write protection; remove identity-only MMC and unplugged USB semantics.
Execute the upstream partition parser against retained stock metadata before
considering any compatibility change. Do not rewrite tables or infer current
raw-media contents from factory files. Instrument failures through production
interfaces rather than another diagnostic firmware. Keep layout/data schema 1,
OTA architecture and protected boundaries unchanged. Build a uniquely named
successor to the already-flashed Storage03, validate the integrated package, and
stop for manual owner flashing. BOOTIMG will own the matching DRM module and
stage it in RAM for normal /lib/modules access. The rootfs gains only platform
startup/status tools, removing the current exact-kernel module dependency from
future rootfs updates. This changes packaging ownership, not the DRM driver or
storage geometry; validate module ABI, archive, handover and release contracts.
No physical assistant writes or SPFT execution.

## Storage03 complete manual boundary — 2026-09-13

The [complete Storage03 package](../build/y2linux-production-v1-r3-deployment.md)
is built from 4845860 and passes 33 regression tests plus a linked ARM USB test,
ARM userspace/reader/filesystem checks and full image/layout/package validation.
Partition layout 1, data schema 1 and root/data runtime write ranges are retained.
No further hardware evidence is available; this remains a correction candidate,
not physical acceptance. The truncated rejected MMC command and off-state power
entry cause remain unproven. No protected/table writes, physical assistant
operation, OTA/player work, M4/M5 activation or milestone closure occurred.
Storage remains ACTIVE and stops for owner manual SPFT Download Only operation.

## Storage03 correction scope — 2026-09-13

Latest owner DIAG01 photos confirm internal MMC, nonremovable, 15269888 sectors,
no exported partitions, rejected storage commands, and USB PREFLIGHT -19 with
zero polls and initial CHRDET=007b. The display truncates the command/argument;
no specific rejected opcode is established. Owner reports cable-dependent power
entry and apparent SD-dependent progress, and cannot provide more device data.

Correct the production storage request boundary and cable-present PHY startup;
keep stock loader/table/calibration untouched and the same root/data write ranges.
Quiet expected empty-slot console traffic and retain useful rescue status. This
is authorized production correction, not M4 power/charging qualification or a
new full USB milestone. No evidence yet attributes off-state power entry to a
specific image; do not claim a software or physical fix without acceptance.
Build a complete three-payload candidate with targeted request/PHY fault tests
and full production package validation. Owner remains the only physical writer.
Storage stays ACTIVE; later milestones and M3 qualification remain unchanged.

## DIAG01 manual observation boundary — 2026-09-13

The [BOOTIMG-only diagnostic](../build/y2linux-storage-diag-01-deployment.md)
passed seven targeted tests and ARM/newc/DT/D08/BOOTIMG/selection/hash checks.
The exact Storage02 kernel/module and all hardware/write policies are unchanged.
Owner photos establish failed internal-root discovery and Linux display; host
still has no Linux USB capture. Hardware coverage and later gates remain as in
the preceding audit; no new root/USB success is established by offline tests.
Storage stays ACTIVE and pauses at the owner manual diagnostic flash boundary.
No assistant physical write or SPFT execution. Do not start later milestones.

## Storage02 diagnostic BOOTIMG preparation — 2026-09-13

A later owner startup/USB attachment still yields no Linux USB enumeration.
The established unplugged-start sequence alone has not restored observation;
its actual device-side USB status is unknown. Repeated button/cable trials are
not useful without additional evidence. The rescue photo confirms failed
internal root/data discovery, not its cause.

Prepare a bounded BOOTIMG-only diagnostic with the exact Storage02 kernel,
module, controller configuration and command guard. Replace only rescue PID1
with an unmounted-filesystem observation loop and reduce console verbosity so
empty removable-slot warnings cannot bury status. Display cached USB diagnostics,
internal block geometry, approved root/data signatures and relevant driver logs.
No root/data rewrite, filesystem repair, new hardware controls, changed write
allowlist, OTA implementation or physical assistant operation. Owner flashes.

Storage stays ACTIVE; no internal-root/USB acceptance or milestone closure.
M3 physical results and later power/radio/GPU/reconnect/stability/app gates remain
unchanged. Targeted new reader/ARM/shell/DT/newc/D08/BOOTIMG/selection checks are
required before the manual diagnostic boundary. No repeated ROM provenance or
M1/M2/M3 bring-up is justified.

## Storage02 rescue failure observation — 2026-09-13

Latest owner photos establish Linux display with no SD, then rescue's aggregate
missing/invalid internal root/data failure with SD present. Full internal-root
boot has failed this attempt; recovery PID1 remains alive. USB remains unobserved.
Source review recovers the existing boot-unplugged / attach-after-startup contract
and corrects the assistant's incompatible cable-present startup instructions.
See [rescue evidence](../hardware-evidence/2026-09-13-storage02-owner-flash/rescue-console.md).
No milestone closure or new hardware scope; storage remains active and every
later milestone retains its prior gate. Next work is read-only ACM observation.

## Storage02 owner transfer result — 2026-09-13

[New physical/operator evidence](../hardware-evidence/2026-09-13-storage02-owner-flash/README.md):
SPFT Download Only transfer completed successfully. Owner reports response after
holding Power + Volume Up following a black start. No SD and connected USB are confirmed. Resulting boot stage is unspecified;
host sees brief preloader appearances but no Linux USB/ACM/Ethernet. This qualifies the raw
transport past its earlier failure, not internal boot or protected-byte integrity.
Storage #33 remains ACTIVE; no milestone boundary is crossed. All remaining
hardware acceptance and later M4/M5/platform/application gates are unchanged.
No code change, build, assistant flash or repeat bring-up is warranted without
identifying the current running state. See the handoff for pending observations.

## Storage02 manual retry boundary audit — 2026-09-13

Source/build `c2db4893b79eca25458b6adc318232c80dd99872` produces the
[corrected raw-image package and full checklist](../build/y2linux-production-v1-r2-deployment.md).
26 regression tests, ARM ABI/ALSA/shell checks, kernel/DT/BOOTIMG, filesystem,
stock geometry, manifest/hash validation and synthetic readback rejection pass.
The isolated scatter test now carries the hash-checked original fixture.

Reassessed the coverage matrix against the newer owner failure: BOOTIMG transfer
completed; ANDROID failed; USRDATA was not reached. This supersedes any statement
below that AUDIO-02 is necessarily still deployed. No native eMMC filesystem,
no-SD boot, protected preservation or rescue-negative physical acceptance exists.
M3's clean 44.1 kHz and remaining 48 kHz/LR/repeat gaps are unchanged. Power,
radios, GPU, USB reconnect, stability and whole-platform gates remain unchanged.

Production Storage v1/#33 remains ACTIVE, awaiting manual retry and physical
qualification. All stock boundaries, restoration sources and protected write
exclusions remain unchanged; no further source/ROM/recovery audit is justified.
Use only out/y2linux-production-v1-r2, including its new BOOTIMG. Before/after
readbacks taken only after the failure cannot prove the prior attempt preserved
all protected bytes. No assistant flash or physical write was performed.
M4/M5 and Y2PlayerNative remain deferred. Stop at this owner operation boundary.

## Storage flash failure / transport correction audit — 2026-09-13

[Owner flash evidence](../hardware-evidence/2026-09-13-storage-flash-failure/README.md)
supersedes the package-ready state below: BOOTIMG transfer checksum passes,
ANDROID sparse transfer fails3154, USRDATA not reached. Selected MT6582 DA's
parser rejects FILL chunks. First package is withdrawn from retry use. Physical
acceptance remains pending; all other hardware coverage rows retain their prior
limits. No kernel/root boot success or protected-byte readback is established.

DA physical user capacity differs from historical Android exported capacity;
update exact capacity checks for both observed views without reclaiming the tail
or changing a partition boundary. Normal write spans remain ANDROID/USRDATA only.
Use raw ext4 transport to define every byte without the unsupported sparse path.
#33 remains active; M3/M4/M5 and platform/app gates unchanged. This is a bounded
failure correction with targeted storage/packaging/BOOTIMG validation, not a
repeat of M1/M2/M3 bring-up. Owner alone performs the retry; no assistant flash.

## Production Storage v1 manual-package boundary audit — 2026-09-13

Implementation/build commit `f2ef297a81cc0229155ba5d96f1955bdd230f11d` now yields the
[complete manual deployment package/checklist](../build/y2linux-production-v1-deployment.md).
Re-reviewed every coverage row against the live AUDIO-02 entry, retained DEV-02/
M3 results, current open issues#16/#27–33, stock table/scatter and recovery sources.
24 regression tests, kernel/DT/BOOTIMG checks, root/data filesystem integrity,
manifest/scatter/sparse/hash checks and synthetic readback rejection pass.
No new production hardware result or protected-data readback is claimed.

**Production Storage / Installation v1 ACTIVE / manual owner operation pending**
([#33](https://github.com/SchulzCode/Y2Linux/issues/33), GitHub milestone3).
Internal root/data implementation is ready for its first manual test, not closure.
M3 remains open with clean44.1kHz;48kHz/LR/repeat remain pending. eMMC is disabled
in the currently deployed kernel. Live MemTotal954384KiB/four CPUs/SD/ALSA are
confirmed; current no-SD/internal-write/rescue-negative acceptance is absent.
All other hardware gaps remain unchanged and tracked. M4 follows storage, then
M5, remaining GPU/USB reconnect/stability, final qualification, then native app.

Overwritten-partition FM factory sources are available/hashed; personal Android
state and independent recovery retention remain explicit operator choices/steps.
Before/after bounded protected/table readbacks and actual address verification
belong to the owner manual boundary. The assistant has performed no eMMC write,
SPFT operation, kernel deployment, OTA updater or Y2PlayerNative implementation.

## Production Storage v1 activation audit — 2026-09-13

Authoritative current scope, superseding the historical phase order below.
Baseline `1583360`; all current open GitHub issues #16 and #27–32 reviewed.
Every coverage row below was reassessed against retained DEV-02 and AUDIO-02
physical results, the stock 21-entry map, current source/config and owner direction.
[Read-only live entry](../hardware-evidence/2026-09-13-production-entry/README.md) confirms AUDIO-02, four CPUs, MemTotal954384KiB, SD root and ALSA; eMMC remains disabled in current DT.
M1/M2 narrow successes, M3 clean44.1kHz and all power/radio/GPU/USB reconnect gaps
remain unchanged. 48kHz, L/R and repeated playback remain pending; M3 stays open.

**Production Storage / Installation v1 ACTIVE**, inserted after current M3 work
and before M4 by explicit owner authorization. Ready for offline implementation
and package validation; physical acceptance pending manual owner deployment.
[Partition audit and exact classification](../architecture/production-storage-v1.md)
select stock ANDROID for Y2ROOT and USRDATA for Y2DATA. No repartition, protected
write, live eMMC write, SPFT execution, updater or native app implementation.

Coverage changes: internal eMMC remains PARTIAL (native identity exists; native
block/mount/write not yet qualified); internal rootfs/data, release manifest,
rescue, installer/update contract become ACTIVE implementation scope. Buildroot
remains canonical. Recovery retains verified stock FM sources for overwritten
partitions, with personal Android-data backup and independent retention explicit
operator obligations. No full-ROM backup or provenance repeat is imposed.
All other matrix statuses and tracked gaps were reviewed and remain unchanged.

Sequence: current M3 acceptance remains open → Production Storage v1/manual
no-SD acceptance → M4#30 power → M5#31 radios → remaining platform GPU/lima,
USB reconnect#27 and stability#28 → whole-platform qualification → Y2PlayerNative.
#32 continues broader lifecycle/security ownership; [storage #33](https://github.com/SchulzCode/Y2Linux/issues/33)
tracks this bounded activation. Audit again at package/manual-deployment boundary;
do not call installation successful until physical acceptance is retained.

## Current session-close audit — 2026-09-10

This was the authoritative session-close status; dated candidate/activation sections
below are historical. Reconciled owner reports, source commits, retained core
and AUDIO-02 logs/build metadata, and current GitHub#22–32. Every coverage row
was reviewed: the audio, PCM/rootfs tools and phase rows change below; remaining
power/radio/GPU/production gaps keep their prior scope. No new phase is activated,
closed or implemented tonight. No build, flash or device change in this audit.

- **M1 COMPLETE.** Linux6.18 and native boot objectives achieved.
- **M2 core/Buildroot qualified for progression.** #22–26 confirmed closed;
  four CPUs, bounded RAM pass, writable SD/SSH, display/input and buses remain
  proven. #27 reconnect stays open/deferred under#28. Initial ACM/ECM/SSH and
  owner startup/restart recovery work; do not label that cable reconnect success.
- **M3 ACTIVE / NEAR COMPLETION.** AUDIO-02 `8f93e44` contains startup fix
  `137b3f2`, is owner-deployed and has clean S16 stereo44.1kHz headphones.
  3.13s/130IRQs/advancing ALSA pointers and owner "perfect, no clicking!" support
  the fix. Persistent ALSA tools are installed on SD. Remaining small checks:
  48kHz, explicit L/R, stop/restart/repeat and associated noise/reset/XRUN checks.
  Do not redeploy the already validated fix or rediscover AFE/CON3/CS43131/VGP2.
- **M4 NOT STARTED.** Next after M3 short qualification and a boundary audit:
  battery/charging/thermal/DVFS/idle/suspend/wake/reboot/poweroff.
- **M5 NOT STARTED.** Wi-Fi/Bluetooth remains later; no native app starts.
  GPU/lima and final reusable-platform qualification still precede Y2PlayerNative.

DEV-02 measured MemTotal954660KiB; AUDIO-01 measured954384KiB with its larger
kernel. Do not turn either into954MiB or invent a fresh AUDIO-02 /proc/meminfo
measurement: AUDIO-02 retains the same bank/reservations and comparable boot
memory log, while this closeout performs no device inventory. Physical bank is
992MiB; exclusions leave about951MiB before kernel/page metadata reservations.

[Current physical audio result](../knowledge/m3-audio-01-live-result.md) ·
[Core qualification](../knowledge/y2linux-dev02-live-qualification.md) ·
[Exact next-session handoff](session-handoff-2026-09-10.md).

Native24/32-bit and higher rates are not implemented.32-bit I2S slots carry16-bit
PCM. These are later enhancements; amp/speaker identity/output, jack and precision
clock checks remain separately tracked under#29, not grounds to reopen the basic
headphone proof. Before M3 core closure explicitly retain those subpath gaps and
re-audit before M4; clean one-run audio alone is not full milestone completion.

## M3-AUDIO-01 manual-deployment boundary — 2026-09-10

[Integrated result](../build/y2linux-m3-audio-01-result.md) now passes offline
build, thirteen targeted tests, resolved audio/rail DT, expanded RAM/BOOTIMG and
rootfs checks. Reconciled every coverage row against the same physical DEV-02
baseline and owner authorization: no new audio hardware result exists. Core
acceptance stands, #27/#28 reconnect remains deferred, #29 M3 stays active/open;
M4/M5/GPU/final-platform gates and Y2PlayerNative deferral remain unchanged.
The owner next updates removable SD content and manually flashes BOOTIMG only.
No assistant flash/reboot, internal-storage write or repeat M2 pass occurred.

## M3 activation audit — 2026-09-10

The owner explicitly authorizes one integrated native-audio implementation/build
and manual BOOTIMG deployment, using physical DEV-02 as the baseline (checkpoint
`d76e57f`). This supersedes the reconnect prerequisite below. M2 core/Buildroot is
substantially qualified; #27 and #28 retain USB reconnect for later whole-platform
qualification. Do not repeat reconnect experiments or reopen satisfied #22–26.

Reconciled the coverage matrix and current open issue snapshot with the retained
[physical pass](../knowledge/y2linux-dev02-live-qualification.md) and a narrow SSH
entry check. Four CPUs, 954660 KiB MemTotal, bounded 256 MiB RAM pass, writable SD
Buildroot, protected eMMC, initial SSH/ECM/ACM, visible display, wheel/buttons and
PMIC/core buses retain their physical acceptance. Audio remains UNTESTED; VGP2's
live selector is 2.5 V with no consumer and must be constrained to 1.8 V before DAC
operation. No sound card exists in DEV-02. No new power/radio/GPU acceptance exists.

**M3 ACTIVE (#29)**: ALSA/ASoC DL1 DMA/IRQ -> second I2S CON3 -> upstream CS43131,
regulator-owned power and controlled headphone playback. Speaker identity/routing,
jack reporting and higher rates remain separately qualified subpaths. One clean
kernel build, one audio-equipped Buildroot build, relevant static/DMA/config/DT
checks and current layout/BOOTIMG validation. Stop for owner BOOTIMG-only flash;
no internal storage, loader, calibration or partition writes. M3 remains open until
real clean headphone playback, repeated start/stop and rate tests succeed.

M4 power, M5 connectivity, GPU/lima and final whole-system qualification remain
subsequent platform gates. Y2PlayerNative remains deferred. Unchanged provenance,
recovery research and M2 qualification are not repeated.

## DEV-02 live qualification boundary audit — 2026-09-10

Owner-authorized complete live pass on physical Linux `6.18.0-y2linux-dev02`,
source checkpoint `a34a360` plus preserved DEV-02 working changes. Live DT hash
matches DEV-02; full flashed BOOTIMG was not read back. Reassessed every coverage
row, current #16/#22–32, retained baseline, raw evdev, RAM tests, DRM/sysfs and
host disconnect evidence. [Complete report and evidence](../knowledge/y2linux-dev02-live-qualification.md).

Buildroot PID1 runs on writable removable Y2ROOT, four CPUs online, MemTotal
954660 KiB. The completed 256 MiB allocator test passes; a separate exhaustive
run was capped at 600 seconds without observed memory errors. All eight buttons
and both wheel directions produce evdev events; wheel/I2C IRQs agree and no
transport timeout remains. Owner confirms native display pattern after standard
fb0 unblank. Internal eMMC is disabled and unexposed. PMIC/core buses, initial
ACM enumeration, key-authenticated SSH and USB Ethernet work.

**USB reconnect FAILED.** The host sees removal, then no enumeration after owner
reconnect. The owner reports USB works only after restarting; no post-reconnect
uptime/continuity measurement exists. Do not perform the second cycle or close
M2. The generic core/Buildroot foundation is substantially qualified, with the
real #27/#28 reconnect blocker replacing the old research sequence. A later
shared-framebuffer checksum mismatch is corrected by hiding/restoring the cursor;
visible display acceptance and independent allocator-test success stand.

Safe runtime/module/config changes are represented in canonical Buildroot/host
sources and retained readbacks. Missing evtest/modetest and longer syslog records
are configured for the next userspace build, without triggering a rebuild.
Close satisfied #22–26 narrow slices; retain long-run/DMA, full power, routing,
production and final platform concerns under #29–32. Optional physical UART #16
remains separate, not an M2 closure prerequisite. No new kernel/BOOTIMG, flash,
assistant-initiated reboot, internal storage access or unchanged provenance qualification occurred.

M1 COMPLETE; M2 ACTIVE solely around remaining real qualification/fixes, chiefly
USB re-entry. M3 audio is the next platform phase once that foundation is usable;
M4 full power and M5 connectivity follow with shared dependencies. GPU/lima and
final whole-system qualification precede Y2PlayerNative under the owner's new
explicit platform-first gate. No later hardware phase is activated by this audit.
After the failed reconnect the owner restarted the device; SSH returns at33.32s
with the old tmpfs token missing, confirming a new boot. Runtime fixes persist
and actual ACM capture now works after the host rule. A bounded SD recorder is
installed/tested so a future failure log can survive loss of USB.
A USB-focused DEV-03 requires device-side failure evidence and a justified change;
host absence alone does not identify which guard/callback failed.

## DEV-02 root handoff / wheel correction audit — 2026-09-10

Owner's new photograph confirms SD partition mmcblk0p1, ext4 recovery, exact
Y2ROOT UUID, writable remount and handoff attempt. PID1 then dies by SIGILL
(exitcode4) at23.723s. Visible fbcon remains positive. SD block/filesystem access
now has physical evidence; Buildroot init/SSH do not. The raw fault PC is absent.
All other coverage rows retain their latest evidence and open gaps; no milestone
closes, M2 stays active. No loader/partition/internal-eMMC write or new hardware
subsystem is authorized. Preserve the observed RAM/display/SD configuration.

DEV-02 combines standard ARM userspace Thumb/signal and kuser compatibility,
framebuffer device-node availability, userspace fault reporting/preflight, and
an evidence-backed MT6582 I2C combined-message correction. v6.18 selects WRRD
only with auto_restart, but MT6582 uses the older non-auto-restart compatible;
DEV-01 logs op1/transactions2 with only one byte of completed TX DMA. Restore
combined WRRD for this exact compatible and test the controller-level selection,
without changing APT32F packet framing or adding retries.

Use one new kernel build, existing pinned Buildroot binaries, targeted regression
checks and fresh actual layout/BOOTIMG validation. Keep the already written SD
rootfs usable; the new DRM module loads from rescue before switching. A later
SSH module-index refresh can bring the SD module directory to the new kernel.
Stop at owner BOOTIMG-only flashing, then collect all independent failures again.

## DEV-01 retained rescue/RAM/display checkpoint — 2026-09-10

[Capture-02](../knowledge/y2linux-development-hardware-result.md) now retains all
704 kernel records, visible-console supporting scanout/vblank state, four CPUs,
MemTotal954668KiB and HighTotal228352KiB. RAM/HIGHMEM visibility is physically
observed; stress and full DMA containment remain open. Wheel remains regressed
with ten I2C timeouts; the test-pattern tool cannot open/query fb0. Rescue fallback
is confirmed without Y2ROOT; SD boot/ECM traffic/SSH/reconnect remain unqualified.
M2 ACTIVE/exit incomplete, later gates unchanged. Transfer the existing rootfs to
the owner's Mac, identify its removable card, then prepare exact manual commands.
No hardware scope expansion or new build is needed for this transfer.

## DEV-01 owner-visible console update — 2026-09-10

The owner reports a visible Linux console after manually flashing DEV-01; SD
rootfs is not installed. Host USB descriptors independently identify Linux
6.18.0-y2linux-dev01 and composite ACM+ECM enumeration. This is positive visible
output, superseding the prior black-screen outcome for this boot; display remains
PARTIAL pending broader qualification. [Evidence and host access limitation](../knowledge/y2linux-development-hardware-result.md).

No new RAM, wheel, SD-root, network, SSH or reconnect success is inferred.
M1 COMPLETE; M2 ACTIVE/exit incomplete, later phases unchanged. Continue the
already authorized removable-SD preparation after exact host device identification;
no assistant write or guessed disk target. The existing DEV-01 BOOTIMG can remain.

## Y2LINUX-DEV-01 manual-deployment boundary audit — 2026-09-10

Offline implementation and the combined candidate are now built and validated:
[DEV-01 result](../build/y2linux-dev-01-result.md), [memory reconciliation](../knowledge/development-memory.md).
Rechecked every hardware coverage row against the retained BASELINE-03 capture
and owner observations. No newer physical capture exists, so **no physical
classification or milestone exit is promoted**. M1 COMPLETE; M2 ACTIVE/exit
incomplete; audio/application, radio, product PM/security and later milestones
retain their gaps and owners.

The owner-specified ED25519 public key is packaged; no host private key was
accessed. Kernel/module, large-memory DT, rescue archive, actual pinned Buildroot,
ext4/tar content and key policy pass offline validation. Rootfs/ECM/SSH now have
built implementations; their physical state remains untested. SD writes and
BOOTIMG flashing remain owner-only. Next action is one manual deployment and
combined visible-display, wheel, RAM, SD, persistent USB/network/SSH collection.
No unchanged ROM/recovery provenance or duplicate clean build was repeated.

## Y2LINUX-DEV-01 integration scope audit — 2026-09-10

Fresh-session local baseline **2efcdc1**, clean main; current open issues #16,
#22–32 agree with the local BASELINE-03 checkpoint. Reviewed all coverage rows,
retained capture-02 (102314 bytes, all 606 kernel records, 180 seconds), source
and artifact metadata. No new hardware success is claimed. Owner confirms an
earlier wheel-working integrated baseline; its exact artifact identity is not
established in the retained canonical captures. Treat current eight -110 reads
as an integration regression and preserve register-zero repeated-start framing.

The owner now explicitly authorizes **one integrated development candidate**:
post-modeset display fixes/diagnostics at 480x360; wheel transport correction;
large RAM with explicit reservations and HIGHMEM; pinned ARMv7 hard-float glibc
Buildroot; removable-SD ext4 LABEL=Y2ROOT; rescue initramfs; persistent ACM+ECM;
key-only Dropbear. This supersedes D08 as the final target, identity-only SD and
temporary USB disconnect deadlines. Internal eMMC remains protected; no assistant
flash, SD write, loader/partition/calibration/Android write is authorized.

Every physical coverage classification below remains unchanged. Display remains
PARTIAL with failed visible output despite successful DRM initialization. Wheel
remains PARTIAL with working historical owner observation and current transport
regression. RAM, removable SD and USB are PARTIAL; Buildroot/rootfs/SSH are
PLANNED pending implementation/build and then physical qualification. SMP and
buttons retain their narrow successes. All power/thermal, audio, radio, recovery,
production security, time/entropy and application gaps retain their existing
owners and limitations. M1 COMPLETE; M2 ACTIVE/exit incomplete; M3/application
and later product phases are not activated. Existing #22–28 own this work;
#30 retains sustained-workload/power qualification and #32 production policy.

Proceed offline together, preserving attribution/identity and unchanged donor,
ROM and recovery provenance. One clean kernel tree, one pinned Buildroot build,
changed-subsystem tests and full new memory/layout/BOOTIMG/rootfs validation.
Record exact artifact hashes and remaining physical tests. Stop for owner manual
SD identification/write and BOOTIMG-only deployment. Permanent USB observation
is a development capability, not sustained power/thermal qualification. Re-audit
before milestone closure using the subsequently collected real hardware results.

## BASELINE-03 hardware evidence update — 2026-09-10

Owner capture-02 restores complete kernel/PID1 observation for180seconds, including
successful live-PHY adoption, DSI park, cold Linux PHY initialization, DRM/fb0
registration and module return0. The owner still sees black: visible display
acceptance remains failed. Four CPUs and existing navigation/volume/power input
operate. Wheel EINT55 now fires, but eight I2C reads time out with no controller
completion IRQ; transport debugging replaces the previous unobserved-wheel gap.
[Retained result](../knowledge/m2-baseline-hardware-result.md#latest-baseline-03-logs-the-display-handoff-screen-remains-black).

Next combined source iteration: diagnose actual display output and wheel I2C
completion, include standard runtime state/failure diagnostics and repair snapshot
line framing. Preserve the working single USB writer. No guessed rate/rail/panel
changes, repeated per-register images or milestone promotion. M1 COMPLETE;
M2 ACTIVE, exit incomplete. D08, storage/PMIC firewalls, owner300s limit and later
milestone exclusions remain. This is an evidence/documentation update, not a
new build, hardware scope authorization or five-minute/reconnect qualification.

## BASELINE-03 observation and packaging review — 2026-09-10

BASELINE-02 owner reports grey/black after LK. The retained 180-second capture
identifies baseline2 at high-speed ACM, sends LOG1, but receives zero bytes
(exit2, no protocol header). No kernel/PID1 execution point or display failure
instruction is established. This is a regression of observation, not display
success. All newly tested BASELINE-01 results remain historical, not silently
revalidated for BASELINE-02.

Same M2 hardware scope, revised **initramfs packaging/startup**: compile the
MediaTek DRM aggregate as the sole loadable module, with required DRM helpers
built in. Package the exact module with PID1 in the existing bounded RAM-only
initramfs. Start it once through finit_module in a separate child after LOG1 is
active; PID1 must never wait synchronously for display probing. Keep PID1 on CPU0
and the load child on previously observed CPU1 using normal scheduler affinity;
this reduces local CPU starvation, not global bus/lockup risk. Kernel/driver logs
still use the unchanged /dev/kmsg-to-CDC relay, without competing USB writers.
Repair unpowered DSI host-transfer access and add phase messages. No guessed
panel/clock/rail changes or extra per-register image.

Audit module dependencies, ABI/exports and exact initramfs membership, include
module bytes in the D08 unpacked-file/layout accounting, test child errors and
nonblocking reap, then one clean combined build/package. No second reproducibility
build or unchanged ROM/recovery/source-tree hashing. D08, protected partitions,
PMIC/storage write firewalls, five-minute owner limit and M3/M4/M5 gates remain.
M1 COMPLETE; M2 ACTIVE; stop at BASELINE-03 for manual owner BOOTIMG flash.

## M2 integrated hardware checkpoint — 2026-09-10

[BASELINE-01 physical results](../knowledge/m2-baseline-hardware-result.md): two
complete initial kernel captures confirm four CPUs, shared PWRAP/MFD/regulator
probes, all five navigation channels, both volume keys, PMIC power key through
EINT25, and native microSD/eMMC identity. DSI rejects the inherited PLL before
handoff; wheel rotation has no captured IRQ/transfer/events. These are partial
core results, not M2 closure. M1 COMPLETE, M2 ACTIVE; M3/M4/M5 gates unchanged.

Continue one combined source iteration: repair live-PHY adoption and capture
window defaults; retain D08, PMIC/storage write firewalls, USB logging, the
295-second USB deadline and owner 300-second limit. The five-minute disconnect
report is consistent with the deadline but not captured in the 45-second files.
No memory/rail/production scope expansion or independent per-register flash.

## M2-BASELINE-01 integration authorization — 2026-09-10

Baseline **4dde305** (clean main). The owner explicitly supersedes per-register
and per-driver flash gates with one coherent offline Linux 6.18 core integration.
Reviewed canonical config/DT/PID1/USB, retained USBACM-03 and D08 evidence,
the 36-commit donor and additional history against every coverage row below.
No new hardware result exists: M1 COMPLETE, M2 ACTIVE/exit incomplete, M3 gated,
M4/M5 deferred. No coverage status is promoted by compiling a driver.

Implemented at entry: CPU0/GIC/GPT, early stopped AP watchdog, guarded LK text,
PIO CDC ACM with kernel/PID1 relay, offline USBACM-04 lifecycle and polled
navigation GPIOs. CCF/pinctrl/EINT/I2C/wheel/PMIC MFD/regulators/keypad/DRM/MMC/SMP
are documentation or donor source only. The two previous offline candidates no
longer require separate flashes before the combined baseline.

Integrate evidenced controller contracts together; preserve normal probe errors
and deferral. Remove USB's dependence on display ownership and competing PWRAP
access. Review RAM in the same pass; retain D08 if the LK heap/live DMA ownership
boundary still prevents a defensible larger interval. Storage must enforce no
writes at the kernel boundary. No charger policy, audio, radio, GPU applications,
rootfs installation, protected partition, loader or calibration changes.

The 60-second experiment limit is superseded only for this candidate by a
**maximum five-minute owner test from power-on**. Proven early AP watchdog stop,
Linux/PID1 and USB observation plus retained BOOTIMG recovery justify observing
idle core probes longer. This does not qualify thermal/charging policy or permit
stress: no CPU load test, charging experiment or automatic reboot. USB/PID1 must
remain observable within that interval; early stop on abnormal warmth, repeated
faults or lost observation. End with the existing manual BOOTIMG restoration.

One combined source/config/DT review, one clean build (repair errors in that
build if necessary), shared-provider tests and final layout/package validation.
No duplicate reproducibility build or unchanged ROM/source/recovery audits.
Stop at the resulting BOOTIMG for the owner's flash; classify the complete live
log and fix independent failures together in the next source iteration. M2
closure still requires the standing strict milestone audit and real exit evidence.

## Additional reverse-engineering evidence — 2026-09-10

Baseline **e4a0a81**. Reviewed the blueprint, current issues #1–32 (open
#16/#22–32), every coverage row, canonical source and retained physical results
against the [additional research](../knowledge/reverse-engineering-audit.md).
External f96d4c7 pins our existing donor 53fb57bb. Missing private captures and
stale historical statements are identified in the research matrix.

**Every hardware classification remains unchanged.** USBACM-03 remains the
11,754-byte kernel/PID1 capture. USBACM-04 and M2-INPUT-01 have offline validation
only; host inventory found no Y2 attached. M1 stays complete, M2 active/exit
incomplete, M3 gated, M4/M5 deferred beyond shared M2 prerequisites. D08, the
rootfs/application plan, recovery rules and older-board FM exclusion remain.
No new boot, build, flash or unchanged provenance requalification occurred.

The implementation strategy advances where evidence is stronger:

- #24/#28: validate the navigation candidate, then adapt EINT/I2C/AP-DMA/wheel
  and keypad by dependency. Upstream 6.18 already fixes the donor's PMIC
  single-key null-pointer problem.
- #23/#28: one WACS2 owner shared with USB; distinguish the wrapper IRQ storm
  from PMIC EINT25. Derive CCF/pin/rail contracts; absent PLL probe files cannot
  validate approximate donor rates.
- #25: use the later 480×360 correction, cold PHY/reset findings and live-pipeline
  failure history. v6.18 already protects the OVL address from PITCH_MSB writes;
  port remaining variant differences instead of redundant 6.12 fixes.
- #22 still needs loader/DMA/lowmem ownership despite main-bank agreement.
  The modem/HYP collision reinforces high exclusions. #26 can validate reported
  MSDC operation after clocks/pins/rails/DMA; there is no local native read result.
- #29 retains CON3/VGP2-brownout evidence for M3; #30 retains separate charger/AP
  watchdogs and unresolved thermal/DVFS/suspend limits. #31 retains cold radio
  calibration and failed EDR experiments; #32 retains address-space/recovery
  pitfalls. No later phase is activated.

Execution remains navigation/USB evidence → corrected shared providers and useful
RAM reconciliation → wheel/power, display and storage/rootfs → real M2 stability
and exit audit. The research report specifies the next provider contract.
Existing issues absorb the findings; no new issue/milestone fan-out. Physical
flashes remain owner-performed; protected partitions/calibration are untouched.

## M2 activation audit — 2026-09-09

### Donor adoption and GPIO input scope — baseline 8ebc800

2026-09-09. The owner authorizes donor-assisted Linux **6.18** implementation,
build/test/Git/GitHub and live host logs, with manual owner BOOTIMG flashes.
Reviewed the complete 364-file/36-commit donor inventory, local blueprint M2/M3
and later gates, current GitHub issues #1–32, canonical config/DT/source and
retained stock/RAM/Linux evidence. [Subsystem audit and dependencies](../knowledge/donor-audit.md).
No new physical result: USBACM-03 remains the 11,754-byte kernel/PID1 success;
USBACM-04 robustness remains pending. Host USB is accessible outside the sandbox;
no Y2 was attached at this inspection. Identity/recovery/provenance is unchanged.

Every coverage row below was reconsidered. Hardware status stays unchanged
except that other navigation buttons now have a concrete **PLANNED** evdev
slice; source availability does not promote hardware readiness. The former
"common providers not established" blocker now has substantial donor contracts
to correct/forward-port. SMP, CCF, EINT, PMIC, input, display, storage, audio,
radio and haptic source availability is detailed in the separate reuse matrix.
Thermal/DVFS/idle, full memory/DMA ownership and production/recovery gaps remain.
FM reception is inapplicable to the owner's older board; retain donor knowledge
only for other revisions. M1 complete, M2 active/exit blocked, M3/app deferred.
All M2 rootfs/display/input/storage/observation/battery criteria still apply.

Supersede the rigid #27→#22→#23→#24→#25→#26 order. Keep USB robustness open and
preserve both USB candidates; use GPIO navigation→upstream gpio-keys-polled→
evdev→USB as the first coherent slice under #24/#28. D08 stays unchanged.
The five inherited input lines can be sampled without EINT, I2C DMA, supply,
clock or pinmux writes. Refuse output-configured lines. Poll at 20 ms through
the input core; bounded userspace evdev capture must preserve USB/heartbeat.
This is real input integration, not a hardware-success claim. General pinctrl,
EINT, wheel rotation, keypad and PMIC power key remain subsequent dependencies.

Next combine corrected CCF/PWRAP/GPIO/EINT/I2C foundations by consumer dependency;
reconcile a useful large RAM expansion under #22 instead of arbitrary tiny
increments. DRM/panel/backlight and removable storage/rootfs follow their actual
rail/clock/DMA requirements. Retain donor AFE/codec knowledge for M3, with radio
work deferred. Planning-only wording in old issue bodies is superseded only by
the owner's explicit implementation scope; it never authorizes device flashes.

New input subsystem qualification: one clean build, full existing offline suite,
GPIO range/refusal and ARM evdev/relay cases, linked access review, resolved
config/DT and D08/BOOTIMG checks. No memory architecture change or repeated ROM,
source or recovery provenance. Stop at the validated candidate for the owner's
physical test. Source/license attribution remains with each reused piece;
donorSource stays immutable and outside versioned implementation.

[M2-INPUT-01](../build/m2-input-01-result.md) is now offline validated: one
clean kernel build, corrected PID1/package after a caught parser defect,
29 build-suite and 12 observation tests, emitted GPIO loads/refusal review and
current D08/BOOTIMG checks. Candidate 1,204,224 bytes, SHA-256
`05c6f33d7fadb75c9bc5d8cc4a23aed1a5dcc79cf6b4fe735ce9006906290d37`.
Stop at this owner-flash boundary; no input/USB hardware classification advances.

### Current scope review — one bounded USB reconnect, baseline 7ee780a

2026-09-09. The owner explicitly authorizes completing #27 robustness before
#22 → #23 → #24 → #25 → #26 → M2 boot/stability qualification and closure audit.
Reviewed the current repository, open #16/#22–#32, #27 acceptance and #28 epic,
the existing blueprint/coverage matrix and USBACM-03 capture/source/build evidence.
The successful baseline is now pushed; its 67 retained implementation/test/tool
files match the build snapshot. No repeated source/ROM/recovery provenance audit.

Every coverage row below is re-evaluated and retains its classification: narrow
kernel/PID1 logging CONFIRMED inside overall USB PARTIAL; boot, RAM, display,
power, recovery and later product gaps remain with their existing issue owners.
M1 stays complete; M2 active with blocked exit; M3 and application work deferred.
No optional exit criterion or D11 experimental limitation becomes a production
waiver. The existing backup/calibration and full DMA ownership gaps remain open.

USBACM-03 stops permanently at first detach, so it cannot satisfy the requested
reconnect criterion unchanged. Develop USBACM-04 using the already-owned PIO
controller: on first sampled CHRDET loss remove SOFTCONN, notify upstream gadget
disconnect and restore only the existing saved digital PHY session inputs;
retain the controller and PID1 history while unplugged. Permit one restart only
after fresh supply/clock, passive digital PHY, zero DMA controls and unchanged
mode/trim checks, using the same guarded session writes and upstream controller
start. A second detach, any failed guard or the original 50-second deadline is
terminal. PID1 heartbeat and the 60-second owner limit remain unchanged.

This is a localized lifecycle extension of the proven subsystem, not new USB,
DMA, memory, boot packaging, rail or production ownership. Targeted transition,
relay/ARM PID1, host capture, linked and D08/BOOTIMG checks plus one clean build
apply. Qualification still needs the physical combined delayed-open/no-reader/
disconnect/offline-heartbeat/reconnect/replay test; do not close #27 from fixtures.
The real host sees Y2 at 5-2, ttyACM0, 0525:a4a7 with the USBACM-03 manufacturer;
this session lacks tty permissions, independently of sandbox visibility.

[USBACM-04 is now ready for the owner test](../build/m2-usbacm-04-result.md).
One clean build, 16 targeted methods and eight host observation/orchestration
methods pass; 34 ARM PID1, eight worker and 29 lifecycle/guard scenarios are
included. Current D08/BOOTIMG and focused emitted-ARM checks pass. Candidate:
1,161,216 bytes, SHA-256
`ecdce008c34998dce03e1b5dd88b0c78c3015e5865b93d4ba609f6cbf134f365`.
No physical robustness run occurred and no capability classification is promoted.
Stop at this BOOTIMG for the owner flash/cable test; #27 remains open.

### Previous checkpoint — first kernel/PID1 USB capture succeeds

2026-09-09, USBACM-03 retained candidate/source identity. The owner-run
[capture-03](../knowledge/m2-usbacm-hardware-result.md#usbacm-03-kernel-and-pid1-capture-confirmed)
contains 11754 verified bytes over a 45-second reader window: exact LOG1 build
header, contiguous kernel sequence 0–77, every PID1 beat 1–43, US:6 RC:0 from
beat 4 and increasing IRQs. No relay GAP/ERR or PID1 error appears. The normal
reader timeout exits 0. The prior host-permission blocker is resolved for this
run; this is the first captured log result, not just enumeration.

Update the USB coverage row to PARTIAL: narrow kernel/PID1 logging is confirmed,
while repeatability, late-open/non-reading host, detach/reconnect and stability
remain unqualified under #27/#28. All other coverage classifications and owners
remain unchanged; no boot/memory/power or M2 exit criterion is promoted by this
capture. M1 complete, M2 active/exit blocked, M3/application deferred. This is
an evidence review within the existing experiment: no milestone closure,
new hardware scope, build or flash. Repeat the standing full audit before
activating a different scope or closing the milestone/qualification boundary.

### Previous checkpoint — USBACM-03 enumerates; host capture access blocked

2026-09-09. [Host journal](../knowledge/m2-usbacm-hardware-result.md#usbacm-03-host-enumeration-confirmed)
and owner report confirm high-speed 0525:a4a7, the USBACM-03 kernel identity
and CDC ACM ttyACM0. The former capture timeout came from the assistant's
isolated `/dev`; an unsandboxed run matches the device but gets EACCES because
the host account lacks tty permission. Neither error disproves enumeration.
No LOG1 payload was captured. The next step is the same image and script with
owner-authorized host access, not a new hardware change or build.

Narrow enumeration is now evidenced. Overall host logging remains BLOCKED on
reader access and log-content proof; other coverage statuses and owners remain
unchanged. No milestone activation/closure, memory/production scope change or
reconnect/boot-repeatability qualification is inferred. M1 complete, M2 active
with blocked exit, #23/#27/#28 open, M3/application deferred.

### Previous checkpoint — USBACM-02 sync-only refusal

2026-09-09, `c066d62` plus the retained USBACM-02 source snapshot. The owner
reports the attach prompt now appears, followed by an error **before attachment**.
The [new photo](../knowledge/m2-usbacm-hardware-result.md#usbacm-02-result)
shows poll 5, US:2 RC:-16, WACS2 `00200001` at entry/failure, valid mask zero,
and passing wrapper gates. Init is set, request is clear and FSM is idle;
only sync-idle is absent. The snapshot does not establish how long this lasts.
No controller registration or enumeration occurred. Wake/clock evidence remains
confirmed within the prior narrow scope; USB BLOCKED, boot/memory/power PARTIAL,
all other coverage classifications and issue owners unchanged. M1 stays complete,
M2 active with blocked exit, M3/application work deferred.

The next localized fix is a bounded, read-only pre-command wait for **sync only**
in M2-USBACM-03. Retained MT6582 definitions identify bit 20 separately from
channel FSM/request; vendor `wait_for_sync`/`wait_for_idle_and_sync` explicitly
poll this condition. Require init/no-request/FSM-idle on every sample, retain
the full existing ready predicate before any command, and stop immediately on
stale/active/invalid channel states. Limit each wait to 1000 ten-microsecond
delays; no transaction retry, stale acknowledgement, wrapper reset, new register,
PMIC write, memory map or packaging scope. This repairs single-sample rejection
within the active PWRAP/USB contract; it does not authorize channel recovery or
promote hardware readiness. Targeted protocol/fault, worker/ARM PID1, linked
access and one clean BOOTIMG/D08 validation apply before the owner test.

[M2-USBACM-03](../build/m2-usbacm-03-result.md) now passes one clean build,
15 targeted methods plus four host-capture methods and linked/D08/package
review. The new protocol cases cover sync settling and failure at all four
pre-command gates. Image: 1,159,168 bytes, SHA-256
`20c7f01ad97832def704523897fb5ec08cbfacd560558bbd3989b4f4426f734a`.
Hardware sync recovery/enumeration remains untested; no capability is promoted.

### Previous checkpoint — USBACM cable-wait poll refusal

2026-09-09, baseline `c066d62`. The owner reports unplugged startup and supplies
[one M2-USBACM-01 photo](../knowledge/m2-usbacm-hardware-result.md): cached
power/clock and PHY wake succeed, US:2 RC:-16 with invalid live CHRDET stops the
worker before controller registration. BEAT 12 and MemTotal 22096 kB are visible;
there is no enumeration/logging success or identified repeatability result.
The raw failed WACS2 sample was discarded, so the particular idle-gate cause
remains unknown. The earlier CHRDET/wake evidence is retained.

The localized next candidate, M2-USBACM-02, preserves that failed sample and
corrects the stale screen heading. No hardware access, guard, memory map,
packaging or production scope changes; targeted validation applies. This is
an evidence-display repair within the active experiment, not a milestone
boundary or authority to recover a busy/stale channel. Coverage classifications
and owners remain unchanged: USB BLOCKED, boot/memory/power PARTIAL, M1 complete,
M2 exit blocked, M3/application and other input/display/media/thermal gaps deferred
to their existing gates. Stop at the validated candidate for owner testing.

[M2-USBACM-02](../build/m2-usbacm-02-result.md) now passes one clean build,
15 targeted methods and four host-capture methods, including 33 ARM PID1 paths
and real-artifact/D08 checks. Candidate is 1,159,168 bytes, SHA-256
`4ff741513ef842fca28e6fd8cd08829c9c8c9d2e3e160da70552e06166460e20`.
It awaits the failed-poll screen or bounded enumeration evidence; no busy-state
recovery or new hardware success is claimed.

### Combined USB ownership scope review — baseline d9c2c98

2026-09-09. The owner explicitly replaces the register-by-register handoff with
one guarded candidate targeting enumeration and kernel/PID1 logs. USBGUARD-01
remains an untested fallback; there is no new physical result. Reviewed the
blueprint M2/exit and later gates, current open #16/#22–#32, this coverage matrix,
the CHRDET and PHY-wake reports and current implementation. All capability
classifications remain unchanged: M1 complete; M2 active, exit blocked; host USB
logging blocked pending implementation and hardware proof. Boot/memory/power
remain partial; input, standard display, media/rootfs, thermal/battery and all
later-phase gaps retain their existing owners. No milestone criterion waived.

Authorize development of one experimental vertical slice under #23/#27:
source-reviewed MT6582 controller/PHY ownership, peripheral-only PIO MUSB,
one standard g_serial ACM function and bounded kernel/PID1 log relay. Use the
observed passive handoff as a runtime gate, initially booting without USB and
requesting attachment only after successful preflight. The unobserved complete
connected-start save-current state is not normalized by guessing. Refuse unknown
clock/supply/PHY/DMA states and preserve the screen at every stage. Resolve each
write/IRQ/FIFO contract from retained FM/LK and pinned Linux/vendor source before
implementing it; combine resolved stages within the candidate, not hardware claims.

This is first controller/IRQ/FIFO ownership and requires broader qualification:
production guard/fault tests, actual ARM relay/syscall tests, full existing
offline suite, linked access review, resolved config/DT and D08/BOOTIMG checks.
One clean candidate build, no duplicate reproducibility or unchanged source/ROM/
recovery audit. Keep D08/CPU0, watchdog stop, framebuffer fallback and original
loader/partition/calibration boundaries. No USB DMA, host VBUS supply, analog
calibration rewrite, M3/audio or application work. Stop at validated BOOTIMG for
owner physical test; successful enumeration cannot be promised offline.

[M2-USBACM-01](../build/m2-usbacm-01-result.md) is now the immediate next owner
test: one clean build, full offline suite plus ARM relay/linked syscall and host
capture checks, unchanged D08 bounds and valid BOOTIMG. It attempts enumeration
and logging in one unplugged-start/late-attach boot. Implementation has advanced;
USB hardware status remains BLOCKED pending that evidence. No milestone or
capability is promoted from the build. First-detach/reconnect qualification and
connected-start recovery remain outstanding; the original #27 criteria stand.

### Previous checkpoint — CHRDET confirmed, connected-start guard refusal

Baseline `de1a6d0`, 2026-09-09. [Three owner photos](../knowledge/m2-chrdet-hardware-result.md)
show CHRDET 007B/1 with USB attached at startup and 0001/0 without it. PWRAP,
clock and initial USB snapshots succeed. The connected boot reaches BEAT 10 but
refuses wake before writing (6a=BE, W0, V00/000); no-USB photos show successful
wake and BEAT 11/46. Same-boot chronology of the no-USB pair is not explicit.
The connected screen's zero wake fields are unread, not hardware values.

Narrow PMIC charger detection advances to CONFIRMED. Broader power/boot/memory
remain PARTIAL and host USB logging BLOCKED. Current #22–#32 tracking and the
coverage matrix retain their remaining gaps: no input/display/media/rootfs,
thermal or later-phase capability advances from these photos. M1 stays complete,
M2 active with blocked exit, M3 and Y2PlayerNative deferred; no criterion waived.

The next localized step is [M2-USBGUARD-01](../knowledge/m2-usb-guard-diagnostic.md),
preserving the already-captured raw USB state on early guard refusal and marking
unread data. Hardware access/guards, 260-byte ABI, D08, DMA, loader, calibration,
recovery and packaging policies are unchanged. This is a presentation fix within
the active prerequisite, not a new subsystem or milestone boundary. Targeted
tests and one clean build with D08/BOOTIMG checks apply. Stop at the candidate
for connected-start owner photos before selecting a recovery/session write.

[M2-USBGUARD-01](../build/m2-usbguard-01-result.md) now passes one clean build,
all 15 selected methods (26 ARM PID1 and 49 PHY-wake scenarios), linked probe
comparison and D08/BOOTIMG checks. Candidate: 1,097,728 bytes, SHA-256
`e4078583ab53bd7e6388c94a357023fe8a7504ccc5a7499341cf72287ecd19b5`.
It awaits the connected-start owner test; no new hardware result is inferred.

### Previous checkpoint — PHY wake confirmed, baseline 305f8f9

The owner-confirmed [same-boot USB-connected pair](../knowledge/m2-phy-wake-hardware-result.md)
shows BEAT 10→49, IRQ 1134→5463, MemTotal 22208 kB, no reported error, the guarded
6a 04→00 write and unchanged sampled trims. The earlier conflicting pair remains
retained with unresolved chronology. Full PHY readiness/session detection is
still unproved: POWER20/DEV80 remain after release with the cable attached.

Reconsidered the coverage matrix and current open #22–#32: narrow PHY release
advances to CONFIRMED; boot qualification/memory/power remain PARTIAL, USB logging
BLOCKED, and input/display/media/rootfs/thermal and later-phase gaps unchanged.
No optional criterion waived, M1 reopened or M3 work started. The next localized
step is [one PMIC CHRDET status read](../knowledge/m2-chrdet-probe.md) through the
proven PWRAP transport, addressing only the cable/session prerequisite. It adds
no new PMIC/analog/VBUS/MAC/DMA write, changes no memory/packaging policy and uses
targeted validation. Stop at the next BOOTIMG for owner test. Existing recovery,
stock loader, partition and calibration boundaries remain unchanged.

[M2-CHRDET-01](../build/m2-chrdet-01-result.md) passes one clean build and all
18 selected methods plus linked transaction and D08/BOOTIMG checks. Candidate:
1,097,728 bytes, SHA-256
`e511fb89e45326673183f426a4bdd3b6ca708c25bf3e6ceecfc4a67478aa9660`.
It awaits owner test; no physical charger-presence result is claimed yet.

### Latest scope review — PHY suspend release, baseline 726d38d

[USB-state photos](../knowledge/m2-usb-state-hardware-result.md) confirm all 21
reads, BEAT 11→50 without errors, MemTotal 22204 kB, disconnected B-device MAC,
eight zero DMA controls, inherited IRQ enables and PHY force_suspendm=1/value=0.
Current active issues and every coverage row were reconsidered against owner/
blueprint requirements. Narrow MAC/PHY read access is CONFIRMED; the controller
contract/host logging remain BLOCKED. Memory, boot qualification and power stay
PARTIAL; input/display/media/rootfs/thermal/battery and later-phase gaps remain.
No optional exit criterion is waived; M1 stays complete and M3 deferred.

The smallest functional next step is [M2-PHYWAKE-01](../knowledge/m2-phy-wake-probe.md):
read/check remaining PHY mode controls, then conditionally clear only 6a bit2,
as corroborated by FM/LK. Require disconnected B-device and zero DMA, preserve
calibration and all analog/VBUS settings, inspect readback without retry. This
adds one bounded PHY control write, not UDC/DMA ownership or calibration policy.
Full existing offline checks plus emitted access review apply; stop at BOOTIMG
for owner test. Recovery/loader/partition boundaries and D08 are unchanged.

[M2-PHYWAKE-01](../build/m2-phywake-01-result.md) now passes one clean build,
21 top-level and four observation test methods plus linked-write/layout review.
Candidate: 1,097,728 bytes, SHA-256
`d68c8fbc56721c7aa1fda5b3e321a9aff5876661aac857505864e28d8924793a`.
It awaits owner test; no successful PHY release is claimed yet.

### Latest scope review — USB state observation, baseline 23d09db

The [returned clock-probe photographs](../knowledge/m2-usb-clock-hardware-result.md)
show no errors, BEAT 15→50, IRQ 1689→5574, MemTotal 22208 kB, successful
PWRAP/VUSB reads, USB0 ungated, USB mux selector 1 enabled and PLL USB reference
enabled with power on/isolation off. Reconsidered every coverage row below,
current open issues and owner/blueprint requirements: only narrow clock-read and
timer-reporting facts advance. Core clocks/pins/IRQ/power stay incomplete;
boot/memory remain PARTIAL, USB logging BLOCKED. Input, standard display, media,
rootfs, thermal/battery and later milestone gates are unchanged. No optional
feature is waived, no M1 reopening and no M3 work.

Next scope: a single cached read-only MAC/digital-PHY/DMA-control snapshot under
strict observed supply/clock guards. This is first access to those register
windows, requiring source/FM/LK width/side-effect review, fault tests and the
full existing offline suite plus D08/package validation. No interrupt status,
FIFO, endpoint index, analog trim/calibration, clock or controller writes; no
DMA ownership change. [Contract](../knowledge/m2-usb-state-probe.md).
Stop at the new BOOTIMG for owner test. Keep all existing recovery boundaries;
no repeated unchanged source/ROM/recovery audit is needed.

[M2-USBSTATE-01](../build/m2-usbstate-01-result.md) now passes one clean build,
20 full build-suite methods and four observation-tool methods. Candidate:
1,095,680 bytes, SHA-256
`8c1f701b61a1adb70b1377071e6cb885cda989a422fd75d626cd5c7da5bdb5e7`.
It awaits the owner test; no MAC/PHY hardware result is inferred.

### Original activation review

Baseline `ebeed72`, clean `main`; current GitHub #1–#32, open M0 and closed M1
reviewed against the local blueprint and retained hardware result. The owner now
explicitly authorizes autonomous M2 research/implementation/builds and normal
Git/GitHub operations, with a stop at each hardware-testable BOOTIMG and before
any physical-device modification. This supersedes the earlier planning-session
restrictions and queued research order; it does not establish any hardware fact.

**M2 is ACTIVE, entry PARTIAL; exit BLOCKED on unimplemented/unverified core
interfaces.** M1 stays complete. All coverage rows below were reconsidered:
their capability classifications and evidence limits remain unchanged because
no newer physical result exists. M2's phase status changes from PLANNED to ACTIVE;
#22–#27 remain open, and #27's controller gate is still BLOCKED. The working
owner-reported version is 6.18.0-y2-m1; exact flashed SHA-256, MemTotal, repetition
and timing are not supplied. The retained time32 candidate identity is recorded
in [the M1 result](../knowledge/m1-runtime-hardware-result.md), not promoted to
an identified hardware trial.

Execution priority: (1) #27 USB contract and only #23's needed VUSB/PWRAP/clock
prerequisite; (2) one logging implementation/trial if that contract permits it;
(3) RAM adequacy and #22's bounded expansion decision; (4) #24 input; (5) #25
standard display handoff; (6) #26 read-only removable media and recoverable
stage-appropriate rootfs; (7) identified repeatability/stability and M2 exit audit.
RAM expansion, UART wiring, full SMP and full power-framework implementation are
not prerequisites for USB research. Reorder subsequent work only on evidence.
The smallest current blocker is the FM-normal-handoff USB PHY/clock/VUSB/DMA
contract; inspect those exact functions before selecting a runtime change.

D08, CPU0, stopped AP watchdog and guarded framebuffer remain the default.
Stock preloader/LK, partition tables and calibration remain protected. Existing
SPFT/FM recovery evidence is retained without another provenance/recovery audit;
M0 backup/installed-lineage and inherited-DMA uncertainties remain residual risks,
not production waivers. New subsystem ownership must be justified separately.
No M3 audio or Y2PlayerNative implementation starts before real M2 exit evidence.
No optional feature has been waived merely to reach today's target.

### Current execution checkpoint

[M2-PWRAP-01](../build/m2-pwrap-01-result.md) is offline validated, 1,093,632 bytes,
SHA-256 `c51102c861d65d39f5af2425ed73e9c1ba21d8adaae1b016f450a58630946ac0`.
The owner has now supplied [two physical photographs](../knowledge/m2-pwrap-hardware-result.md):
PWRAP RC=0/VALID=3, CID=0x2023, VUSB=0xc000, BEAT 18→50 and MemTotal 22208 kB.
The cached PWRAP/VUSB result is confirmed on this boot; exact flashed hash,
host timing and restoration remain unreported. Timer -61 is a source-identified
IRQ-name parser mismatch. #23/#27 and M2 remain open.

Scope review at `c6709cc`: the next localized candidate corrects IRQ parsing and
reads four exact USB clock handoff registers; no clock/PHY/USB controller/PMIC
write or DMA ownership change is added. [Contract](../knowledge/m2-usb-clock-probe.md).
All coverage rows were reconsidered against these photographs: boot stability,
memory and power remain PARTIAL (now with measured MemTotal and one successful
PWRAP read path); USB remains BLOCKED on its lower-level ownership contract.
Other input/display/storage/rootfs/thermal/battery gaps and later-phase gates are
unchanged. Preserve D08, CPU0, screen/watchdog and established recovery policy.

[M2-USBCLK-01](../build/m2-usbclk-01-result.md) is now offline validated and
awaits the owner test: 1,093,632 bytes, SHA-256
`602080eb199f3a4be30082ee114dfeb97ef414e10d8ad4acf5d239f2096cd6ec`.
One clean build and 19 targeted validation methods pass; no device action occurred.

### First prerequisite scope review

The focused [PWRAP/VUSB probe contract](../knowledge/m2-pwrap-probe.md) now resolves
one implementable #23 step using FM/LK and pinned upstream/vendor source. This
adds bounded AP PMIC read transactions to a candidate, with no PMIC register
writes or USB ownership. Power remains PARTIAL and USB remains BLOCKED pending
physical state evidence and the remaining PHY/clock/DMA contract. D08 and all
other coverage classifications remain unchanged. Basic battery/charger reporting
also remains an M2 exit gap from the blueprint; VUSB status cannot satisfy it.

## Blueprint assessment and authority

The [Y2Linux/Y2PlayerNative blueprint](https://github.com/SchulzCode/Y2PlayerNative/blob/main/YY2PlayerNativeBlueprint.md)
still covers the project's direction adequately: upstream 6.18, standard Linux
interfaces, native Rust/C application, native audio before central player work,
power/connectivity/product gates and rolling-wave planning. This audit reviewed
the local blueprint, the actual Y2Linux code/config/DT and retained build reports,
current issues, and the stock evidence indexed in
[evidence-index.md](../knowledge/evidence-index.md). Local blueprint snapshot SHA-256: `f9850e35ed790008cbb125001cf166848c7dd1c11c580d41299ecbe311add95e`.
No repeated provenance audit
or new driver investigation was performed. Source applicability is not hardware
success; Android behavior is a reference, not the native implementation.

Important reconciliations:

- Blueprint example task IDs are illustrative. Existing Y2E/Y2B IDs and results
  remain authoritative; do not create duplicate tasks with those sample IDs.
- The owner superseded the original role split with direct M1 implementation.
  That authorization is not a standing permission to execute a planning epic.
- The owner accepted specific first-boot risks under D11. M1 core success does
  not retroactively certify all M0 backups/lineage/DMA questions. M0 remains open;
  proven same-device SPFT recovery must not be repeatedly rediscovered.
- **M1 core is achieved**, as explicitly reported by the owner. Blueprint M1
  wording about reproducible boots/stability is broader than the retained report:
  boot identity/repetition/timing/stress evidence remains PARTIAL and is tracked
  under Y2H-300. Do not reopen the achieved core or claim release qualification.
- New recurring milestone audits and the fast-iteration validation policy below
  supplement the blueprint. They are not permission for a full build on a
  documentation-only day.

## Classification key

One primary status describes the scoped area, not every fact in it:

- **CONFIRMED:** the stated narrow result is directly evidenced; scope matters.
- **PARTIAL:** useful evidence or execution exists, but the platform capability
  or its qualification is incomplete.
- **PLANNED:** a bounded next task exists; it has not been implemented/validated.
- **UNKNOWN:** the required contract/capability is not established; an epic is a
  reminder, not enough evidence to upgrade this to a resolved plan.
- **BLOCKED:** a specific missing prerequisite prevents the stated next capability.

Hardware-fact labels in older knowledge documents (including INFERRED/DISPROVEN)
remain unchanged. An epic does not turn unknown hardware into confirmed support.

## Coverage matrix

Reassessed at the September 15 M5 entry audit using fresh authenticated
POWER-02 evidence, current sources and retained milestone results. All rows
were reviewed; unchanged rows retain their stated limits. Driver/interface
availability and external research do not imply consumer qualification.

| Area | Status | Actual evidence / remaining gap | Durable tracking |
| --- | --- | --- | --- |
| Physical Linux + initramfs + native PID1 | **CONFIRMED** | Storage06 switches from rescue to internal Buildroot at 8.728982s and authenticates owner-key SSH with no SD block device. Earlier DEV-02 removable-root and M1 results remain historical. | Closed M1 / #20–21; core stays achieved; #33 internal storage. |
| Boot/kernel stability and maintenance | **PARTIAL** | POWER-02 full image-length readback matches; internal boot and SSH work. Two retained reconnects pass with runtime PM held on. Charging failures and broader boot/cycle/stress/maintenance qualification remain. | #28/#30/#32. |
| RAM and reserved/DMA ownership | **PARTIAL** | 992 MiB bank with exclusions; physical MemTotal954660KiB and HighTotal228352KiB. Completed 256MiB short allocator test passes, substantial HIGHMEM consumed. Full-suite run capped600s; every-page/long-run/inherited-DMA containment is not proved. Narrow expansion #22 satisfied. | #22; #28. Owner-authorized development qualification. |
| SMP | **CONFIRMED** | Four CPUs online on DEV-02 with advancing cross-CPU/timer IPIs and no interrupt errors; full coherency/PM stress is a later qualification scope. | #28; optional for first wired player. |
| Clocks, resets, pinctrl/GPIO/IRQ and I2C | **PARTIAL** | DEV-02 CCF/PWRAP/MFD/regulator/core consumers operate; wheel has52IRQ/52I2C completions and both directions, no timeout. Exact full clock rates/unused orphan gates and future consumer reset/rail ownership remain #29/#30 prerequisites. | #23/#24/#28; [research](../knowledge/reverse-engineering-audit.md). |
| Watchdog and controlled reset | **PARTIAL** | POWER-02 charger watchdog serviced 646 times before voltage-fault inhibition. AP_RGU reset, RTC poweroff and offline routing are implemented; controlled reboot/off/wake acceptance remains open. | #28/#30; no new transition test. |
| On-device diagnostic channel | **CONFIRMED** | DEV-02 native fbcon and owner-confirmed visible color/checkerboard pattern; supported unblank succeeds. Cursor handling fixes a later shared-buffer checksum mismatch without a kernel change. | #20–21; [result](../knowledge/m1-runtime-hardware-result.md). |
| Developer host logs / USB | **PARTIAL; reconnect workaround tested** | Storage06/POWER-02 ACM and owner-key SSH over ECM work. POWER-02's first reconnect reproduces an IRQ-storm shutdown (-75). Keeping MUSB runtime PM active passes two physical reconnects on one boot, including wall-source charging; the scoped startup change is installed. Kernel correction with automatic runtime PM, fresh-boot validation of the startup edit, broader reconnect and power qualification remain open. [Result](../hardware-evidence/2026-09-15-usb-reconnect/README.md). | #27, #23, #28; #30 charging blocker; #33 owner SSH. |
| Physical UART / early crash capture | **UNKNOWN** | UART0 candidate exists; no pad/level/wire capture. Ramoops retention/reader is unproved. USB cannot log hangs before its initialization. | Existing #16; broader crash/debug policy #32. |
| Display/controller/panel | **CONFIRMED** | DEV-02 DSI-1 connected, CRTC55 active, framebuffer56 XR24 480x360/pitch1920, fbcon bound, safe GEM/OVL address match and owner-confirmed pattern. Blank state corrected through sysfs; broader modes/power sequencing remain later. | #25/#28; [research](../knowledge/reverse-engineering-audit.md). |
| Backlight | **PARTIAL** | DEV-02 brightness/actual/max32 with physically visible panel; native brightness range, PWM/rail transitions and suspend sequencing remain #30. | #25/#23; #28/#30. |
| Wheel/select input | **CONFIRMED** | DEV-02 raw evdev has balanced Select and both wheel KEY_UP/KEY_DOWN directions;52wheelIRQ/52I2C completions. Supersedes DEV-01 -110 transport regression. | #24/#28; [research](../knowledge/reverse-engineering-audit.md). |
| Other buttons/power-key/touch/wake | **PARTIAL** | All five navigation keys, both volume keys and brief Power have balanced DEV-02 evdev events. No redesign; wake/suspend remains M4 and no touchscreen claim. | #24/#28 and #30; [donor audit](../knowledge/donor-audit.md). |
| Removable SD | **CONFIRMED** | DEV-02 SD128 on11240000.mmc at13MHz/one bit; ext4 Y2ROOT boots Buildroot, reads files, accepts verified userspace updates and sync.512MiB filesystem retained; throughput/hotplug/card-removal/power-fail qualification later. | #26/#28; [research](../knowledge/reverse-engineering-audit.md). |
| Internal eMMC | **PARTIAL; internal boot CONFIRMED** | Storage06 authenticated reads confirm 15203328 logical sectors, exact p5/p7 geometry, stock MBR/EBR hashes and complete BOOTIMG hash. Offset 23552 restores normal reads; internal ext4 root/data mount rw and Buildroot starts without an SD block device. Owner SSH now works. Deliberate bounded-write/repeat/stress qualification remains. Two synthetic ext4 buddy-cache inode warnings remain; both filesystem checks report clean. | #33 storage; #28/#32. |
| Development rootfs and filesystem/data layout | **CONFIRMED within observed scope** | Current production Buildroot uses internal ext4 Y2ROOT/Y2DATA and BOOTIMG-owned modules. Earlier removable-root results remain history. Live USB startup differs additively from the retained root image. Preserving updates/data remain the production contract. | #28/#32/#33. |
| PMIC/battery/charger telemetry | **PARTIAL; current charging failure** | POWER-02 stops at 4.200073 V with latched voltage fault 0x8. Earlier OVP 0x10 is separate/unresolved. Brief DCP result is not sustained/full/offline acceptance. Pack thermometry, measured current and SOC remain unavailable. | #30; fresh M5-entry evidence above. |
| Thermal sensors/protection | **PARTIAL** | Fresh CPU about 14.7 C and PMIC about 46.9 C preserve the accuracy discrepancy. Own-efuse conversions and software trips exist; physical cooling/protection and absolute accuracy remain unqualified. Neither reading is pack temperature. | #30/#28. |
| cpufreq/voltage/OPP and cpuidle | **PARTIAL** | POWER-02 source offers 598/747.5/1040 MHz at the retained 1.15-V constraint, with WFI fallback. Fresh current frequency is 598 MHz. Repeated OPP/load/cooling and consumption acceptance remain; no voltage-scaling claim. | #30/#28. |
| Runtime PM/suspend/resume/wake | **PARTIAL** | POWER-02 exposes s2idle/deep and integrated SPM/CPU context. Fresh entries=0/resumes=0; same-session deep restore, intended wake and consumption remain unqualified. USB on/active workaround has unmeasured idle-power cost. | #30; integrates #28/#29/#31. |
| MT6582 AFE/ASoC/I2S/DMA | **CONFIRMED narrow S16/44.1k path** | AUDIO-02 DL1/CON3/IRQ/ALSA pointer progress and clean headphones; startup fix137b3f2 is deployed/tested. 48kHz, L/R and repeated stop/start remain;24/32-bit/higher rates unimplemented. [Result](../knowledge/m3-audio-01-live-result.md). | #29; shared resources #23/#28/#30. |
| CS43131 / headphone / amplifier / speaker routing | **PARTIAL; basic headphones CONFIRMED** | Own CS43131 revisionA1 operates through upstream ASoC, VGP2=1.8V, ordered20/18/15 enables; clean44.1kHz audible headphones. SpeakerGPIO8 stays low; amplifier identity/output, jack reporting and precision clock validation separate/unqualified. | #29 with #23/#30. |
| Wi-Fi | **PARTIAL evidence; no native interface** | Own stock CONSYS_MT6582/mt-wifi metadata and matched firmware inventory; donor uses AHB HIF, not SDIO. Native driver, exact stepping/calibration/regulatory handling and end-user WPA2/network/reconnect/coexistence tests remain. | #31 entry audit; #30 prerequisite. |
| Bluetooth | **PARTIAL evidence; no native HCI** | Own stock BTIF/DMA/wake IRQs corroborate transport. External full cold calibration/A2DP and matching generic modem firmware reduce uncertainty. Own HVR/FVR, calibration/addresses, BlueZ/audio/reconnect/EDR/BLE/coexistence remain unqualified. | #31; #29 audio, #30 PM. |
| Firmware and per-device calibration | **PARTIAL** | Five own-stock files have exact hashes/headers and patch order; redistribution grant and controller compatibility not established. Own protected copies obtained read-only and retained privately; record mapping and valid addresses unproved. | #31/#32; no blobs/private data distributed. |
| FM where physically supported | **CONFIRMED scope exclusion on this board** | Owner identifies older board without usable reception; earlier own FM driver reported 0x6627 but reception failed. FM reception is unsupported here and not an M5 gate. Later populated revisions need independent evidence. | Conditional #31/M16 reference only. |
| Recovery and safe acquisition | **PARTIAL** | Owner-proven SPFT/FM history plus documented BOOTIMG-only Android restoration. Exact current image/per-device backups, consistency and independent retention remain incomplete. [Recovery](../knowledge/recovery.md), [actual trial](../knowledge/first-experiment-result.md). | Open M0; #32 owns continuing production/recovery coverage. |
| Updates / rollback / production security | **PARTIAL** | V1 release manifest, component versions, allowlists, per-owner SSH provisioning and staged rescue update/installer contract implemented. No OTA/signature implementation; no automatic torn-BOOTIMG rollback, unchanged LK supplies no new verified boot. | #33 architecture/package; #32 future lifecycle/security. |
| Production rootfs / services / non-root app contract | **PARTIAL** | Fresh POWER-02 inventory confirms internal p5/p7 read-write, owner-key SSH and no SD block device. Wider write/recovery/non-root lifecycle remain. Radio services are absent; future mutable configuration/bonds belong in Y2DATA. | #33/#32; radio services #31. |
| Time/RTC, entropy, identity and diagnostic privacy | **PARTIAL** | POWER-02 RTC exists but reports 2082; persistence/alarm/deep wake unqualified. SSH uses the retained reinstall pin and persistent owner authorization. Radio addresses/calibration remain private and unvalidated. No invented time/entropy/identity defaults. | #30/#31/#32. |

## Gaps found and minimum additions

The blueprint already names the major phases; the repository had only M0/M1
GitHub milestones and narrow #22–#27 follow-ups. Audio, product PM, connectivity
and production lifecycle had no durable epic owner. Common clock/pinctrl/I2C,
watchdog/reset, eMMC/rootfs, firmware selection/redistribution/calibration and
conditional FM could otherwise fall between the narrow issues.

Five **deferred planning epics** now retain these gaps. No detailed children or
new GitHub milestone shells are needed today:

| Blueprint phase | Tracking epic | Current phase status / dependency |
| --- | --- | --- |
| M0 Evidence & Recovery | Existing open milestone 1 | **PARTIAL**; owner-accepted experimental exceptions are documented, not a full pass. |
| M1 First Boot | Existing closed milestone 2 | **CONFIRMED core**; broader stability qualification moves into #28. |
| M2 Core Hardware | [Y2H-300 #28](https://github.com/SchulzCode/Y2Linux/issues/28) | **Core/Buildroot qualified for M3 progression**; #22–26 satisfied. USB reconnect #27 stays open under #28 for later whole-platform qualification. |
| M3 Native Audio | [Y2A-300 #29](https://github.com/SchulzCode/Y2Linux/issues/29) | **ACTIVE / NEAR COMPLETION**; clean native S16/44.1kHz headphones. Notification fix deployed/tested. Remaining48kHz, L/R, stop/restart/repeat; #27 deferred. |
| Production Storage / Installation v1 | #33 storage activation (2026-09-13); #32 lifecycle | **ACTIVE**; internal root/data boot, stock block reads and owner-key SSH confirmed after manual flash. Wider write/stress/recovery qualification remains. |
| M4 Power Viability | [Y2P-400 #30](https://github.com/SchulzCode/Y2Linux/issues/30) | **ACTIVE/PARTIAL; charging failure**. POWER-02 deployed and hash verified; voltage fault 0x8 now stops charging. Earlier OVP and full/offline/deep-suspend/RTC/thermal acceptance remain open. Preserve implementation; no M4 rewrite under M5. |
| M5 Connectivity | [Y2N-500 #31](https://github.com/SchulzCode/Y2Linux/issues/31) | **ENTRY AUDITED / BLOCKED**; owner authorizes the end-user platform, but current M4 charging/qualification and own silicon/calibration mapping prevent integrated implementation/deployment. No native radios yet. |
| M6 Production Y2Linux | [Y2R-600 #32](https://github.com/SchulzCode/Y2Linux/issues/32) | **PLANNED**; storage/recovery/PM gates and later M18–20 platform release obligations. |
| M7–M20 application/product phases | Existing blueprint, no native backlog added | **NOT STARTED**; Y2PlayerNative waits for reusable Linux platform and final qualification. |

M4 numbering is not permission to defer basic thermal/power safety until after
audio. M5 radio-specific power measurements depend on radio operation, so these
phases have shared gates rather than a rigid all-or-nothing serial chain. M6
rootfs contracts can mature with M2 without prematurely choosing production OTA.
The original next-five order is retained as history. Current owner direction
defers #27 while finishing small M3 acceptance; it does not waive reconnect from
final platform qualification.

## When Y2PlayerNative can become the serious workstream

The owner's 2026-09-10 platform-first instruction supersedes the earlier
wired-player-only gate. First qualify the reusable general-purpose Y2 platform:
M2 core/Buildroot including reliable development reconnect; M3 native ALSA/ASoC,
MT6582 AFE, CS43131, amplifier, headphone/speaker routing and jack detection;
M4 battery/charging/thermal, cpufreq/cpuidle, suspend/resume/wake and controlled
shutdown/reboot; M5 Wi-Fi/Bluetooth with own firmware/calibration, BlueZ,
reconnect/coexistence. Then complete GPU/lima and final whole-system qualification.
Only after that does Y2PlayerNative implementation begin. #29–32 retain these
gates; no application or later hardware phase starts in this live audit.

## Standing milestone-boundary rule

**At every major milestone boundary, repeat this gap audit and update the roadmap
from real hardware evidence before proceeding.** Apply it before phase activation,
exit/closure or a material change in hardware/memory/production scope. Root
`AGENTS.md` links this rule so it remains visible in future sessions.

The boundary review must:

1. Compare current blueprint/owner decisions, repository changes, all active
   issues/epics, retained validation and actual hardware results. Record date,
   revision, tested artifact identity when known, evidence origin and limitations.
2. Refresh every coverage row using this classification vocabulary. Separate
   working narrow behavior, untested driver availability and planned work. Preserve
   older failures/history with explicit superseding notes; do not invent missing
   timings, hashes, firmware selection or hardware success.
3. Reconcile dependency/phase gates, accepted risks, unresolved backup/calibration
   requirements and the next smallest blocker. An experiment's accepted risk is
   not a permanent production waiver. Honor all later explicit owner decisions.
4. Link each material gap to an existing issue/epic; add only a missing high-level
   bookmark when necessary. Decompose only the next evidence boundary. Record
   whether the next phase is ready, partially ready or blocked and why.
5. Update this audit, relevant knowledge pages, roadmap and GitHub milestone/epic
   status before proceeding. Hardware authorization remains separate.

An audit is a planning/evidence review, not an automatic test run. Future small
localized code changes use one clean build, affected tests, D08/layout/BOOTIMG
checks and final size/hash. Architecture/memory/packaging changes, new hardware
subsystems, milestone qualification and release candidates use the appropriate
full verification boundary. Do not rehash the unchanged entire source tree or
redo ROM/SPFT provenance without relevant cause. A documentation-only request
never triggers a build or hardware operation to fill a missing audit datum.
