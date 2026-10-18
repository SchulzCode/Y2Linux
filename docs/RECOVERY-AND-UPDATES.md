# Recovery and updates: how the Y2 updates itself

Y2Linux updates itself over Wi-Fi: kernel, rescue system, root filesystem and the Reborn
player, in about a minute, with an automatic way back if anything goes wrong. This page
explains how that works and why the **RECOVERY** partition has to be flashed once. The
technical contract is [platform OTA v2](architecture/platform-ota-v2.md).

## The short version

- A running system cannot safely overwrite itself. So updates are installed by a second, small
  Linux system, the **Y2 recovery image**, which lives in the RECOVERY partition and runs from
  memory while the main system is switched off.
- The Y2's bootloader can start only two things: the normal system (BOOTIMG) or whatever is in
  RECOVERY. Out of the box RECOVERY holds Android's recovery, which cannot install Y2Linux. That
  is why the Y2 recovery image has to be written there **once, with SP Flash Tool**. The
  release's first-install scatter does this in the same pass as everything else.
- After that, every update brings its own new recovery image along, and the player installs it by
  itself. You never need SP Flash Tool again for updates.

## How the Y2 starts

```
Preloader → LK (bootloader) ─┬─ normal start ───────→ BOOTIMG  (Y2Linux kernel + rescue) → system
                             └─ "boot-recovery" set ─→ RECOVERY (Y2 recovery image)
```

The **preloader** and **LK** are Innioasis/MediaTek's bootloaders. Y2Linux never changes them:
they are the part that can always bring the player back with SP Flash Tool. LK looks at a small
message block in the **MISC** partition. If it says `boot-recovery`, LK starts the RECOVERY
partition instead of BOOTIMG. This is the standard Android mechanism, and the only door LK offers
into a second system. (Holding a key at start-up does not open it on the Y2.)

## Why not let the normal system install the update?

- **It is in use.** The kernel, the system partition and the player are running from exactly
  the partitions an update replaces. Writing over them while they run risks a broken
  half-new, half-old system.
- **Power loss.** If the battery dies or the player crashes halfway through, a system that
  updated itself might not start at all. The recovery image is separate: if an install is
  interrupted, the next start goes straight back into the recovery (the MISC request is still
  set), which finishes the job or restores the previous version.
- **Guard rails in the kernel.** The Y2Linux kernel only lets each side write what it must
  ([storage policy](architecture/platform-ota-v2.md)):

  | Running from | May write |
  | --- | --- |
  | BOOTIMG (normal system) | RECOVERY, the MISC message, its data partition |
  | RECOVERY (recovery image) | BOOTIMG, the system partition, the MISC message, the data partition |

  Neither side can overwrite the image it is running from, and neither can touch the bootloaders,
  NVRAM (calibration, MAC addresses) or the partition tables.

## Why RECOVERY has to be flashed once

The stock Android recovery only knows Android update packages. It cannot check Y2Linux's
signatures, does not know the Y2Linux system layout and cannot do the backup and readback steps
below. The bootloader will only ever start BOOTIMG or RECOVERY, and the bootloader is never
changed. So the Y2 recovery image must sit in RECOVERY, and the very first copy can only get
there from outside: with SP Flash Tool, which is how the whole first install is done anyway.

From then on the recovery maintains itself. Each update package contains the matching recovery
image, and the normal system writes it into RECOVERY (and reads it back) before it restarts.
The recovery that installs an update is always the one built with that update.

## What happens during an update

1. **Check** (daily on Wi-Fi, or *Check for Updates*): the player downloads only the small
   `manifest.json` and its signature from the GitHub release and checks them against the
   built-in release key (Ed25519). A release with a lower or equal version number is refused, so
   nobody can push an old, vulnerable version back.
2. **Update Now** (one step since Beta 7.1) needs the charger or at least half a battery; the
   player says so before it downloads anything. **Download**: the three payloads (system, boot
   image, recovery image), each checked against the SHA-256 the signed manifest pins, then
   decoded once to prove they are complete (**Check**). It makes room first if needed (old logs
   only; never music or settings). One screen shows the steps Download · Check · Install with
   progress, speed and time left; Cancel stops the download and changes nothing.
3. **Install**: right after the check the normal system writes the new recovery image into
   RECOVERY, reads it back, sets `boot-recovery` in MISC and restarts by itself.
4. **In the recovery** (the screen shows *Installing Update*, step and time left):
   - verifies the package again;
   - keeps the radio firmware on the data partition (players that ran an image before 49 had it
     inside the system partition);
   - backs up the current system and boot image (compressed, on the data partition). Since
     Beta 8.2 the system backup is usually the signed package of the running version, kept from
     its own install (about 2 s); otherwise, for example after a computer flash, the running
     system is compressed on all four cores;
   - writes the new system and reads it back from the chip, writes the new boot image and reads
     it back (both on four cores);
   - records *pending health* and clears the MISC request, then restarts.

   On the owner's player (Beta 8.2) the installer takes about 15 s (Beta 8.1: 54 s), and an
   update from *Update Now* to the running new version about a minute.
5. **First start of the new version**: it confirms itself only when the player really works
   (Reborn draws frames and answers, storage, display, audio and input are fine). Then the update
   is *acknowledged*. The package's boot images are deleted; its system part stays on the data
   partition (about 42 MB) as the backup for the next update. The backup of the previous version
   stays too, so you can go back once (Settings → Diagnostics). When space is short, the kept
   package is the last thing an update removes; the next install then backs up the running
   system instead.

If anything fails, nothing is half-done: before the write the recovery stops with "nothing was
changed"; after it, the recovery restores both images from the backup; if the new system never
confirms itself, the next start restores the previous one. A low battery during an install makes
the player turn off and wait for the charger, with the request still set.

## What an update never touches

Your music (internal storage and SD card), your settings and library, the radio firmware on the
data partition, the bootloaders, NVRAM (calibration and MAC addresses) and the partition tables.

One thing follows an update once: on a player that still has the stock Innioasis storage, the
first restart after an update to Beta 7.9 or later turns it into Internal Storage, keeping your
files on it ([Storage](STORAGE.md)). It never runs during the update's own health check.

## Undo

The normal system does not need RECOVERY; only updates do. To put Android's recovery back, flash
the stock `recovery.img` to the RECOVERY row with SP Flash Tool (Download Only, that one row).
Updates then stop working until the Y2 recovery image is flashed again. The full way back to
stock is in [INSTALL](INSTALL.md#going-back-to-stock).

## For developers

- Contract, journal states and failure handling: [platform OTA v2](architecture/platform-ota-v2.md).
- Writing only the recovery image to an existing install: [one-time recovery install](architecture/ota-recovery-install.md).
- Releases are made with `tools/update/package.py` and published as GitHub releases; the player
  reads `releases/latest/download/manifest.json`.
