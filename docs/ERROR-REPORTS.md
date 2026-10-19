# Error reports

When something goes wrong on your player (no charging, a crash, sound or display trouble), an error report gives
the maintainer the player's logs and technical details in one file. You decide whether reports are sent: since
Beta 8.10 the player can send them over Wi-Fi if you turn that on, and you can always save one and send it yourself.

## Send reports over Wi-Fi (Beta 8.10 and later)

After updating to Beta 8.10 the player asks once: **Help Improve Y2Linux?** Choose **Send Reports** or **No Thanks**.
You can change it any time under Settings → **About & Updates** → **Error Reports** → **Send Error Reports**.

With it on:

- When something goes wrong (a crash, an unexpected restart, a failed update, sleep, display, Wi-Fi/Bluetooth or
  storage problem), the player makes a report and sends it in the background. At most five a day.
- **Send Report Now** makes one at once and sends it, with its progress on screen (Collect · Clean · Send). Then
  write in your GitHub issue what went wrong and when; the **Install ID** shown on the page lets the maintainer
  find your reports.
- Reports go out only on Wi-Fi, never during an update and never below 15 % battery unless the player is charging.
  Without Wi-Fi they wait (up to three days) and go out later.
- They are sent encrypted (HTTPS) to the maintainer's private storage and deleted there after 180 days. Your IP
  address is not stored. To have your reports deleted earlier, open an issue with your Install ID; **New ID** on
  the page starts a new one.

With it off, nothing is collected or sent.

## Save a report

1. Settings → **About & Updates** → **Error Reports** → **Save Report**.
2. Choose **Internal Storage** or **SD Card** (only the ones the player can write to right now are listed).

The report is saved straight away and holds everything; say what went wrong in the GitHub issue you attach it to.
The screen shows the progress (Collect · Clean · Save). (Beta 8.8 also asks what the problem is about; any answer is
fine.)

The player collects its logs and removes personal details, which takes about ten seconds. The file goes into an
**Error Reports** folder on the storage you chose:

```
Internal Storage/Error Reports/y2linux-report-20261019-143205.tar.gz
SD Card/Error Reports/y2linux-report-20261019-143205.tar.gz
```

## Send a saved report

Connect the player to a computer, copy the file from the **Error Reports** folder over MTP (or take out the SD
card) and attach it to your issue on [GitHub](https://github.com/SchulzCode/Y2Linux/issues). A report is usually
about half a megabyte. A saved report never leaves the player on its own.

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
y2-platform report save --to internal|sd   # what Save Report runs (--topic is optional, only a label)
y2-platform report send                    # what Send Report Now runs: build, then upload
y2-platform report status
y2-platform report work                    # what the platform service runs: triggers, then the upload queue
```

Design and privacy rules: [ERROR-REPORTS-PLAN](planning/ERROR-REPORTS-PLAN.md); the receiver, its limits and
costs: [tools/reports](../tools/reports/README.md).
