# M4 end-user power completion

2026-09-14 entry, source `aacb9a5` on local `main`. GitHub `main` is
`444e7eda99dc1f7d90d0d591ebda4f3219efe0c9`; the five local charging/source/evidence
commits are retained. **M4 #30 remains ACTIVE/PARTIAL. M5 is not started.**

## Current hardware evidence takes precedence

The owner confirms that the production M4 charging kernel has been installed,
BAT0 reports Charging at 70 mA / 4.175 V, the first active test was near 3.46 V,
watchdog servicing worked, and no charger-driver fault was observed in that
initial test. This supersedes the older M4-01/ADC-01 and CHARGE-01 predeployment
statements. It does not establish sustained energy gain or completed charging.
Internal Y2ROOT/Y2DATA, SSH, Linux 6.18 and the M3 audio foundation remain the
confirmed baseline; their earlier retained physical results are not rerun merely
to repeat provenance.

This session independently reads USB descriptors identifying
`Linux 6.18.0-y2linux-m4-charge-01 with musb-hdrc`, VID/PID `0525:a4aa`.
The direct route to 10.42.0.1 and ACM belong to the same physical USB device.
The owner confirmed yesterday's latest flash and explicitly confirmed the new
SSH host key. Strictly pinned SSH observation now succeeds. ACM host device
permissions remain unrelated to charger correctness or issue #27.

The [new baseline](../hardware-evidence/2026-09-14-m4-70ma-baseline/result.json)
supersedes the initial no-fault charging report: on this same boot, Charging
began at 3.439819 V and stopped after about 1016 s at 3.399829 V with the
3.4-V voltage guard (`fault=0x8`). There were 907 watchdog services and no
watchdog or thermal fault. The subsequent 36-sample observation began after
inhibition; voltage continued falling, including with the backlight off.
This is a failed sustained-gain baseline, not proof of a measured net current.
The owner has now switched the Y2 off to retain remaining battery energy.
The attempted SSH shutdown and RTC set did not reach it; neither is a pass.

New physical discrepancies also require correction/qualification: CPU output
near 6 C is implausible alongside PMIC readings near 38 C; RTC read is in 2082;
preflight `ro,noload` unmount logs show inode-1 orphan warnings before successful
journal recovery. Do not carry forward a thermal-accuracy or storage-integrity
pass for these new observations.

## Authorized production scope

The owner's current instruction expands the earlier fixed-70-mA candidate scope:
reconstruct actual Y2 source/current and low-battery policies; implement one
integrated production candidate; qualify it physically; update docs/GitHub to
the results. Do not increase current before observing the installed 70-mA
baseline. Do not exceed the actual stock ceiling or change CV without evidence.
No diagnostic image series, raw-register charging script, guessed voltage or
temperature, alternate PM architecture, or automatic milestone closure.

The production rescue/initramfs must own offline charging before root discovery
and switch_root. Charger insertion from off must show a simple timed charging
display, then blank it, without normal Buildroot, SSH, ECM or applications.
A brief Power interaction shows status; a deliberate Power action requests
normal boot only above the evidenced boot threshold. Charging completion stays
offline; removal requests actual poweroff. Poweroff with external power must
return to offline charging. Preserve the stock preloader/LK wherever possible
and preserve OTA/recovery, root/data separation and exclusive PMIC ownership.

## Required physical exit gates

Every row below is required to close M4. A compiled implementation is not a pass.

