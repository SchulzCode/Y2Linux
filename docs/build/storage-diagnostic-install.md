# Storage02 DIAG01 — manual BOOTIMG-only observation

This is a temporary rescue diagnostic for the current Y2 whose production root
resolver fails and whose Linux USB gadget does not enumerate. It reuses the
exact Storage02 kernel, internal-storage guard, hardware DT and display module.
Only rescue content and DT console verbosity/initrd extent change. No root/data
filesystem is mounted or repaired; no normal Buildroot boot is attempted.

The normal kernel still initializes the hardware with its existing command
policy. This diagnostic adds no eMMC write command or new allowed write range.
Its storage reads probe filesystem signatures only at the approved root/data
partition geometry. Protected partition contents are not probed. Live MMC
controller identification/status commands are distinct from filesystem writes.

## Owner flash

1. Verify `sha256sum -c SHA256SUMS` in the diagnostic package.
2. In SP Flash Tool v5.2032 load **this diagnostic package's scatter**. Use
   **Download Only**. Select **BOOTIMG → BOOTIMG.img only**.
3. Every other box stays unchecked, including **ANDROID and USRDATA**, all
   loaders, partition tables, recovery and device-specific partitions.
   Do not format or choose Firmware Upgrade. No assistant operates SPFT.
4. Use the established owner flash entry procedure. Retain the operation log.
   For readback, BOOTIMG is EMMC_USER physical start **0x01d80000**, length
   **0x01000000**. Compare the first manifest payload size bytes to its SHA256.
   The scatter linear/global start **0x03180000 is not the readback offset**.
5. After flashing, **disconnect USB and boot unplugged**. Wait at least ten
   seconds after Linux starts, then attach USB once. Cable-present initial
   startup is rejected by the current driver; this image does not change USB
   lifecycle or claim to fix reconnect. No SD is required for the diagnostic.

## What to send back

After display initialization, three text pages rotate about every twelve seconds:

- **PAGE 1:** cached USB failure stage/code and initial/live cable detection,
  internal controller binding, disk type and capacity.
- **PAGE 2:** internal partition starts/sizes and Y2ROOT/Y2DATA identities,
  followed by the unmodified production resolver's result.
- **PAGE 3:** relevant internal MMC and USB kernel messages.

Please photograph all three pages. The complete pages remain in RAM and are also
sent to the existing ACM log stream if USB works. There is no SSH in this rescue.
The console no longer prints the continuous empty-SD warning traffic, but those
kernel messages remain in the kernel log. No SD automount or formatting occurs.

## Restore the previous BOOTIMG

Use the same BOOTIMG-only scatter and Download Only, but select:
`out/y2linux-production-v1-r2/BOOTIMG.img` from the canonical repository.
Its size is **5187584** bytes and SHA256 is:
`6dfe7ff5de30f39581cc3479ed17d9a2fbf6671619741924d563f29114228d7d`.
This returns to the existing Storage02 rescue/root attempt without changing
Y2ROOT or Y2DATA. The failed root detection is not fixed by restoration.

The alternative known SD-development fallback is BOOTIMG only to
`out/y2linux-m3-audio-02/BOOTIMG.img`, **4816896** bytes, SHA256
`b9dbf6b528e4afbee2a5ffb2fc65df7f6a076564e92960332e24cee3809b5842`,
with the original development SD inserted. This is not a new recovery experiment.

Production Storage v1 stays active. Diagnostic display or SPFT completion does
not qualify internal root, persistence, protected-byte preservation or USB.

## Reproduction

With the verified Storage02 build/package still present and clean committed
source, run `python3 tools/production/diagnostic.py`. It verifies the exact base,
builds a small ARM cached-status reader, validates changed rescue/DT/BOOTIMG and
runs targeted tests in the isolated environment. Output is
`out/y2linux-storage-diag-01/`. It never accesses a physical device.
