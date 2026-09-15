# CONNECTIVITY-05: two-byte DMA tail stalls initial WMT command

Authenticated SSH confirms `6.18.0-y2linux-m5-connectivity-05` and retained
CONNECTIVITY-03 root. The same boot ID remains responsive at uptimes 56.57 and
124.63 seconds, with no captured Oops/panic. The owner installed the image;
this inspection made no assistant deployment, protected write or radio toggle.

At 48.782865 seconds MD reports `stage=2 FS=841 restore=1 open=0 result=0
poweroff=0`. Remap verification and MMIO chip ID `6582` follow. The first WMT
opcode `08` times out in mandatory STP mode after four seconds:

```text
TX en=1 flag=0 wpt=1a rpt=18 valid=2 left=8190 flush=0
RX en=1 flag=0 wpt=0 rpt=0 valid=0
```

DMA has consumed 24 of the framed command's 26 bytes; the two-byte trailer is
still queued. All BTIF/DMA/wake IRQ counters are zero. No `wlan0`, no `iw dev`
result and no BlueZ adapter from `bluetoothctl list`. Sysfs `hci0` registration
and post-recovery `error=0 powered=0 functions=0x0` are not radio readiness.
Automatic recovery count is 1, calibration is retained, and both saved radio
preferences remain off. HVR/FVR/stepping, networking and Bluetooth remain unqualified.

Internal p5 root/p7 data and BlueZ/BlueALSA persistence mounts are present.
USB SSH works. PC charging reports Charging, 450 mA setting and CV 4.175 V.
No full M4 regression/suspend/audio test is inferred from this short inspection.
Raw captures are mode-0600 files under `evidence-private/20260916-m5-connectivity05/`;
no radio identity, traffic, credentials or factory contents are published.

[CONNECTIVITY-06](../../knowledge/m5-connectivity06-corrections.md) targets the
missing per-transfer TX interrupt rearm and stock DMA submission order.
M5 #31 stays open; no broad audit or firmware inventory is repeated.
