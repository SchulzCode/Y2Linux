# Y2LINUX-M4-CHARGE-01: manual BOOTIMG handoff

**Candidate, not physical charging acceptance.** The installed M4-ADC-01 was
inspected read-only. Owner manual flashing is the next boundary. No root/data
image changes or Buildroot update: retain Storage06 Y2ROOT/p5 and Y2DATA/p7,
SSH credentials, production/rescue architecture and M3/M4 components.

[Charger implementation and reconstructed stock contract](../knowledge/m4-charging.md).
[Final build identity, size and SHA256](evidence/y2linux-m4-charge-01/README.md).

## Artifact and fallback

Candidate: `/home/luca/Dokumente/Code/Y2Linux/out/y2linux-m4-charge-01/BOOTIMG.img`.
The package's `manifest.json` and `SHA256SUMS` record the final bytes and source
commit. They include the exact preserved root/data reference identities.

Fallback: `out/y2linux-m4-charge-01/fallback/BOOTIMG-previous.img`, the installed
M4-ADC-01, 5355520 bytes, SHA256
`c3c9778e396413fced69364190eb892456dd71c66a0c23fa650be2dcd30e7461`.
It restores the inhibited acquisition kernel; it does not provide charging.
The original copy remains `out/y2linux-m4-adc-01/BOOTIMG.img`.

## Owner SPFT procedure

1. Keep the first test attended at normal room temperature. Use the existing
   working computer USB connection; an unclassified wall adapter cannot charge
   under this first input policy. A reading below 3.4 V will inhibit; deeply
   depleted recovery is not yet qualified. Do not leave the old uncharged kernel
   running while preparing the host.
2. In the existing SP Flash Tool, load this package's
   `MT6582_BOOTIMG_only_scatter.txt`. Select **Download Only**.
3. Select **BOOTIMG only**, pointing at this package's `BOOTIMG.img`.
   Every other row must be unchecked, including PRELOADER, LK/UBOOT, MBR, EBR,
   ANDROID, USRDATA, recovery and NVRAM. Do not use Format or Firmware Upgrade.
4. With Y2 normally off/disconnected, click Download; connect it using the
   already proven owner flashing connection procedure. Wait for success,
   disconnect, then boot normally with the existing internal root/data.
5. Connect the computer USB cable and tell the agent that M4-CHARGE-01 is running.
   Charging starts automatically only after host configuration and driver checks.
   Observe the player during this first start. If the case becomes unexpectedly
   warm, there is a smell/swelling, or the session becomes unobservable, unplug USB.

Fallback uses the same scatter/Download Only/BOOTIMG-only selection, replacing
only BOOTIMG with the fallback path above. No root/data restore is required.

## Agent SSH qualification after owner deployment

The agent runs these commands; the owner does not need to paste a long sequence.
Use the existing owner key and verified host key, never disable host checking.
The current verified temporary known-host file is `/tmp/y2-m4-battery-known-hosts`;
if unavailable, re-establish host trust through the existing owner ACM procedure.
Expected host fingerprint:
`SHA256:pzFq5XLjtTuhCFeK66aocsEBoAmBKVo6ZlBqdQWSxEU`.

Initial SSH inspection and immediate standard-policy inhibit:

```sh
ssh -T -o StrictHostKeyChecking=yes -o UserKnownHostsFile=/tmp/y2-m4-battery-known-hosts -i ~/.ssh/y2linux_ed25519 root@10.42.0.1 '
  uname -r
  echo inhibit-charge > /sys/class/power_supply/BAT0/charge_behaviour
  cat /sys/devices/system/cpu/online /proc/meminfo /proc/mounts
  cat /sys/class/power_supply/BAT0/uevent
  cat /sys/class/power_supply/BAT0/charging_state
  cat /sys/class/power_supply/y2-usb-input/uevent
  cat /sys/class/power_supply/y2-usb-presence/uevent
  dmesg | tail -100
'
```

