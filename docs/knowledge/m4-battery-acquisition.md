# M4 charging prerequisite: this Y2's battery sensor

2026-09-14. Charging completion is **blocked before the first active charge**.
The first missing physical fact is whether this Y2's BATON input measures the
pack temperature with the resistor network assumed by its stock firmware.
The owner does not know the battery label or wire count; the device is at room
temperature, without an independent numeric temperature reference.

The source audit and live M4-01 reads are retained under ignored
`evidence-private/20260914-m4-battery/` with their SHA256 manifest. Entry source
was `444e7eda99dc1f7d90d0d591ebda4f3219efe0c9`. Last read in this pass, at
uptime 2074.16 s: BAT0 present, Not charging, 3544189 uV, USB online, configured
70000 uA / 4175000 uV. No new BATON/ISENSE acquisition or charging occurred.
The configured current is not a measurement of current flowing into the pack.

## Why this needs an owner kernel deployment

The live M4-01 driver exposes only channels 3 and 7. Its built-in PWRAP write
policy also restricts AUXADC request register 0x076e to mask 0x88. A new IIO
consumer or module cannot request channels 5/6 through that transport: the
request is rejected with EPERM. PWRAP suppresses unbind, regmap debugfs is
read-only, and this kernel has no kexec/livepatch route. Rebinding the ADC
child does not replace its parent's compiled guard. A competing raw WACS
client or live kernel patch would defeat the intended ownership/protection.

Y2LINUX-M4-ADC-01 is a production sensor extension, still inhibited. It changes
the same ADC driver and admits only two additional request bits (mask 0xe8).
Current, CV, charger enable, watchdog and protection permissions stay unchanged.
No NTC constants are installed as board calibration. It is not a charging
candidate and cannot replace Android for recharging yet. Storage06, existing
root/data and all other M4 drivers are retained. No rootfs update is needed.

## What the new standard interface means

Discover `/sys/bus/iio/devices/iio:device*/name` equal to `mt6323-auxadc`.

| Pin | Channel / result register | IIO attributes | Meaning |
| --- | --- | --- | --- |
| BATON1 | 5 / 0x071a | `in_voltage5_raw`, `in_voltage5_scale` | Nominal pin mV = raw × 1800 / 32768; not battery Celsius |
| ISENSE | 6 / 0x0716 | `in_voltage6_raw`, `in_voltage6_scale` | Nominal pin mV = raw × 7200 / 32768; not amperes |
| BATSNS | 7 / 0x0714 | Existing `in_voltage7_raw`, `in_voltage7_scale` | Nominal battery pin mV = raw × 7200 / 32768 |

Each raw read enables the ADC buffer, clears then sets its request bit, waits
at least 1 ms, polls ready bit 15 for at most 20 ms, and restores the saved
buffer state. Conversion and regmap errors propagate; not-ready data is never
returned as a successful sample. The existing mutex serializes all four ADC
channels, including the die sensor, through the one parent regmap. Existing
hardware trim selection is preserved; absolute accuracy is not independently
calibrated. The raw API deliberately allows endpoint readings for diagnosis.

BAT0 still reports voltage, presence, status, health and inherited configured
limits. No `temp`, `current_now`, `capacity`, time estimate or FULL claim is
added. No watchdog servicing or new charging/suspend/reboot policy is added.

## Evidence already reconciled

Retained stock Y2 kernel SHA256:
`7287acf1b397d243c927dde5c6ef9d5d871c9c158207cdd98e0e820923f4c197`.

* Stock `read_adc_v_bat_temp` at 0xc04cab08 requests channel 5. Its table at
  0xc09d0b04 is a 10 kohm NTC table (10 kohm at 25 C). `BattVoltToTemp` at
  0xc039b194 assumes a 16.9 kohm pull-up, 27 kohm pull-down and 1.8 V supply.
  The public stock temperature functions return a literal 25 C. The compiled
  raw path remains real, but these constants do not prove the board wiring.
* BATON/TDET were enabled (0x000e = 0x0005); BATON hot detection was disabled.
  BATON1 and ISENSE result registers both had ready=0 in two snapshots. That
  proves neither a missing NTC nor zero charging current.
