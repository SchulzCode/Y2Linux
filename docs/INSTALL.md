# Installing Y2Linux on the Innioasis Y2

There are two ways in. Most people use the **release images**: flash once with SP Flash
Tool, copy one file from Innioasis's own firmware package onto the player, done. After
that every update comes over Wi-Fi. Developers can also [build everything from
source](BUILDING.md) and flash their own personalized package (with their SSH key).

Read the whole page once before you start.

## What you need

- A computer (Windows or Linux) and a USB cable.
- **SP Flash Tool v5.2032**, MediaTek's flashing program, with its bundled `MTK_AllInOne_DA.bin`.
- The **Y2Linux release**: the `…-first-install.zip` of the newest
  [GitHub release](https://github.com/SchulzCode/Y2Linux/releases) (1.0 Beta 1 or later).
- The **Innioasis firmware package for the Y2**, v3.2.0 (`y2_v3.2.0_FM-20260813`), from
  Innioasis's download page, or the same build as `rom_y2.zip` from the
  [y1-community stock ROM release 3.2.0-fm](https://github.com/y1-community/y1-stock-rom/releases/tag/3.2.0-fm)
  (checked 2026-10-19: identical scatter file, boot image and LK, and its `system.img` passes the
  player's radio firmware check; it is stored unpacked, about 860 MB instead of 520 MB). You need
  it once, for the radio firmware, and it is also your way back to stock, so keep it. **Only this
  version works:** the player checks every radio file against its SHA-256, so a newer or older
  Innioasis package is refused until Y2Linux knows its files
  ([issue tracker](https://github.com/SchulzCode/Y2Linux/issues) if yours differs).

### Why the Innioasis package?

The Y2's Wi-Fi and Bluetooth chip needs five small programs from MediaTek (the Wi-Fi RAM
code, two Bluetooth patches, the modem image used for radio calibration and a chip
configuration). They are MediaTek's property and may not be redistributed, so **no Y2Linux
image contains them**. Every Y2 owner already has them: they are inside `system.img` of the
official Innioasis package. The player reads them out of that file itself, checks each one
against its known SHA-256 and keeps only those five files (plus two small board defaults it
derives). Nothing is downloaded, and your device's own calibration and MAC addresses are
never touched (they stay in the NVRAM partition).

## What the install changes

| Partition (stock name) | First install | Later updates |
| --- | --- | --- |
| BOOTIMG | Y2Linux kernel + rescue system | replaced by each update |
| ANDROID (`/system`) | Y2Linux system (no radio firmware) | replaced by each update |
| USRDATA (`/data`) | Y2Linux data: settings, library, logs, the radio firmware | **kept** |
| RECOVERY | the Y2 recovery image, which installs updates ([why](RECOVERY-AND-UPDATES.md)) | replaced by each update |
| FAT (the player's own storage) | becomes **Internal Storage** at the first start; your files on it are kept or wait for you ([Storage](STORAGE.md)) | kept |
| Preloader, LK, NVRAM, calibration, PROTECT, SECCFG, tables | **never written** | never written |

Stock Android and its user data are erased. The player's own storage is not flashed: at the
first start Y2Linux turns it into Internal Storage, keeping up to about 400 MB of your files in
`Old Player Files` and leaving a larger library untouched until you decide
([Storage](STORAGE.md)). Copying anything you want to keep to a computer first is still wise.

## Install from the release

1. **Download the Y2Linux release zip and unpack it.** Check it if you like:
   `sha256sum -c SHA256SUMS` in the unpacked folder.
2. **Download the Innioasis firmware package and unpack it.**
3. **Open SP Flash Tool** and choose the **Download** tab, **Download Only**. Never use
   *Format All + Download*, *Firmware Upgrade* or *Write Memory*. Load
   **`MT6582_Android_scatter.txt` from the Y2Linux release** (not the stock one). Exactly these
   four rows are ticked and point at the release files:

   | Row | File |
   | --- | --- |
   | BOOTIMG | `BOOTIMG.img` |
   | RECOVERY | `RECOVERY.img` |
   | ANDROID | `Y2ROOT.img` |
   | USRDATA | `Y2DATA.img` |

   Every other row must be unticked. Stop if the tool asks to format, reports a
   partition-table (PMT) change, or shows different addresses.
4. **Click Download**, then connect the **switched-off** Y2 by USB. After about a minute SP
   Flash Tool shows a green tick ("Download OK").
5. **Unplug and press Power.** On the first start the player shows *Setting up storage* for a
   few seconds: the player's old Innioasis storage becomes **Internal Storage** (5.4 GB for
   music; see [Internal Storage](#internal-storage) for what happens to files that were on it).
   Then Reborn starts, and music works. Wi-Fi and Bluetooth say they still need the radio
   firmware.
6. **Restart the player once** (hold Power to turn it off, then press Power again), so
   Internal Storage gets its full space.
7. **Connect the Y2 to the computer again.** It appears like a USB stick (MTP) with
   **Internal Storage** (and *SD Card* if a card is in). Internal Storage contains a folder
   **Firmware**.
8. **From the firmware package, copy the file `system.img` into that Firmware folder** (about
   520 MB from Innioasis's zip, about 860 MB from the community `rom_y2.zip`). Copy, don't move:
   the player deletes its copy afterwards, and you need the original to go back to stock. (If
   it was moved, unpack it again from the zip.)
9. **Wait a few seconds.** The player checks the file, keeps the five radio files, deletes the
   big copy and starts Wi-Fi and Bluetooth; Reborn says *Wi-Fi and Bluetooth are ready*. The
   Firmware folder disappears. That's it.

If Reborn says *That File Isn't the Y2 Firmware*, the file was not `system.img` of the v3.2.0
package: delete it and copy the right one. The file can also go into a folder named `Firmware`
on the SD card. On a player whose Internal Storage is not set up yet (a large old library, see
below) the computer shows a separate **Firmware** drive instead; it has room for the file from
Innioasis's zip, but not for an unpacked copy (about 860 MB, as in the community `rom_y2.zip`):
Reborn then says *Not Enough Space for That File*, and the SD card's `Firmware` folder is the way.

From now on, updates come over Wi-Fi: Settings → About & Updates → Check for Updates →
**Update Now** (Beta 7.1 and later; on Beta 7 and earlier: Download Update → Restart and
Install). The radio firmware stays on the player and is kept
across every update. How that works: [Recovery and updates](RECOVERY-AND-UPDATES.md).

## First boot

The first boot creates the data folders, so it takes a little longer than later boots. If it
stops in the **rescue** screen, the screen shows the reason (for example, the root or data
partition was not found). Turn the player off with a long Power press and check the flashed
rows. Nothing is written automatically in rescue.

## Developers: SSH over USB

The release's data image contains **no SSH key**, so nobody can log in. To use SSH, build a
personalized data image with your own Ed25519 public key and flash it to USRDATA instead of the
release's `Y2DATA.img` (or build the whole package yourself, see [BUILDING](BUILDING.md)):

```sh
python3 tools/production/first_owner.py --help
```

With the player running and plugged in, it shows up as a USB Ethernet adapter (plus MTP).
The device is `10.42.0.1`. Give your computer's new interface `10.42.0.2/24` with no gateway:

```sh
ip -br link                                    # find the new interface (e.g. enp…u2i1 or usb0)
sudo ip address add 10.42.0.2/24 dev <interface>
ssh -i ~/.ssh/y2linux_ed25519 root@10.42.0.1
```

Login is key-only. Useful commands on the device:

```sh
y2-status system --json            # identity, health summary
y2-platform status                 # platform services
y2-platform firmware status        # radio firmware: Ready, Missing, Installing or Failed
y2-platform firmware install /path/to/system.img   # the same as the Firmware drive
cat /etc/y2linux/versions.json
```

## Music

- **MTP:** the player appears in any MTP client with *Internal Storage* and *SD Card*. Copy
  music there.
- **SD card:** FAT, exFAT and ext4 cards mount at `/media/sd`. `y2-media status` shows the
  card; `y2-media format --confirm-cid <CID>` reformats it as ext4 (it erases the card).

### Internal Storage

The full explanation for users is [Storage](STORAGE.md). Every player has the same layout: system (`Y2ROOT`), settings and library (`Y2DATA`) and
**Internal Storage** (`Y2MEDIA`, 5.4 GiB ext4, `/media/internal`). Internal Storage replaces the
stock player's own storage partition. Since Beta 7.9 the player sets it up by itself at start
(also on players updated from an earlier beta, at the first restart after the update):

| The old Innioasis storage holds | What happens |
| --- | --- |
| nothing, or only leftovers (empty folders, thumbnail caches) | set up at once |
| your files, up to about 400 MB | they are kept: afterwards they are in *Internal Storage → Old Player Files* |
| more (a real music library) | nothing changes. Reborn asks once; over USB the old files show up **read-only** as *Old Storage* so you can copy them to a computer. **Settings → Storage → Set Up Internal Storage** then sets it up and **erases** them |

It waits for a later start while an update is still being checked or the battery is low.
Every step is recorded, so a power loss in the middle continues at the next start.

```sh
y2-platform internal-media status        # layout, filesystem, mount
cat /run/y2/internal-media-setup.json    # what the setup did at this start
```

## Updating

**Over Wi-Fi (recommended):** Settings → About & Updates → Check for Updates → **Update Now**.
Use the charger or at least half a battery (the player says so before it downloads anything).
One screen shows Download, Check and Install; Cancel stops the download. The player then
restarts, installs (about a minute), restarts into the new version and confirms it; on any
failure it restores the previous one. (Beta 7 and earlier: Download Update → Restart and
Install.) Details: [Recovery and updates](RECOVERY-AND-UPDATES.md).

**With a computer:** flash a newer release or your own package with
**`MT6582_preserve_data_scatter.txt`**. Only **BOOTIMG** and **ANDROID** are ticked; **never
tick USRDATA**, which would wipe your settings, library and the radio firmware.

## Known issues to plan around

- **Dark screen at start (rare).** If the panel stays dark, hold Power for 10 s and start
  again.
- **USB networking after deep sleep.** After a wake the USB connection can enumerate but pass
  no traffic. Power off and on to recover.
- More, with evidence: [current platform state](CURRENT_PLATFORM_STATE.md).

## Going back to stock

In SP Flash Tool, Download Only, load the **stock** `MT6582_Android_scatter.txt`, untick
everything, then tick only BOOTIMG → `boot.img`, RECOVERY → `recovery.img`, ANDROID →
`system.img`, USRDATA → `userdata.img`. This restores factory Android (not your old personal
data) and erases Y2Linux and its data. Never tick PRELOADER, UBOOT/LK, MBR/EBR, NVRAM, PROTECT
or SECCFG.

It does not restore the old FAT format of the player's storage (the stock package has no image
for it, and stock Android cannot read Internal Storage): **copy your music to a computer first**;
stock Android then has to format that storage ([Storage](STORAGE.md#going-back-to-stock)).

**This full restore has not yet been tested by the project.** The partitions it writes are the
same ones the install wrote, and the bootloaders are never changed. The detailed procedure and
its caveats: [production recovery](architecture/production-recovery.md).