Expected release `6.18.0-y2linux-m4-charge-01`, CPUs 0–3, ~954044 KiB RAM,
internal p5/p7 ext4, present battery, 3.4–4.175 V for starting,
`current_max=500000`, fixed current/CV 70000/4175000. Raw BATON should remain
non-saturated near its prior 10388 counts; PMIC/CPU die temperatures are separate.
After inhibit, `active=0`, `stop_error=0`, and charge-engine bits3/4 are clear
in the diagnostic `chr_con0`. A fault requires diagnosis; do not repeatedly
unplug/replug to defeat it. Pack `temp`, `current_now` and `capacity` stay absent.

Run the checked-in bounded observer through SSH, retaining its output on the
host. It validates admission, uses normal `charge_behaviour=auto`, samples every
5 seconds for **120 seconds**, and inhibits on exit/error. It does not service
watchdogs or write raw hardware. If SSH fails, the owner unplugs USB.

```sh
ssh -T -o StrictHostKeyChecking=yes -o UserKnownHostsFile=/tmp/y2-m4-battery-known-hosts -i ~/.ssh/y2linux_ed25519 root@10.42.0.1 'sh -s' \
  < tools/development/monitor-charging.sh
```

During each observation require Status=Charging, `active=1`, no fault/ADC/stop
error, fixed limits, plausible voltages, and an increasing watchdog-pet count.
Observe temperature trends and the closed case; no heating, chilling, stress
load or unattended charging. Compare medians from the early and late samples;
a single voltage rebound when charging starts is not proof of stored charge.
Repeat a bounded window only if the initial evidence is healthy and a longer
trend is needed. Lack of net gain at 70 mA is a result, not permission to raise it.

Then perform this one coherent sequence, retaining `/tmp` observations through
ACM if USB reconnect prevents SSH:

1. Repeat a bounded observation with display off using the already qualified
   standard backlight `brightness=0`; save/restore its original value. Verify
   the framebuffer restores and audio playback/stop/playback remain usable.
   Do not introduce a display-driver bypass to get a current result.
2. Select `charge_behaviour=auto` after reviewing the bounded logs. Owner unplugs
   USB once; observe through ACM `online=0`, `active=0`, Status=Discharging,
   cleared charge-engine bits. Owner reconnects once; wait for host enumeration,
   `y2-usb-input/current_max=500000`, then verify charging restarts with increasing
   WDT count. A reconnect transport failure is distinct from PMIC charging state.
3. With charging active, `echo mem > /sys/power/state` should return EBUSY while
   watchdog servicing continues. With `inhibit-charge`, run the established M4
   s2idle/wake check and inspect root/data, display and audio after resume. Manual
   inhibit remains selected; `auto` restores eligibility. Deep sleep is not added.
4. Only after healthy charging logs, test one normal reboot and inspect the same
   fixed policy after boot. Test orderly PMIC poweroff and owner-observed USB
   insertion from off. Record whether the retained boot chain starts Y2Linux;
   do not claim low/dead-battery recovery or deliberately deep-discharge for it.
5. Leave `inhibit-charge` selected while any abnormal result is unresolved.
   Once the attended tests pass, normal production `auto` policy can remain
   selected. Physical charging success still requires credible net battery gain.

Immediate inhibit command at any point:

```sh
ssh -T -o StrictHostKeyChecking=yes -o UserKnownHostsFile=/tmp/y2-m4-battery-known-hosts -i ~/.ssh/y2linux_ed25519 root@10.42.0.1 \
  'echo inhibit-charge > /sys/class/power_supply/BAT0/charge_behaviour'
```

There is no calibrated battery-temperature window, net battery current, SOC or
verified full indication. PMIC die thermal shutdown is preserved; it does not
establish hot/cold pack coverage. Charging through screen-off/WFI is intended;
active charging excludes s2idle to keep the 4-second watchdog serviced. The first
profile's 3.4 V floor and configured-host requirement bound low-battery recovery.
These limitations and the physical test results determine whether the broader
owner goal of permanent Android-free charging has actually been reached.