| Area | Entry evidence | Remaining physical acceptance |
| --- | --- | --- |
| Charging | Owner-confirmed 70 mA/4.175 V active charge and watchdog, initial no-fault test | Sustained positive voltage trend; useful evidenced current; PC, wall charger and power bank; removal and source changes |
| Detection | CHRDET and configured-PC allocation exist; BC1.1 absent | Source classification independent of gadget enumeration, conservative unknown policy, consistent USB descriptors |
| Charger lifecycle | Checked protections, 4-second WDT, 1-second service, bounded voltage/CV stop and hysteresis implemented | Final-current termination/recharge, watchdog/fault correctness, precharge/low-battery recovery without Android |
| Offline charging | Owner observes unwanted full-OS boot on charger insertion | All eleven offline-charge acceptance cases, including screen timeout/status/intentional boot, poweroff while plugged in, unplug/reinsert, completion and recovery |
| Battery monitoring | BAT0 status/presence/voltage/current and CV setpoints; raw BATON/ISENSE | Truthful telemetry during every power transition; current and pack temperature only if calibrated |
| Thermal/frequency | Own-efuse conversions implemented, but new CPU readings are implausible; 598/747.5/1040 MHz at 1.15 V | Calibrated physical temperature response, repeated OPP transitions, bounded load and thermal/cooling behavior |
| Idle/suspend | WFI/s2idle implemented; active charger keeps watchdog servicing awake | Safe deeper MT6582 standby, low consumption, same-session restore; WFI/s2idle fallback retained |
| Wake/resume | Power/RTC intended, CHRDET wake implemented but unqualified | Intended Power/RTC/source wake; no accidental wheel wake; display/backlight/audio/input/USB/SD restore |
| Storage | Internal root/data and M3 audio foundation confirmed | Root/data integrity through final suspend, reboot, poweroff and subsequent normal power-on |
| RTC/reset/off | Standard RTC/reset/BBPU poweroff implementation exists | Read/set, reboot persistence, alarm/deep wake, off persistence, actual hardware off and ordinary power-on |

Pack Celsius, calibrated current and a Y2-specific SOC estimate may remain
explicit follow-ups only under the owner's stated conditions; none may be
fabricated. PMIC die thermal shutdown is not pack-temperature protection.
SDP/CDP/DCP are source classifications, not measured net battery current.
Normal-source speed and low-battery recovery are hard gates, not telemetry
enhancements that can be deferred to call M4 complete.

M3 residual 48 kHz/channel/repeat checks and wider #33 storage qualification stay
separate. USB reconnect #27 stays separate unless it blocks these tests. No M5,
radio, GPU or Y2PlayerNative work is activated here.

## Evidence and implementation review in progress

The actual stock `select_charging_curret` at 0xc03a1d88 requests 500 mA for
SDP (PMIC floor 450 mA), 650 mA for CDP, 1100 mA for DCP/nonstandard,
800 mA for its detected Apple signature, and 70 mA for unknown. The working
production implementation caps recognized higher-current sources at 650 mA,
uses 450 mA only with a configured 500-mA SDP allowance, and retains 70 mA as
the conservative fallback. These new policies are **not physically qualified**.

Stock LK uses 3200 mV for ordinary boot and modes 8/9 for kernel/offline
charging; Linux precharge transitions above 3400 mV. LK's early charger
initialization selects 450 mA. The production rescue UI uses 3400 mV for an
intentional OS boot, keeps charging offline above that threshold, and never
boots simply because the charger is removed or charge completes. Its minimal
PC configuration uses the existing CDC-ACM identity, no listener and no ECM,
to obtain an optional configured charging budget; source classification and
conservative charging do not depend on that configuration.

Every retained Y2 CPU frequency table has 598/747.5/1040 MHz at 1150 mV.
The 1050-mV sleep entry is not evidence of a safe lower operating OPP.
Do not invent voltage DVFS for these frequencies.

Pinned BSP `d53dd75c3ff77cac3f5be58fddfe660e94f94d64` supplies
`pmic_chr_type_det.c`, `mt_spm_sleep.c`, `mt_sleep.c` and `mt_dormant.c` for
comparison with the retained stock Y2 binary. The donor `mt6582-spm.c` explicitly
implements normal PCM only. Full suspend additionally depends on CPU/context,
DRAM/infra retention, PMIC/SPM ownership and wake restore. Its normal PCM must
not be relabeled as deep standby.

The current production kernel disables generic ATAG→DT merging and deliberately
uses controlled DT bootargs. Stock boot_reason/boot_mode therefore need a bounded
preserved-loader-data path; importing stock RAM/initrd/command-line policy is
not an acceptable side effect of implementing offline charging.

## CPU-domain and suspend integration audit (before implementation)

The authorized integrated pass now includes actual CPU1–3 MTCMOS hotplug,
shared with the forthcoming system-suspend owner. Original FM Y2
`platform_cpu_die` 0xc001f44c disables cache, flushes it, leaves SMP coherency
and waits; `platform_cpu_kill` 0xc001f3c0 invokes the corresponding
`spm_mtcmos_ctrl_cpu{1,2,3}` (0xc00347c8/0xc00349b0/0xc0034b98).
The original secondary restart writes the boot-ROM resume vector at
INFRACFG_AO+0x800 before restoring that CPU's power. The existing clock owner
must own access to its already mapped INFRACFG_AO registers; do not create a
second mapping. All hardware waits require bounds, and any uncertain domain
state must prevent subsequent deep suspend. The initial four-CPU boot path
remains the physically proven loader handover.

