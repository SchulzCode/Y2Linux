# CONNECTIVITY-06: build passed, factory provider blocks this boot

Source `ef97cbc975f6be441d4e3cd7345233c015e9de31`; BOOTIMG-only package
`out/y2linux-m5-connectivity-06/`, 6150144 bytes, SHA256
`c724c9e25aa9bf8a5523af125e7e7eb5702146f52b109e85de1fa5a730cd196e`.
87 production/M4/connectivity tests (56 + 31), ARM rescue/artifact validation,
and 14 package rejection cases passed. The actual DMA send/IRQ regression
reproduces the old two-byte stall and passes the per-transfer rearm correction.

The owner installed CONNECTIVITY-06. Authenticated SSH at uptimes 31.14 and
102.62 seconds confirms this kernel and retained CONNECTIVITY-03 root. This time
internal root/data are `/dev/mmcblk1p5` and `mmcblk1p7`; `/dev/mmcblk0` is absent.
The only whole disk reports 15203328 sectors and sysfs type `MMC`. The factory
provider's hardcoded `/dev/mmcblk0` fails its geometry check before protected reads.
Its log says `y2-factory: production logical eMMC geometry mismatch`.

Consequently `activated=0 calibrated=0`, no radio IRQs, no Wi-Fi or BlueZ adapter.
**The TX fix has not been physically exercised on this boot.** Kernel/SSH and
internal mounts are working; no crash signature was captured. A live logging SSH
connection ended with status 255 while separate subsequent SSH inspection worked;
no reboot, resume or radio success is inferred. No service restart/alias/protected
write or assistant deployment occurred. Raw logs are private under
`evidence-private/20260916-m5-connectivity06/`.

Y2ROOT remains CONNECTIVITY-03, hash
`1495cc1825ca3831fc4607cf25bc04969495f6bdfa7842d634d0917ecb42824e`.
The preserved previous BOOTIMG is CONNECTIVITY-05, hash
`505a83986b062bedf99163f4f013e96f5ef02c3d6a3ac79e8f56857ea8a6a096`.
Y2DATA is untouched. CONNECTIVITY-07 targets mounted-eMMC identification in the
provider; that userspace correction requires a preserving root update. M5 stays open.
