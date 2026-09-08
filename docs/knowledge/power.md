# Power evidence and reporting discrepancy

Date: 2026-09-08. Related tasks: Y2E-101/105. Status: power baseline incomplete.

CONFIRMED historical observation: `pmic_mt6323`, battery and battery-meter platform nodes in the verified 2026-07-29 snapshot. Current CPU topology and `/proc/meminfo` were captured, but governor/idle/power behavior was not measured.

CONFIRMED current observation: `dumpsys battery` in `20260908-stock/battery.stdout` returns conflicting repeated fields: status 5 and 4, present true and false, level 100 and 50. It also reports USB powered, voltage 4237 and temperature 250. Do not select one duplicate or promote these service values into validated physical battery voltage/temperature/charge state. The cause could be service modification or reporting logic; it has not been established.

UNKNOWN: trusted power_supply values/units, gauge/charger identity and calibration, battery health, safe charging limits, thermal sensor mapping, cpufreq/cpuidle, runtime PM, suspend/resume and wake sources. The service anomaly blocks a trustworthy recovery-power precondition, not all passive research.

Next proof: inspect the source of duplicated framework output and narrowly selected standard power_supply metadata without changing charge, suspend, governor or display state. Do not read arbitrary vendor debug/register files merely because they are exposed under sysfs.
