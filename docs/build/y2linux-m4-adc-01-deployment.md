# Y2LINUX-M4-ADC-01 — owner boot confirmed, charging inhibited

The owner has already deployed this kernel; **no repeat flash is requested**.
It adds production IIO raw BATON1/ISENSE acquisition under the existing PMIC
owner. [Physical result and charging gate](../knowledge/m4-battery-acquisition.md).
It does not enable charging or report battery temperature/current/percentage.

Source commit: `bbd0664dcbc1190a5e16e3b3f5889f2154cb04fd`.
[Build/package evidence](evidence/y2linux-m4-adc-01/README.md).

| Item | Saved artifact |
| --- | --- |
| BOOTIMG | `out/y2linux-m4-adc-01/BOOTIMG.img` |
| Size | 5355520 bytes |
| SHA256 | `c3c9778e396413fced69364190eb892456dd71c66a0c23fa650be2dcd30e7461` |
| Rootfs/data update | None; preserve Storage06 p5/p7 and existing SSH authorization |
| Fallback | `out/y2linux-m4-adc-01/fallback/BOOTIMG-previous.img` (M4-01) |
| Fallback size | 5355520 bytes |
| Fallback SHA256 | `5b2bdfa784aa790a81cd4c4f495ffcb1bd2dbfd2e26ffa33c0881c67967cb883` |

Reproduction from the clean source commit uses the normal production pipeline:

```sh
python3 tools/production/build.py --output out/y2linux-m4-adc-01-build --reuse-userspace out/y2linux-production-v1-r4
python3 tools/production/boot_update.py --build out/y2linux-m4-adc-01-build --base out/y2linux-production-v1-r4 --fallback-package out/y2linux-m4-01 --output out/y2linux-m4-adc-01
```

For a future owner restoration of this saved package:

1. In its directory, run `sha256sum -c SHA256SUMS`.
2. Use the established SPFT/DA setup and **Download Only**; load
   `MT6582_BOOTIMG_only_scatter.txt` from this package.
3. Select **BOOTIMG only**, pointing to `BOOTIMG.img`. Every other row stays
   unchecked, including PRELOADER, LK/UBOOT, MBR/EBR, NVRAM, ANDROID and USRDATA.
   Do not use Format or Firmware Upgrade.
4. To restore M4-01 instead, use the same BOOTIMG-only selection with
   `fallback/BOOTIMG-previous.img`. Neither image provides active charging.

The bounded SSH sampler and next physical sensor gate are documented in the
linked knowledge page. No RTC setting, suspend, reboot/poweroff, raw-register
write or active charge test belongs to this inhibited sensor qualification.
