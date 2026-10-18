# Y2Linux 1.0 Beta 8.3 (Baseline 89)

Status: **PUBLISHED as GitHub release `baseline-89`** (2026-10-18). Release `1.0.0-beta.8.3`, Reborn
`1.0.0-beta.8.3`, kernel `6.18.56-y2linux-1.0.0-beta.8.3`, update sequence 6600, Linux `029d93e`, Reborn
`03e6dff`. Built with `tools/production/release.py`: container suite **774 OK**, host suite 801 OK, update suite
(`tests.test_platform_update`) 34 OK, also under ThreadSanitizer and AddressSanitizer/UBSan, Reborn 517 OK
(fmt, clippy clean); all seal checks PASS, no MediaTek firmware in any image. **Not yet run on a player:** the
owner installs it over the air. Milestone audit BETA-8.3 in [roadmap-gap-audit](../planning/roadmap-gap-audit.md).

| Image | SHA-256 |
| --- | --- |
| BOOTIMG | `f274a076a3f3008116723be42a8129ea1efb6eeabfa00dda2802dec036f0eb7f` |
| Y2ROOT | `9c3663f01be5fccbdc1ba19202629db0f83f23f85eececb088e7ebd0996f571b` |
| RECOVERY | `717aa95967ca41d1132d66a53bcb73fdcaeabb5114c92e3ca4a2e194149ccc69` |
| Y2DATA (no SSH key) | `05ffbf1afa1695bdab21bf439374e8498073398b255ecb516931ee5ce7ad1bd4` |

## A time left you can trust

Since Beta 8.2 the installer takes about 15 s, but its screen still said "This takes 1-2 minutes", showed
"Estimating time left" for the first 8 s and then only "Less than a minute left"; the bar gave the backup 37 % of
its length although the backup now takes about 2 s.

| Part | Change |
| --- | --- |
| Installer estimate (Linux `27cf2cf`) | Each recovery run has a plan of its phases with a modelled duration from the owner's player: hashing 30, live-root backup 21, write 10 and readback 7.5 ms per MiB, fixed parts for the kept root (0.3 s), the boot backup (1.7 s) and BOOTIMG (0.8 s). Finished phases rescale the rest by how fast they really ran (another eMMC, a busier CPU); inside a phase with byte progress its own rate takes over. The bar moves with time and never goes back. |
| Progress line | `"<percent> <step> <seconds left>"` in `/run/y2/recovery-progress` and `/run/y2/update-check-progress`; the kernel log's step lines carry `left=Ns`. Older readers use the first two fields as before. |
| Recovery screen | *About 15 seconds left* (fives, as on Reborn's download screen), *A few seconds left* under ten; it counts down between reports and only raises the number when the estimate grew by ten seconds. With an older core it keeps the minutes. The first screen reads *This takes less than a minute.* |
| Reborn (`59a8702`) | The *Checking the Update* step shows the seconds left from the core's line instead of *This takes a few seconds.* |
| Faster verify (`9c7dae7`) | The kept root is hashed on its own core while verify hashes the new package's payloads, also in parallel; the backup step is logged where it starts (8.2 counted the kept-package check under verify: 3.2 s). |
| Phase timings | Each install and restore records its phases in milliseconds in the journal (`timings`: verify, backup_root, backup_boot, system, check, boot; restore, restore_check, restore_boot). |

Model against the 8.2 measurements on the owner's player: live-backup install 23.7 s modelled / 23.6 s measured,
rollback 11.7 / 12.2 s, kept-package install 13.2 s modelled (8.2 measured 14.9 s with the serial hashing).

## What the owner's OTA to 8.3 will show

The player is on 6500 after the rollback test (Beta 8.2 report). Its kept 6501 package is not the running release,
so the queue drops it and the recovery backs up the live root on four cores (about 24 s, estimated as such). The
download check runs in 8.2's core (no seconds yet); the install runs in 8.3's recovery (seconds left). The update
after that uses the kept package. `y2-platform update status` shows `timings` after each install.

## Known gaps

- Nothing of 8.3 has run on a player yet; the estimate, the parallel hashing and the screen text are host-tested.
- Unchanged: the sleep drain with the panel powered down is not measured yet (#42); the GC9503V panel path awaits
  a GC9503V owner (#45).
