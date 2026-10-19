# Error reports

When something goes wrong on your player (no charging, a crash, sound or display trouble), an error report gives
the maintainer the player's logs and technical details in one file. You decide whether to send it.

## Save a report

1. Settings → **About & Updates** → **Error Reports** → **Save Report**.
2. Choose **Internal Storage** or **SD Card** (only the ones the player can write to right now are listed).
3. Choose what the problem is about (Sound, Display, Battery or Sleep, Bluetooth, Wi-Fi, Crash or Freeze,
   Something Else).

The player collects its logs and removes personal details, which takes about ten seconds. The file goes into an
**Error Reports** folder on the storage you chose:

```
Internal Storage/Error Reports/y2linux-report-20261019-143205.tar.gz
SD Card/Error Reports/y2linux-report-20261019-143205.tar.gz
```

## Send it

Connect the player to a computer, copy the file from the **Error Reports** folder over MTP (or take out the SD
card) and attach it to your issue on [GitHub](https://github.com/SchulzCode/Y2Linux/issues). A report is usually
about half a megabyte. Nothing leaves the player on its own: in this beta reports are only saved, never uploaded.
Sending them over Wi-Fi comes with a later update, and only if you turn it on.

## What is in a report

- **System:** the kernel log, the crash record of the previous start, the system log, memory, processor, sleep,
  power and storage statistics, the state of every driver and service.
- **Power:** battery and charger readings (voltage, charger state and faults), the power and sleep journals.
- **Updates:** update and recovery state and logs.
- **Reborn:** its logs, its last crash, its live status, and your settings (volume, EQ, output and so on).
- **Hardware:** the processor, display, storage chip and bootloader versions of your player.

## What is never in a report

Your music (no titles, artists, albums, file or folder names), playlists, Wi-Fi network names and passwords,
Bluetooth device names, your Last.fm account, SSH keys, serial numbers, your IP addresses and device addresses
(only the maker part of a Wi-Fi or Bluetooth address stays). The player looks up
these values before it writes a report and replaces them everywhere they appear, then also replaces anything that
looks like an address, an e-mail, a path to your music or a serial number. A replaced value becomes a token such as
`<ssid#3fa2c1>`: the same value gets the same token within one report, so a log still shows that, for example,
the same network came back, but the token can't be turned back into the name.

The report is a normal `.tar.gz` archive of text and JSON files. You can open it and read everything before you
send it.

## For developers

```sh
y2-platform report save --to internal|sd [--topic battery]   # what Save Report runs
y2-platform report status
```

Design and privacy rules: [ERROR-REPORTS-PLAN](planning/ERROR-REPORTS-PLAN.md).