* Stock `battery_meter_get_charging_current` at 0xc039cb00 samples ISENSE and
  BATSNS, trims outliers, and divides their voltage difference by an assumed
  68 mohm resistor. Pin routing, resistor value and channel offset are not yet
  verified. It would at most establish charger-path current until topology
  proves that all battery current flows through the sense resistor.
* This unit's own 0x063a/0x063c words (0x46da/0x800f) calibrate PMIC die
  temperature only. Neither those nor the SoC fuses calibrate an unknown NTC
  network. No calibration/NVRAM was written or another device's fixture used.
* No working hardware coulomb counter/SOC path was established. Stock FG
  current/columb functions are stubs; displayed percentage uses a software
  battery model. SOC is a separate follow-up.

Primary source for channels, request/ready and scales:
[pinned MediaTek MT6323 AUXADC implementation](https://android.googlesource.com/kernel/mediatek/+/d53dd75c3ff77cac3f5be58fddfe660e94f94d64/drivers/misc/mediatek/power/mt6582/pmic_mt6323.c).
Stock binary extracts and the fuller audit preserve the separate Y2-specific
evidence. No other board's calibration is treated as this board's data.

## Smallest controlled experiment after manual deployment

The agent performs the commands over SSH after the owner reports the new
kernel booted. The sampling script performs no sysfs writes, direct PMIC
access, charging, RTC operations or persistent device writes. Standard IIO
reads execute the bounded ADC requests in the kernel.

1. Confirm the kernel is `6.18.0-y2linux-m4-adc-01`, Storage06 root/data are
   mounted, BAT0 is present/Not charging and current/CV remain 70 mA/4.175 V.
2. Capture 30 samples on a room-temperature surface using
   `tools/development/sample-battery-adc.sh`. It records BATON and ISENSE,
   bracketing them with BATSNS samples to show sequential measurement drift.
   On ADC error, saturation, battery absence or voltage below the existing
   3.4 V observation cutoff, stop. This cutoff is not a pack protection limit.
3. If the readings are usable, the owner holds the closed player normally for
   about a minute; acquire another bounded set, then allow it to return to the
   surface. No heater, cold source, opening, pack manipulation or stress load.
4. Look for a repeatable response consistent with an NTC. Fixed/saturated
   readings or no discernible response leave the path UNKNOWN; they do not
   automatically prove absence. A plausible room-temperature voltage alone
   cannot establish the NTC resistance curve or resistor values. A calibrated
   ambient/reference comparison or board evidence may still be necessary.
5. Assess paired ISENSE/BATSNS offset with charging disabled. This alone does
   not calibrate a 68 mohm resistor or prove net battery-current measurement.

Example agent capture from the host repository (retain strict host-key checking
and the established verified host-key file if the owner's default is stale):

```sh
ssh -T -o BatchMode=yes -o ConnectTimeout=5 -o StrictHostKeyChecking=yes \
  -i "$HOME/.ssh/y2linux_ed25519" root@10.42.0.1 \
  'timeout 75 sh -s -- 30' < tools/development/sample-battery-adc.sh \
  > out/m4-battery-room.tsv
```

## Gates remaining even if BATON responds

There is no validated temperature window yet. Extract the actual stock Y2
temperature policy and reconcile its assumptions against the sensor/pack;
Sprout's example thresholds are not automatically Y2 limits. Retain the
conservative 70 mA / 4.175 V target unless contrary physical evidence requires
stopping. Current sensing and percentage must not block a safely bounded
policy if independent regulation/termination can be justified.

The live USB gadget advertises self-powered / 0 mA. BC1.1 is inactive and
CHRDET gives presence only. Total system input demand and a valid USB budget
are not established by the 70 mA charge selector. The battery OVP enable is
set but its inherited selector 0 threshold is unresolved; input OVP selector
15 maps to 10.5 V in the BSP. PMIC thermal hardware power-down enable is clear.
Preserve the existing protections while resolving these facts; do not label
unarmed protection autonomous or a high OVP threshold a valid USB budget.

The charger watchdog is enabled with a BSP nominal 4 s timeout and needs
verified driver ownership before charging. Active lifecycle, fault handling,
suspend strategy, termination and recharge rules still need implementation.
Stock's 150 mA termination threshold cannot be used with a 70 mA setpoint.
Powered-off USB insertion/recovery has not been physically qualified. None of
these open gates is solved by adding raw ADC channels.
