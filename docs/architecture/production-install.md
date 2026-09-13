# Manual first installation — Production Storage v1

**Storage04 production candidate, physical no-SD acceptance pending.**

For the already initialized Storage03 device select the preserve-data scatter:
BOOTIMG and ANDROID only. USRDATA/Y2DATA must remain unchecked. This transition
updates platform startup/status tools and moves module ownership into BOOTIMG;
the existing data template is unchanged. Original
Storage01 sparse transport failed3154 and must not be retried. This installs Y2Linux
on this exact Innioasis Y2/MT6582/eastaeon82_wet_kk, retaining stock partition
boundaries. It destroys Android /system and initializes Android /data as Y2DATA.
It does not erase internal FAT/media, CACHE, loaders, tables or protected data.
Read manifest.json, metadata/partition-audit.md, recovery.md and ota-layout.md.
Do not infer physical success from a green SPFT completion indicator.

## Before the first write

Use the owner's proven SP Flash Tool **v5.2032.00**, Download Only, matching
MTK_AllInOne_DA.bin and established power/USB connection sequence. No assistant
runs SPFT. Tool/DA identity and stock paths are in recovery.md. Trusted power,
known entry sequence and independent access to recovery files are operator
prerequisites; do not experiment with bootloader/security/format modes.

1. Run `sha256sum -c SHA256SUMS` inside this package. Use the checked-in
   `python3 tools/production/validate.py out/y2linux-production-v1-r4` for structural,
   scatter, size, filesystem and hash checks. These are host-file operations.
2. Confirm the retained original FM boot.img, system.img and userdata.img against
   manifest restoration_sources. They restore factory Android, **not personal
   Android data**. Keep an independently accessible copy for recovery. If original
   Android settings/apps/data matter, take a full private USRDATA readback first
   (and ANDROID if modified system state matters). No whole-ROM backup is required.
3. Before overwriting, use SPFT Readback EMMC_USER to capture MBR, EBR1, EBR2 and
   the bounded protected ranges listed in metadata/readback-plan.json. Preserve
   before/after files privately. Verify table prefixes against the original FM
   MBR/EBR files; stock scatter + historical map alone do not prove current tables.
   The checked-in readback verifier checks the exact original table prefix hashes.
   Before flashing, run `python3 tools/production/verify_readback.py --package
   out/y2linux-production-v1-r4 --before /path/to/before --before-only` (one line).
   Record DA/tool/device identity and coordinates with the readbacks. If actual
   capacity, starts or tables differ, stop. Never substitute a donor's layout.
4. Keep the working SD and AUDIO-02 BOOTIMG available as the development fallback.
   Power down by the established owner procedure and remove SD for the first
   production test. No SD write, repartition or formatting is part of this install.

## Select the exact profile and boxes

Load this package's `MT6582_Android_scatter.txt` for FIRST INITIALIZATION.
Select **Download Only**. Review every row after loading; use no saved defaults.

| SPFT row | First install | Image | Physical EMMC_USER start | Partition span |
| --- | --- | --- | --- | --- |
| BOOTIMG | SELECT | BOOTIMG.img | 0x01d80000 | 0x01000000 |
| ANDROID | SELECT | Y2ROOT.img | 0x05180000 | 0x33400000 |
| USRDATA | SELECT, erases existing Android/Y2DATA state | Y2DATA.img | 0x40380000 | 0x32000000 |

**DO NOT SELECT** PRELOADER, MBR, EBR1, PRO_INFO, NVRAM, PROTECT_F, PROTECT_S,
SECCFG, UBOOT (LK), RECOVERY, SEC_RO, MISC, LOGO, EBR2, EXPDB, CACHE, FAT,
BMTPOOL. Their file mappings are NONE and download flags false in this package.
No Format All + Download, Firmware Upgrade, Write Memory, partition-table
changes, DA patching or checksum bypass. Stop if SPFT requests formatting,
reports a PMT/table change, rejects the file format or resolves a different range.

For later **preserving reinstall**, load `MT6582_preserve_data_scatter.txt`:
select BOOTIMG and ANDROID only, NEVER USRDATA. Data initialization is not OTA.
A kernel-only reinstall is permitted only with the matching root/module contract.

Use the normal Download tab and image mappings, not raw address-entry writes.
Stock YAFFS_IMG filesystem rows now receive raw ext4 files through SPFT's
ordinary byte-write path. The selected legacy MT6582 DA rejects sparse FILL
chunks (observed3154); do not use the original .spft.img files. Raw image sizes
fit their original partitions and every written byte has a defined SHA256 identity.

