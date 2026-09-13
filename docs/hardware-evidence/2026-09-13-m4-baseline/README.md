# M4 entry, read-only live Storage06

Captured with the existing owner SSH identity, strict server-key verification,
against local source `ae5819f`. `capture.json` records UTC time, command exit
codes, byte counts and compressed-file hashes. This capture changes no target
file, PM state, mixer, raw register, calibration or storage content.

Four CPUs, MemTotal 954376 KiB, internal p5/p7 ext4 mounts, Y2Audio, USB presence
ONLINE=1, VPROC 1.15 V, VGP2 1.8 V and ARMPLL 1.04 GHz are observed. Missing
optional `/etc/y2linux-release` causes identity exit 1; the preceding identity
reads succeed. `/proc/config.gz` is absent (exit 1); use the retained build
configuration for source availability, not as a live configuration dump.

The compressed dmesg contains 11014 lines; 11011 are removable-host command
diagnostics. `dmesg-filtered.txt` removes only `msdc_track_cmd_data:` lines.
The ring no longer contains the initial boot; use the separately retained
Storage06 owner boot evidence. No new filesystem-error or whole-session
stability guarantee is inferred from this snapshot.

A subsequent bounded read used the existing regmap debugfs `registers` file.
The actual v6.18 debugfs implementation was reviewed first: this map's fixed
10-byte lines and seekable offsets allow reading just the selected registers,
without reading intervening clear-on-read status. CID 0x100 returned 0x2023.
CHR_CON0..23 at 0x000..0x02e returned:

```text
000=0063 002=00f2 004=0084 006=001e 008=000f 00a=0000
00c=0001 00e=0005 010=0000 012=0000 014=0060 016=0000
018=0000 01a=0010 01c=0000 01e=0001 020=0005 022=0000
024=0000 026=0000 028=0022 02a=0024 02c=0001 02e=0090
```

PMIC thermal efuses 0x63a/0x63c were read without writes. Calibration enable is
set; E2 decoding gives VTS 3910, calibration temperature 27 C and slope 0.
These are sensor calibration coefficients, not a measured temperature. No
private donor fixture or NVRAM/calibration storage was accessed.

An initial read of 0x800/0x80a..0x816 returned zero. **These are not RTC time
registers**: upstream MT6323 RTC uses 0x8000, beyond the current map's 0xffe
maximum. No RTC state/value/persistence is established by those zeros.

The charger current selector 0xf means configured 70 mA and CV selector 0x1e
means 4.175 V in the pinned BSP tables. CHR_EN and CSDAC_EN are clear. No actual
current, battery voltage, capacity, thermal temperature or charging success was
measured. See [the architecture reconciliation](../../knowledge/m4-power-platform.md).
