# Power evidence and reporting discrepancy

Date: 2026-09-08. Related tasks: Y2E-101/105. Status: power baseline incomplete.

CONFIRMED historical observation: `pmic_mt6323`, battery and battery-meter platform nodes in the verified 2026-07-29 snapshot. Current CPU topology and `/proc/meminfo` were captured, but governor/idle/power behavior was not measured.

CONFIRMED current observation: `dumpsys battery` in `20260908-stock/battery.stdout` returns conflicting repeated fields: status 5 and 4, present true and false, level 100 and 50. It also reports USB powered, voltage 4237 and temperature 250. Do not select one duplicate or promote these service values into validated physical battery voltage/temperature/charge state. The cause could be service modification or reporting logic; it has not been established.

UNKNOWN: trusted power_supply values/units, gauge/charger identity and calibration, battery health, safe charging limits, thermal sensor mapping, cpufreq/cpuidle, runtime PM, suspend/resume and wake sources. The service anomaly blocks a trustworthy recovery-power precondition, not all passive research.

Next proof: inspect the source of duplicated framework output and narrowly selected standard power_supply metadata without changing charge, suspend, governor or display state. Do not read arbitrary vendor debug/register files merely because they are exposed under sysfs.

## M2 USB supply prerequisite — 2026-09-09

Source/binary CONFIRMED: FM uses AP PWRAP WACS2 at physical `0x1000d000` to
read MT6323 CID (`0x0100`) and VUSB (`0x0502`); the latter's software enable is
bit 14 and reported enable bit 15. The USB init requests fixed 3.3 V VUSB.
This is an internal USB supply and does not establish host VBUS sourcing.
[Exact locators, guards and candidate contract](m2-pwrap-probe.md).

Hardware status remains UNKNOWN after normal Linux handoff. One bounded
identity/status candidate is specified to use only inherited initialization and
PMIC read commands, never rail/charger writes. It must refuse a missing/busy
transport and mismatched chip identity. Full PWRAP provider initialization,
shared input/display/SD rail ownership and battery/charger telemetry remain open
under #23; the conflicting Android battery fields above remain unresolved.
There is no evidence that a VUSB enable report measures actual supply voltage or
satisfies the basic battery/charger reporting required for M2 exit.


## Returned M2-PWRAP-01 hardware evidence

[Owner photographs](m2-pwrap-hardware-result.md) confirm RC=0/VALID=3,
CID=0x2023, VUSB=0xc000 and continued execution through BEAT 50. This resolves
the narrow inherited AP read-transport/VUSB-enable observation for this boot.
It is one cached snapshot, not repeated sampling, voltage measurement or battery
telemetry. Earlier pending/unknown statements describe the original handoff.
Next: [read-only USB clock handoff snapshot](m2-usb-clock-probe.md), with the
source-identified timer IRQ-name parser correction. USB ownership remains blocked.


## Clock hardware result and next USB prerequisite — 2026-09-09

The [clock trial](m2-usb-clock-hardware-result.md) confirms another successful
PWRAP CID/VUSB snapshot and enabled inherited USB clocks, with no reported errors
through BEAT 50. Supply voltage, consumer regulator control and battery/charger
telemetry remain unqualified. [Next read-only USB state probe](m2-usb-state-probe.md)
preserves the inherited rail/PLL state; it makes no PMIC/clock/calibration write.
