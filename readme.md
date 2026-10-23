# Y2Linux

Linux 6.18 and a Buildroot userspace for the **Innioasis Y2** music player
(MediaTek MT6582), together with the native music player
[Y2Reborn](https://github.com/SchulzCode/Y2Reborn). It replaces the stock
Android system on the internal eMMC and leaves the bootloaders, calibration and
factory partitions untouched.

> **Status: Y2Linux 1.0 Beta 8.26 (Baseline 125, 2026-10-23), public beta.** It runs every day
> on the maintainer's Y2. Install it once from the ready-made images of the
> [GitHub releases](https://github.com/SchulzCode/Y2Linux/releases) with SP Flash Tool
> ([INSTALL](docs/INSTALL.md)); after that the player updates itself over Wi-Fi. Only one
> hardware revision has been tested (see [Supported hardware](#supported-hardware)). Read
> [Known issues](#known-issues) before you flash: battery life in sleep is not yet stock-like.

| | 1.0 Beta 8.26 (Baseline 125) |
| --- | --- |
| Git tag (both repositories) | `baseline-125` (previous: `baseline-124`); the GitHub release carries the first-install images, the signed over-the-air update and the GPL sources |
| Release / kernel | `1.0.0-beta.8.26` / `6.18.56-y2linux-1.0.0-beta.8.26` (Linux 6.18 long-term line) |
| Root filesystem | `2026.08-platform-v1.166` (Buildroot 2026.08) |
| Player | Y2Reborn 1.0.0-beta.8.26 (Rust 1.99) with the UI v4 design, Cover Flow, Genres, audiobooks, a sleep timer, a clock, 16 skins, Last.fm scrobbling and 47 headphone profiles |
| Bluetooth codecs | SBC, AAC (fdk-aac-free) and LDAC |
| Main components | GCC 15.3 / glibc 2.44, Mesa 26.2.4, FFmpeg 9.0.2, BlueZ 5.87, OpenSSL 4.0.3, Python 3.15.0 |
| Images built from | Y2Linux `518c641`, Y2Reborn `404edb3` ([Beta 8.26 report](docs/validation/Y2-LINUX-1.0-BETA-8.26.md): no power-off loop on a charger after the battery ran down, Wi-Fi on at the first tap after sleeping with Wi-Fi off, more charging current from simple chargers, fuller error reports; [8.25](docs/validation/Y2-LINUX-1.0-BETA-8.25.md): the headphone DAC's crystal starts as on stock and gets more tries when it does not (#52, #54); [8.24](docs/validation/Y2-LINUX-1.0-BETA-8.24.md): the headphone output runs at 44.1 kHz like stock, so the DAC's PLL is never used (#52); [8.23](docs/validation/Y2-LINUX-1.0-BETA-8.23.md): a frozen update install restarts the player by itself and leaves a trail, the headphone DAC is powered as on stock and the output stays open across track changes (#52, #54); [8.22](docs/validation/Y2-LINUX-1.0-BETA-8.22.md): headphones on players with a slow DAC crystal run as on stock (#52, #54), set the date and time by hand (#53), no "Downloading Update…" forever after an interrupted download, music right after waking, Bluetooth after an interrupted codec switch, sleep after a harmless kernel warning, evidence from update boots; [8.21](docs/validation/Y2-LINUX-1.0-BETA-8.21.md): headphone audio on players whose DAC crystal is slow to start (#52, #54), the crystal run as stock runs it; [8.20](docs/validation/Y2-LINUX-1.0-BETA-8.20.md): first #52 attempt (15 µA bias, 5-s open wait); [8.19](docs/validation/Y2-LINUX-1.0-BETA-8.19.md): audiobooks with chapters, progress, speed and a sleep timer, long M4B books, sharper covers, Settings → Customize (wrap-around lists, screen-off buttons); [8.17](docs/validation/Y2-LINUX-1.0-BETA-8.17.md): headphones after a pause and Next (#51), SD cards Windows reads, Bluetooth disconnect, search and connect, SBC on weak links; [8.16](docs/validation/Y2-LINUX-1.0-BETA-8.16.md): large libraries — All Songs plays, rescans in seconds, tags read on several cores; [8.15](docs/validation/Y2-LINUX-1.0-BETA-8.15.md): Japanese, Chinese and Korean text, a Check for Updates that waits for Wi-Fi after waking, a codec choice that survives connecting; [8.14](docs/validation/Y2-LINUX-1.0-BETA-8.14.md): a new random order for every shuffle, a seek that starts small, Shuffle on genres, long titles on Home; [8.13](docs/validation/Y2-LINUX-1.0-BETA-8.13.md): songs listed in Strawberry and other libmtp apps over USB, a Wi-Fi connect fix; [8.12](docs/validation/Y2-LINUX-1.0-BETA-8.12.md): no crash loop from SD-card playlists (#49), no silence after EQ changes (#48), sleep after a refused attempt, fuller error reports; [8.11](docs/validation/Y2-LINUX-1.0-BETA-8.11.md): Internal Storage is set up on battery from 25 % and the screen says when a charger is needed; [8.10](docs/validation/Y2-LINUX-1.0-BETA-8.10.md): charging on players whose charger settings came from other software (#47), error reports over Wi-Fi (opt-in), report progress; [8.8](docs/validation/Y2-LINUX-1.0-BETA-8.8.md): error reports saved to Internal Storage or the SD card; [8.7](docs/validation/Y2-LINUX-1.0-BETA-8.7.md): GC9503V players identified from LK; [8.6](docs/validation/Y2-LINUX-1.0-BETA-8.6.md): audio review fixes, no loudness jumps, less CPU with a profile; [8.5](docs/validation/Y2-LINUX-1.0-BETA-8.5.md): headphone level, headphone profiles, DAC filter; [8.4](docs/validation/Y2-LINUX-1.0-BETA-8.4.md): the dark display after a restart; [8.3](docs/validation/Y2-LINUX-1.0-BETA-8.3.md): the update screens show the seconds left) |

## Getting started

1. **[Install on the Y2](docs/INSTALL.md)**: download the release, flash it once with SP Flash
   Tool, copy `system.img` from the Innioasis firmware package onto the player for Wi-Fi and
   Bluetooth. Updates then come over Wi-Fi ([how they work](docs/RECOVERY-AND-UPDATES.md)).
2. **[Build from source](docs/BUILDING.md)**: host setup, the build, the first-install package.

## What works

Observed on the maintainer's device (candidates up to 49; 49 was installed over Wi-Fi from its
public release, and its Firmware-drive install was tested on the device):

- **Over-the-air updates:** Settings → About & Updates checks once a day on Wi-Fi (or on demand);
  **Update Now** downloads the signed release (Download · Check · Install, with progress, speed
  and time left; Cancel during the download) and restarts into a Y2 recovery image
  that backs up, installs and verifies kernel, rescue and system, then restarts into the new
  version (about a minute; the old version is restored on any failure). It makes room on its own
  by removing old logs; music and settings are never touched. The release images include the Y2
  recovery image this needs.
- **Radio firmware from your own copy:** no image carries MediaTek's firmware; copy `system.img`
  from the Innioasis package onto the player's *Firmware* drive once and it installs the five
  radio files itself. Players that already run Y2Linux keep theirs across updates.
- Software restart works (since 43).

- Boots from internal eMMC: stock preloader and LK, then Linux 6.18 and Buildroot. No SD card needed.
- Display with GPU (DRM + Lima), buttons, click wheel and the Reborn player UI.
- **Skins:** Settings → Skin switches between Reborn's own look and 16 shipped
  skins that restyle every screen (Classic, Nano Glass, Walkman, Terminal, Winamp, Paper,
  Big Type, Cassette, Neon Night, Zune, Bunny Bow, five Mini colours). A status-bar clock
  follows the time zone, found automatically over Wi-Fi or chosen under Settings → Display →
  Time Zone; short messages (a computer connected, files copying, problems) appear in the
  status bar. More can be added by copying a
  skin folder into `Skins` on the player over MTP (an existing `Themes` folder still works). Authoring guide:
  [Reborn skins](https://github.com/SchulzCode/Y2Reborn/blob/main/docs/skins.md).
- Albums in **Cover Flow** (perspective covers with reflections, the cover flies into the
  album page; the grid stays available in Settings → Display), and **Genres** in the main list
  (genre → All Songs and its albums).
- Album art from the album folder first (cover.jpg, folder.jpg, … and Windows Media Player
  names), the embedded picture when the folder has none.
- Wired headphone audio (CS43131 DAC), S16 stereo at 44.1 kHz as on stock; other rates (48, 96 kHz …) are converted.
- Headphone level Low/Medium/High (Low is the stock level) and the DAC filter under Settings → Sound →
  Output; headphone profiles that correct 47 common headphones and IEMs (AutoEq), more from an `EQ`
  folder ([Headphone output](docs/HEADPHONE-OUTPUT.md)).
- **Error reports** (Settings → About & Updates → Error Reports): the player's logs and technical details, with
  personal data removed. Opt-in: sent over Wi-Fi when something goes wrong or with Send Report Now (Beta 8.10), or
  saved to Internal Storage or the SD card to attach to a GitHub issue; progress on screen
  ([Error reports](docs/ERROR-REPORTS.md)).
- Bluetooth audio; the codec is chosen automatically (SBC, or AAC/aptX/LDAC when the
  headphones support them and BlueALSA offers them). The output follows the headphones:
  Bluetooth headphones take over from the jack when they connect, the jack when they leave.
  AAC runs in the encoder's high-quality mode at the highest rate the headphones accept (AirPods:
  256 kbit/s) and steps down by itself on a weak link.
- Wi-Fi (2.4 GHz) and **internet radio** (Home → Radio): the radio-browser.info directory
  (popular, by country, by genre, search), favorites and your own `.m3u`/`.pls` lists.
- **Last.fm scrobbling** (new in Beta 7.7, confirmed on the maintainer's player; Beta 7.8 fixes Now Playing after a pause): Settings → Last.fm →
  scan a QR code with your phone and allow Y2Reborn. A song counts after half of it or 4 minutes
  of real listening; plays are kept on the player while offline and sent when Wi-Fi is back;
  Now Playing on your profile; an optional Rockbox-style `.scrobbler.log` works without an
  account. The player never sees your password with the QR sign-in, keeps only Last.fm's session
  key in its private storage, and talks to Last.fm over verified HTTPS only. How it works and why
  it is safe: **[Last.fm scrobbling](docs/LASTFM.md)**.
- **Audiobooks:** a Home entry of their own once the library holds a book (Audiobooks/Hörbücher
  folders, `.m4b` files, genre Audiobook); the player remembers your place in every book, moves
  by chapter (M4B/MP3 chapter marks or one file per chapter), plays each book at its own speed
  (0.75–2×) and has a sleep timer. Books stay out of your music. How it works:
  **[Audiobooks](docs/AUDIOBOOKS.md)**.
- **Sound and playback:** a five-band equalizer with presets (Bass Boost, Vocal, Treble Boost),
  Normalize Volume (ReplayGain), gapless playback, crossfade (up to 12 s), shuffle and repeat.
- Search across artists, albums and songs with live results, and a full-screen keyboard (driven by
  the buttons) for search, Wi-Fi passwords and the Last.fm password sign-in. Radio has station logos, Station Info and each
  country's top 10 on top of an A–Z list.
- Playlists (.m3u/.m3u8 from Internal Storage and the SD card, also from a computer) made and
  changed on the player, with a Favorites playlist and a heart; pause and play continue at once.
- USB: MTP (internal storage and SD card) with copy, move, rename and delete from Windows, macOS
  (MTP apps) and Linux, about 30 MB/s to the player and 38 MB/s from it, and SSH/SFTP over USB
  Ethernet for developers.
- **Internal Storage** (5.4 GB) on every player ([how storage works](docs/STORAGE.md)): the stock
  player's own storage is set up by itself at the first start; files that were on it are kept in *Old Player Files* (up to about
  400 MB) or, for a larger old library, shown read-only as *Old Storage* until you choose
  Settings → Storage → Set Up Internal Storage.
- microSD card for music (`/media/sd`): FAT32, exFAT or ext4. Each card is checked read-only
  before use; a card that was only pulled out while in use is used again, a damaged one is left
  untouched and Settings → Storage says *Needs repair · Check it on a computer*.
- Charging over USB (conservative 650 mA default ceiling) with the stock top-off: the battery
  is charged until really full, shows 100 % then counts down 99, 98, …, and keeps its
  percentage across a restart. An offline charging screen when the player is off.
- Deep sleep when the screen times out: the SoC suspends and wakes every 30 min for
  housekeeping, with the stock suspend settings (CPU at 0.85 V, USB PHY off, wheel controller
  asleep). Sleep and USB state are written to `/data` so they survive a restart.
- The display hardware is powered off with the screen, stock hardware clock gating is on,
  and unused PLLs and clocks are switched off for sleep. The battery percentage uses the stock
  fuel-gauge curve. A PC port charges at the stock 450 mA even when the PC is off or asleep.
- In sleep the power-management chip is set to the stock suspend values (audio and unused
  clocks off, references off) and restored on wake.
- Waking with Power lights the screen in about half a second; Wi-Fi and Bluetooth come back
  in the background.
- A dark screen after the shutdown animation (no white or colour noise). The start-up colour
  flash fix (the backlight is off while the display is handed over) is in, but not yet confirmed.

## Known issues

- **Battery life in sleep is not yet stock-like.** About 8.3–9.4 mA asleep (stock: about 1.3 mA,
  ≈0.1 points/h); several stock-parity measures since then still await their overnight
  measurement. The percentage is estimated from voltage; there is no current sensor.
  Beta 8.1 finally powers the panel down at screen-off on players with the Sitronix ST7701S
  panel ([#45](https://github.com/SchulzCode/Y2Linux/issues/45)); its effect on sleep is not
  measured yet. Players with the GC9503V panel keep the old behaviour until it is confirmed there.
- **Two panel types.** The Y2 ships with a GC9503V or an ST7701S panel; Y2Linux identifies it at
  start (`/sys/firmware/y2_boot/metadata`, `/sys/module/panel/parameters/panel_kind`).
- **Rarely a dark display after a restart.** Before Beta 8.4 a restart could leave a display clock
  gate closed for the bootloader, and the screen stayed dark (an update then rolled itself back).
  8.4 reopens it as the last step of every restart; this is not yet confirmed over many restarts.
  If the panel ever stays dark, hold Power for 10 s and start again.
- **USB after a wake from deep sleep** has worked on every wake since 16, but is not yet confirmed
  over many wakes. Power off and on if USB stays silent.
- **Bluetooth:** Beta 2 fixes the reconnect that gave up when the headphones were absent at a wake
  and the transport race behind the radio-start timeouts; both await their device check. AAC is
  negotiated with AirPods but not yet confirmed by listening. Beta 6.1 changes how Bluetooth pauses
  (Beta 6 resumed silently on AirPods); the fix awaits its listening check. There is no
  wired-headphone detection: the jack is used whenever no Bluetooth headphones are connected.
- **Updates frozen at "Step 1 of 4".** Some players' installer stops at its first step
  ([#56](https://github.com/SchulzCode/Y2Linux/issues/56)); the cause is not known yet. Since Beta
  8.23 the player restarts by itself after two minutes without progress and finishes or undoes the
  update; on older versions a forced restart is safe. A frozen install leaves a record for error reports.
- **Headphones on some players (#52, #54).** A few players' DAC crystal starts slowly or stops when
  skipping a lot. Beta 8.25 starts it with the chip powered as stock does and retries a failed start;
  both affected players confirmed it fixed.
- **Install:** the full path from stock has been run on the maintainer's player only. A kernel that
  hangs before its rescue system runs can only be fixed with SP Flash Tool.
- **Copying to the player over USB** became about three times faster in Beta 7.11 (receive DMA);
  pulling the cable during a copy and sleep with it on are not yet tested on this path.
- **Going back to stock** does not restore the player's old storage format: copy your music off
  first; stock Android then has to format that storage. Not yet tested ([Storage](docs/STORAGE.md)).

The full list, with evidence and next steps, is in the
[current platform state](docs/CURRENT_PLATFORM_STATE.md) and the
reports for 1.0 Beta [8.4](docs/validation/Y2-LINUX-1.0-BETA-8.4.md), [8.3](docs/validation/Y2-LINUX-1.0-BETA-8.3.md), [8.2](docs/validation/Y2-LINUX-1.0-BETA-8.2.md), [8.1](docs/validation/Y2-LINUX-1.0-BETA-8.1.md), [8.0](docs/validation/Y2-LINUX-1.0-BETA-8.0.md), [7.11](docs/validation/Y2-LINUX-1.0-BETA-7.11.md), [7.10](docs/validation/Y2-LINUX-1.0-BETA-7.10.md), [7.9](docs/validation/Y2-LINUX-1.0-BETA-7.9.md), [7.8](docs/validation/Y2-LINUX-1.0-BETA-7.8.md), [7.7](docs/validation/Y2-LINUX-1.0-BETA-7.7.md), [7.6](docs/validation/Y2-LINUX-1.0-BETA-7.6.md), [7.5](docs/validation/Y2-LINUX-1.0-BETA-7.5.md), [7.4](docs/validation/Y2-LINUX-1.0-BETA-7.4.md), [7.3](docs/validation/Y2-LINUX-1.0-BETA-7.3.md), [7.2](docs/validation/Y2-LINUX-1.0-BETA-7.2.md), [7.1](docs/validation/Y2-LINUX-1.0-BETA-7.1.md), [7](docs/validation/Y2-LINUX-1.0-BETA-7.md), [6.3](docs/validation/Y2-LINUX-1.0-BETA-6.3.md), [6.2](docs/validation/Y2-LINUX-1.0-BETA-6.2.md), [6.1](docs/validation/Y2-LINUX-1.0-BETA-6.1.md), [6](docs/validation/Y2-LINUX-1.0-BETA-6.md), [5.1](docs/validation/Y2-LINUX-1.0-BETA-5.1.md), [5](docs/validation/Y2-LINUX-1.0-BETA-5.md), [4.1](docs/validation/Y2-LINUX-1.0-BETA-4.1.md), [4](docs/validation/Y2-LINUX-1.0-BETA-4.md), [3](docs/validation/Y2-LINUX-1.0-BETA-3.md), [2](docs/validation/Y2-LINUX-1.0-BETA-2.md) and
[1](docs/validation/Y2-LINUX-1.0-BETA-1.md),
[49](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-49.md) (firmware-free images, flash package, codecs),
[48](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-48.md) (download screen),
[47](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-47.md) (DSI clock gating),
[46](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-46.md) (update space, restart by default, first release),
[44](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-44.md)/[45](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-45.md) (display read lag, installer screen),
[43](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-43.md) (software restart fixed),
[35](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-35.md)–[38](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-38.md) (over-the-air updates),
[34](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-34.md) (shipped themes, battery display),
[33](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-33.md) (themes),
[32](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-32.md) (deep idle during playback),
[31](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-31.md) (artwork and formats),
[30](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-30.md) (storage and decode speed),
[28](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-28.md)/[29](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-29.md) (stock battery percentage and top-off),
[27](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-27.md) (stock panel, VGP2 and VSYS sleep),
[26](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-26.md) (Bluetooth codecs),
[25](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-25.md) (Cover Flow frame gate),
[24](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-24.md) (frame pacing),
[23](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-23.md) (Cover Flow and Genres),
[21](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-21.md)/[22](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-22.md) (display power transitions),
[19](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-19.md)/[20](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-20.md) (PMIC_WRAP and UART1 clocks),
[18](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-18.md) (stock PMIC sleep settings),
[17](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-17.md) (modem gate, faster wake),
[16](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-16.md) (stock sleep parity),
[15](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-15.md) (newest versions),
[14](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-14.md) (persistent sleep diagnostics) and
[13](docs/validation/Y2-PLATFORM-V1-POWER-FINAL-13.md) (stock suspend power parity).

## Supported hardware

One device has been tested: Innioasis Y2, board `eastaeon82_wet_kk`, MT6582,
7.8 GB eMMC, shipped with Innioasis firmware **v3.2.0 FM (2026-08-13)**. The
build and packaging tools check the stock firmware files by SHA-256. A Y2 with a
different board, eMMC size or partition table is **not supported** until someone
verifies it. Please report your device's details in an issue.

## Repository layout

| Path | Contents |
| --- | --- |
| `kernel/` | Kernel configuration, device tree, platform drivers (`kernel/platform`, `kernel/usb`, `kernel/audio`) and the overlay patches applied to Linux 6.18 (`kernel/patches`) |
| `buildroot/` | Buildroot external tree: defconfig, board overlays, local packages (Reborn, connectivity, MTP, updater) |
| `initramfs/` | Boot-time rescue and handover to the internal root filesystem |
| `tools/build/` | Locked, network-isolated kernel build environment (bubblewrap) |
| `tools/production/` | Production build, packaging and validation, plus on-device helper sources |
| `tools/platform/` | `y2-platform`, the on-device platform service and CLI |
| `tools/connectivity/` | Wi-Fi/Bluetooth services and stock firmware provisioning |
| `tests/` | Host and container test suites (`tools/production/tests.sh`) |
| `docs/` | Documentation; start at [docs/README.md](docs/README.md) |

## Licence

Each source file carries its own SPDX licence identifier; the kernel drivers and
patches are GPL-2.0, and Y2Reborn is MIT. Each release carries the corresponding
sources of the packages in its images (Buildroot legal-info and the Linux archive).
The proprietary MediaTek radio firmware is **not** in this repository or in any
image: each player takes it from its owner's copy of the Innioasis firmware package
([INSTALL](docs/INSTALL.md#why-the-innioasis-package)). Bluetooth codecs in public
images: SBC, AAC (fdk-aac-free) and LDAC.

This project is not affiliated with Innioasis or MediaTek. Flashing replaces the
stock system; you do it at your own risk.
