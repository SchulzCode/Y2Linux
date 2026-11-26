# Y2Linux 1.0 Beta 9.3.42: audiobook series, Hörspiele and the SD card as a USB stick

Release `1.0.0-beta.9.3.42`, Y2Reborn `1.0.0-beta.9.3.42`, kernel `6.18.56-y2linux-1.0.0-beta.9.3.42`, update
sequence 19900, root `2026.08-platform-v1.317`. Published as GitHub release `baseline-294` (build 294; test builds 289–293 came
before it on the developer's player). Previous release: [Beta 9.3.41](Y2-LINUX-1.0-BETA-9.3.41.md).

This report is written for people who use the player. A short technical appendix is at the end.

## In one paragraph

Audiobooks groups a series into one row that opens its books in order, by the series tags or by a series folder,
and folders of Hörspiele count as audiobooks. A new, experimental switch lets a computer open the SD card as an
ordinary USB drive instead of through Music Transfer. It is for the SD card only: Internal Storage is still reached
through Music Transfer (MTP) as before.

## What changed

- **Audiobook series** ([Y2Linux #144](https://github.com/SchulzCode/Y2Linux/issues/144),
  [Y2Reborn #185](https://github.com/SchulzCode/Y2Reborn/issues/185)): long series and audio dramas (50 to 200
  episodes) were one flat list. Now books form a series when two or more share
  - a **series tag**: `SERIES` with `SERIES-PART` (Audiobookshelf, Mp3tag, Audible downloads), ID3 `MVNM`/`MVIN`,
    or else the grouping tag (`©grp`, `TIT1`); differences in capitals and spaces don't matter; or
  - a **folder** inside the Audiobooks folder, one folder or one `.m4b` per book:
    `Hörspiele/Die drei ???/001 Der Super-Papagei/…`, `Audiobooks/Discworld/01 The Colour of Magic.m4b`,
    `Audiobooks/Series/Book/Book.m4b`. A folder with a single book stays as it was (an author folder, say).

  The series is one row with "N books · author" and the first book's cover; open it for its books, ordered by
  `SERIES-PART` as a number (1, 2, 2.5, 10), otherwise by name with 2 before 10. It stands where its most recently
  heard book would, so the series you are in stays at the top. Cover Flow shows a series as a cover that opens a
  Cover Flow of its books. Search still finds every book on its own. Settings → Customize → Browsing →
  **Audiobook Series** (on) switches it off. After the update the player reads the tags of the audiobook files once
  more; songs are not read again.
- **Hörspiele**: a folder named `Hörspiele`, `Hörspiel`, `Hoerspiele` or `Hoerspiel`, and the genres `Hörspiel` and
  `Audio Drama`, make audiobooks, as `Audiobooks` and `Hörbücher` did.
- **The SD card as a USB stick (experimental)**: Settings → Library → Storage → **SD Card as a USB Stick**, off by
  default. With it on, a computer gets the SD card as an ordinary USB drive ("Y2Linux SD Card"), next to Music
  Transfer, so tools that need a drive (rsync, beets, foobar2000's sync) work. **Internal Storage stays in Music
  Transfer (MTP)**; making it a drive would mean reformatting it, which this release does not do. Only FAT32 and
  exFAT cards become a drive. While the computer has the card, songs from it stop and Storage says *On the
  computer · Eject it there*. Eject the drive on the computer: the player checks the card, as on every insertion,
  and the new songs appear. The switch applies the next time the cable is connected. Details:
  [Storage](../STORAGE.md#the-sd-card-as-a-usb-drive).

## How it was tested

- **Tests**: the platform's full check, Reborn's tests and the 582 tests of the release images passed. New
  tests: series from tags and folders (one book is no series, `.m4b` files side by side and one per folder, numbers
  as numbers, tags differing in capitals), `SERIES`/`SERIES-PART` from an MP3 and the grouping tag from an M4B, a
  library from before the update re-reading only its audiobooks, the series rows, page, title, Cover Flow and
  switch, the USB stick switch drawn as a switch, the card on the computer on Storage.
- **On the developer's player** (test builds 289–293, then this release), with a Linux computer:
  - A Hörspiele series of three episodes and a tag series spread over two folders appeared as two rows and opened
    in order; Cover Flow showed both levels; a test folder with four books (two `.m4b` side by side, one `.m4b` in
    its own folder, one MP3 book) became one series of four; only the audiobook files were read again.
  - The SD card (exFAT, 128 GB) appeared as a drive about 5 s after connecting and read at 40 MB/s; ejected, the
    player had it back in 4 s; connecting while a song from the card played paused it and closed the card's files.
  - Booting with the switch on gave the drive and, for developers, the USB network together (19.8 MB/s).
- **Not tested**: Windows and macOS with the USB stick, unplugging during a copy, a series with hundreds of parts.

## Updating

Settings → System → Software Update → **Update Now**. Your music, playlists, settings, Wi-Fi networks and Bluetooth
pairings stay. **Restore Previous System** (Settings → System → About, tap Version seven times, then Diagnostics) goes
back to the version you had.

## Known issues

- The USB stick has been tried with Linux only. If Windows or macOS shows it wrongly, or Music Transfer stops
  working with it on, switch it off and send an error report.
- A song from the SD card ends with an error, not paused, when the card is taken out while it plays.
- Videos and sound can drift apart over Bluetooth on some players
  ([Y2Linux #145](https://github.com/SchulzCode/Y2Linux/issues/145)). Videos larger than 1920 × 1080 are too much for
  the player.
- Everything listed for [Beta 9.3.34](Y2-LINUX-1.0-BETA-9.3.34.md#known-issues) still applies.

## Technical appendix

- Series: `reborn_core::books` `Book.series` (`Series { key, name, part }`), `series_folder`, `keep_shared_series`,
  `series_books`; `media.c` `RbMetadata.series`/`series_part` (4912 bytes, pinned both sides); library columns
  `series`, `series_part`, `METADATA_REVISION` 7 with `books_only_reread`; UI filter `series:<key>\u{1f}<name>`.
  Fixtures `book-series.mp3`, `book-series.m4b` (`fixtures.py --series`). Commits Reborn `4ced8f2`, `e6e15e5`.
- USB stick: platform `storage/media/lend.rs`, `service/usb/disk.rs`, gadget `ffs.mtp` + `mass_storage.0` +
  `ecm.usb0` (bcdDevice 0x0101), kernel `CONFIG_USB_CONFIGFS_MASS_STORAGE`. MUSB FIFO table: endpoint 5 for the
  drive, and every IN FIFO bulk-capable (endpoint 2 single, 4 double 512 bytes; 7744 of 8192 bytes): f_ecm's
  full-speed bulk descriptor has no packet size, so a boot whose first bind had the drive put ECM's bulk IN on
  endpoint 4's 16-byte FIFO and the host's `cdc_ether` failed with -32 until the next boot (test build 291).
  Commits Linux `3b726ae7`, `5b7dfecb`, `8e691c91`, `e3c9fd32`; Reborn `e20f225`, `200d697`, `84b19fa`.
- Images built from Linux `cab44d1e`, Reborn `84b19fa`.