System suspend must use the Linux CPU suspend/CPU-PM context framework and
retain the exact Y2 PCM variant and its host contract. Byte equality alone
is insufficient. IRQ wake, PMIC command ownership, retained infrastructure,
CPU context and observed same-session restore remain mandatory qualification
gates. WFI and s2idle remain available. No CPU-domain or deep-sleep physical
pass exists at this point.

The baseline inode-1 warning matches upstream Linux commit
[4091c8206cfd2e3bb529ef260887296b90d9b6a2](https://github.com/torvalds/linux/commit/4091c8206cfd2e3bb529ef260887296b90d9b6a2),
which initializes ext4's dynamic inode state at allocation on 32-bit systems.
The exact three-file backport is included in the integrated source pass.
This explains the warning path; final filesystem qualification is still needed.

## Integrated candidate source contract

The source pass implements the decisions above in the production kernel and
production rescue image. It has not yet been flashed. The
[source reconciliation receipt](evidence/m4-power-source.json) binds the exact
597-word suspend and 28-word normal PCM programs to the retained FM Y2 binary;
the host sequence comes from the pinned BSP and actual Y2 CPU power functions.

`innioasis,y2-spm` owns SPM, Linux CPU/cluster PM owns architectural and GIC
context, and the existing clock provider owns the boot-ROM restart vector.
The selected system mode is **CPU/cluster shutdown with infrastructure retained**.
Its PCM handles DRAM retention; software preserves MCU_BIU_CONTROL and uses
`cpu_suspend`/`cpu_resume_arm`. It does not implement infrastructure power removal
or vendor CIRQ mirroring. Power/RTC/CHRDET share the retained, latched PMIC level
IRQ; direct USB detection, thermal/system requests and a bounded 600-second PCM
timer are enabled wake conditions. Wake correctness, retention and actual energy
saving are physical gates, not consequences inferred from a successful build.

The sole PWRAP owner programs the stock SPM command slots 5–7 for VPROC register
0x220, all at 1.15 V, before suspend. Charger engines and watchdog must already
be off. Runtime OPP voltages remain 1.15 V. Runtime cpuidle remains WFI; no
unqualified dormant state is advertised as an idle state. s2idle is retained.
While actively charging, the one-second kernel watchdog service keeps suspend
inhibited. Screen blanking, WFI and offline CPU1–3 power removal reduce activity
without disabling charger safety. No charging-specific deep suspend is claimed.

Early normal-boot gating defers MSDC/eMMC/SD, I2C wheel/DAC, AFE/card and the
volume-key scanner. Rescue's one-way `normal_boot` handover reprobes these through
the ordinary driver core, then exposes normal ECM/ACM before root discovery.
The offline UI waits for CPU1's display initialization to complete before
parking secondary CPUs. It draws activity without a percentage, blanks after
eight seconds, shows status on a short Power press, and requires a fresh
two-second hold and release plus three valid voltage samples at or above 3.4 V
to boot. Source removal always requests hardware poweroff. Full/hold/recharge
remain owned by the kernel, independent of this display process.

Charging below 3.2 V is provisionally limited to 70 mA; between 3.2 and 3.4 V
the limit is at most 450 mA, then the detected-source limit applies. This is
conservative relative to LK's early 450 mA and stock source-selected precharge.
It is **not a demonstrated deeply discharged recovery path** until the physical
test establishes gain. The stock 3-hour top-off and 24-hour total bounds remain,
with a separate bounded precharge phase. Full requires consecutive CV/comparator
and voltage confirmations; recharge uses 4.110 V with consecutive confirmation.
No termination-current threshold is used without a validated current conversion.

BATON retains raw acquisition and stock-equivalent PMIC die/hardware shutdown;
the original Y2 software temperature is fixed at 25 C and its BATON high-temperature
comparator is disabled. This is not pack Celsius or proven pack thermal
protection. ISENSE/BATSNS offsets and charger-path versus net battery current
remain uncalibrated. Retained stock Y2 voltage/resistance tables contain
95 entries per temperature profile, but load/rest, aging and current compensation
are not validated. `temp`, `current_now` and `capacity` therefore stay unavailable;
the UI never fabricates them. These optional telemetry gaps do not excuse any
missing charging, low-battery, suspend or poweroff acceptance gate.
