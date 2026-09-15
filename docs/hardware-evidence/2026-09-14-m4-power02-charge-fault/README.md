# M4-POWER-02: post-reinstall charging fault

2026-09-14, repository `bbbbbf2`. The owner reports that wall charging showed
the animation but did not replenish the battery, requiring a return to stock
to recharge before restoring Linux. The requested SSH inspection confirms
`6.18.0-y2linux-m4-power-02` through both USB descriptors and `uname`.
No installed BOOTIMG hash was acquired; this is release identification.

[Power evidence](power.txt), [structured result and raw hashes](result.json),
[follow-up read-only commands](read-only-check.sh).

## Observed on the present PC-connected boot

The source is recognized as SDP with a configured 500-mA allowance. Linux
programmed 450 mA / 4.175 V and enabled charging. Its history then records:

| Uptime | Battery reading | Driver state |
| --- | --- | --- |
| 15.122924 s | 4.138110 V | Charging, fault=0, watchdog_pets=1 |
| 16.202659 s | 4.183374 V | Not charging, latched fault=0x10 |
| 376.855359 s | 4.112841 V | Same fault, watchdog_pets still 1 |
| 555.53 s | 4.098120 V (fresh sysfs) | Same fault, active=0 |

Six follow-up samples confirm that charging stays stopped despite USB presence
and `charge_behaviour=auto`. The driver accounts for only one active second.
The 450-mA value is a programmed limit, not measured battery current. Voltage
relaxation and ADC variation are not a calibrated current or capacity measure.
This boot has no sustained charging result.

## Confirmed fault and remaining uncertainty

`Y2_FAULT_OVP` (`0x10`) is raised by either input-HV detection at CHR_CON0 bit 7
or battery-OV detection at CHR_CON6 bit 6. The retained BSP accessor/header
definitions agree with these driver checks. No watchdog, temperature, ADC,
voltage-range or PMIC-IO fault is recorded alongside it.

The later register snapshot has CHR_CON0=0x0063 and CHR_CON6=0x0001: both
instantaneous overvoltage flags are clear and both charging engines are off.
The software fault remains latched. The driver overwrites sampled registers
on subsequent polls and its transition log omits these two detector registers.
Consequently this evidence cannot identify which detector asserted, whether
there was an electrical transient, or the exact cause of the earlier wall test.

There is a relevant stock/Linux configuration difference. The retained actual
Y2 `charging_hw_init` at 0xc04cb1dc–0xc04cb1e0 passes selector 1 to
`upmu_set_rg_vbat_ov_vth`; the retained MediaTek BSP labels this 4.3 V. Linux
instead preserves the inherited selector 0, observed as register 0x000c=0x0001.
Its exact threshold remains unresolved in the existing source audit. This is
a diagnostic lead, not proof of causation or justification for raising a
protection limit. See the [stock contract](../../knowledge/m4-charging.md).

## Limits and next blocker

Current loader metadata is `valid=1 boot_mode=0 boot_reason=0 offline=0` with
`normal_boot=1`. There is no `Y2CHARGE` history, `/data/logs` is empty,
`/var/log` points to volatile `/tmp`, and no pstore interface exists. The
pre-reflash wall session is therefore not recoverable from these interfaces;
the owner's observation remains separate from the measured PC-connected fault.

The inspection used existing power_supply interfaces and a bounded read of
the charger's register prefix through the sole PWRAP regmap. No charging-policy,
register, display, storage, RTC or power-state writes, build, flash or reboot
were performed. The prior host key was stale after reinstall; the address was
matched to the direct Y2 USB topology and its current public key pinned in a
temporary file. Strict SSH checking remained enabled and default known_hosts
was untouched. The new fingerprint was not independently confirmed by the owner.

Internal p5/p7 are mounted, but filesystem integrity was not retested. PMIC
41.62 C versus CPU 7.9 C preserves the earlier thermal-accuracy discrepancy.
The optional boot-ID interface is unavailable; no suspend continuity is claimed.

M4 #30 remains ACTIVE/PARTIAL. The next charging blocker is to distinguish and
retain the first input-versus-battery overvoltage trigger and reconcile the
CV/OVP configuration with actual stock behavior. Wall/low-battery recovery,
sustained charging and the remaining power qualification stay open.