SPFT scatter linear addresses are BOOTIMG0x3180000, ANDROID0x6580000 and
USRDATA0x41780000. **They are not physical EMMC_USER readback offsets.** The
stock scatter carries both address fields and SPFT performs its own translation.
No custom DA/global address translation is implemented. Verify physically below.

## Readback before boot acceptance

Read back full target spans using the physical starts above into
`BOOTIMG.bin` (16777216 bytes), `ANDROID.bin` (859832320 bytes),
`USRDATA.bin` (838860800 bytes). Also repeat the before-protected/table ranges.
Capture before normal Linux boot, because root/data journal and host-key writes
will legitimately change filesystem hashes after boot. Keep the tool/DA log.

Run from the repo:

```
python3 tools/production/verify_readback.py --package out/y2linux-production-v1-r4 --after /path/to/after --before /path/to/before
```

The verifier hashes the first manifest.raw.size_bytes of each full partition
readback and compares with the **raw image hash**.
It compares every protected/table before/after range byte-for-byte and checks
original MBR/EBR prefix hashes. BOOTIMG and root tails outside the images are
not image identity; selected partitions may be erased within their stock bounds.
No neighboring-partition write or shifted superblock is acceptable. Full target
readbacks also retain those tails; do not call a prefix check a full backup hash.

On Linux the same physical starts apply to the whole internal EMMC_USER disk;
on a correctly resolved partition block node the image starts at **offset zero**.
Never apply the whole-disk start to a partition node. The resolver checks internal
host11230000, MMC/nonremovable, either the observed15203328-sector Android disk view or
15269888-sector full physical user area, start/size, label and UUID; it
ignores removable host11240000 even if an old card still says Y2ROOT.

## Expected first boot and acceptance

**Storage04 USB startup:** the production kernel now performs checked MT6582
PHY saved-state recovery and permits cable-present startup. There is no longer a
required unplugged-start workaround. Initial enumeration and reconnect must still
be physically verified. The owner's cable-dependent stock power-entry complaint
has no established cause or verified fix; this package does not modify loaders,
PMIC power entry or battery controls. See metadata/storage04-production-platform.md.
Rescue prints its failure and cached USB state on-screen even without an SD card;
it has ACM logging intent but no SSH service before Buildroot handover.

Stock Boot ROM → unchanged preloader → unchanged LK → BOOTIMG Linux6.18 rescue →
internal Y2ROOT and Y2DATA → switch_root → Buildroot. No SD should be present or
required. Rescue logs `Y2ROOT: Production Storage v1; internal-only discovery`
and then the verified devices/switch message. Internal discovery waits up to40s.
A filesystem needing simple safe recovery is checked with e2fsck -p; other errors
stay in rescue, never autoformat or reboot-loop. Retained ACM logging starts
before storage discovery. Rescue has no SSH service; its console shell is UART/
on-device console only, with ACM retaining its single diagnostic writer.

Normal boot should provide usb0 at10.42.0.1/24, initial USB Ethernet, CDC ACM,
key-only root SSH, display/buttons/wheel and ALSA Y2Audio. New client key location
is supplied in the deployment report; host keys are generated on Y2DATA, so
verify the new host key rather than disabling host-key checking globally.
Entropy can delay first SSH. USB cable reconnect remains unresolved; initial
connection is the acceptance target. Native eMMC filesystem behavior is untested.

Record uname, /proc/mounts, /proc/partitions, sysfs host/type/start/size, blkid,
/proc/meminfo, online CPUs, ALSA and dmesg over SSH. Require Linux6.18, four CPUs,
RAM comparable to954384KiB (exact new value measured), display/input/wheel,
initial SSH/ECM/ACM, ALSA, internal writable root/data and no SD dependency.
Perform a small owner-authorized root/data persistence check, plus the product rescue-entry contract
(`y2.rescue=1` on the boot command line) when an explicit entry mechanism is
available; do not corrupt filesystems or introduce a diagnostic-only kernel. Recheck protected data and
retain the recovery path. The package is not qualified until these pass.

External SD is optional user media at /media/sd, with MUSIC, AUDIOBOOKS, playlists
and arbitrary files. No boot automounter or formatter runs. Use `y2-media mount`
for a card's unique supported filesystem; `y2-media unmount` before removal.
The old development card can be reused without erasing it; its Y2ROOT label is
ignored for boot. FAT/ext4 are supported; exFAT/NTFS are not claimed by this build.
