# SD card as a USB drive (Disk Mode for the SD card)

Owner decision 2026-11-26: people dislike MTP (macOS needs an extra app, drive-based tools such as rsync, beets
or foobar's sync don't work, big folders list slowly). Start with **the SD card only**; Internal Storage stays
ext4 and on MTP, and is **not** reformatted.

## What the listener gets

Settings → Library → Storage → **SD Card as USB Drive** (a switch, off by default). With it on, connecting a
computer:

- Internal Storage appears as before (MTP, "Y2"), and music on it keeps playing.
- The SD card appears as a **USB drive** (a card reader, "Y2Linux SD Card"), on Windows, macOS and Linux without
  any app. While the computer has it, the player cannot read the card: songs on it stop, the SD card leaves the
  library views, and Storage says "On the computer".
- **Eject** the drive on the computer (or unplug the cable): the player checks the card as on every insertion,
  mounts it again and scans what changed.

Off (the default) behaves exactly as before: the SD card is a second MTP storage.

## Why only FAT32/exFAT cards

The computer mounts the filesystem itself. Windows and macOS cannot read ext4: an ext4 card is never lent and
stays on MTP. Internal Storage is ext4 for the same reason and stays on MTP.

## How it works

Platform (`y2-platform`):

1. **Setting.** `y2-platform usb-sd-disk on|off` writes `/data/system/platform/usb-sd-disk.json`
   (`{"enabled": true}`); missing or unreadable is off. The USB service reads it every pass and reports it in
   `usb-transfer.json`'s new `sd_disk` object, which is how Reborn reads it.
2. **Gadget.** With the setting on, the MTP composition gains a mass-storage function (`mass_storage.0`, one
   removable LUN, empty at first): `ffs.mtp`, `mass_storage.0`, `ecm.usb0`. `bcdDevice` becomes 0x0101 so Windows
   does not reuse the cached interface list of the MTP-only device. The composition changes only while no
   computer is attached, as the mode does today; the kernel gets `CONFIG_USB_CONFIGFS_MASS_STORAGE=y`.
3. **Lending** (`storage::media::lend`, driven by the USB service each pass):
   - *Lending*: a computer configured the gadget, the platform's own SD mount is FAT32/exFAT and not in its
     background check. The record `/run/y2/sd-lent.json` is written first. It takes the SD view away from the
     MTP responder (live, no re-enumeration), and Reborn drops the SD source (its songs stop, files close).
   - *Lent*: the card is unmounted (plain `umount`, never lazy or forced) and its partition becomes the LUN's
     backing file. A card still busy after 30 s is not lent for this connection (`sd_in_use`), and it stays
     with the player.
   - *Back*: the computer ejected it (the LUN's file is empty), the cable was pulled, the card was taken out
     or the setting was turned off: forced eject of the LUN, remove the record, and ask `media reconcile` to
     admit the card again (read-only check, then mount; a card pulled without ejecting is handled by the
     existing check: read-only with Repair when it finds damage). A card the computer ejected is not lent
     again until the cable is plugged in again.
   - While the record exists, `media` operations, repair, format/grow and reconcile refuse with
     `sd_on_computer` (nothing on the player touches a card the computer writes).
   - If the service restarts with the record present, the LUN is emptied and the card admitted again.
4. Sleep is already refused while a computer has the gadget configured (`charger_or_usb_active`).

Reborn:

- `storage::sources` leaves the SD card out while the receipt says Lending or Lent; the observation reports the
  card as not present, so a song on it pauses as when the card is pulled.
- `SdCard::OnComputer` (Storage: "On the computer · Eject it there"); the computer page names it too.
- The switch on Storage runs `usb-sd-disk on|off`; shown only when the platform reports `sd_disk`.

## Not in this step

- Internal Storage as a drive (needs exFAT and a wipe; owner: not now).
- ext4 cards as a drive (computers can't read them).
- Making Disk Mode the default: only after device and Windows/macOS/Linux checks.

## Checks owed on the device

Windows, macOS and Linux: the drive appears, copying, Eject → the player plays the new songs; unplug without
ejecting → check finds the dirty card (Repair offered if needed); playing from the SD card when connecting;
toggling the switch while connected (applies on the next connection); MTP still works for Internal Storage in
the same connection; charging on a PC port.
