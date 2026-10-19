# Y2Linux 1.0 Beta 8.9 (label 102, not published)

Status: **BUILT (label 102) and installed on the owner's player; NOT PUBLISHED** (2026-10-19). The owner then chose
to continue the error report uploader (R3/R4) on `main`, so the next release is a new build from there. Release `1.0.0-beta.8.9`, Reborn
`1.0.0-beta.8.9`, kernel `6.18.56-y2linux-1.0.0-beta.8.9`, update sequence 7100, Linux `0e0760f`, Reborn `4da5f84`.
Built with `tools/production/release.py` (135 s, resumed workspace, update based on `baseline-101`): container suite
**792 OK**, host suite 819 OK, Reborn 543 tests and strict Clippy; all seal checks PASS, no MediaTek firmware in any
image. Milestone audit BETA-8.9 in [roadmap-gap-audit](../planning/roadmap-gap-audit.md).

| Image | SHA-256 |
| --- | --- |
| BOOTIMG | `fc8e71b9c4484fe80a3cdc663ec84c05c3217d2f945ca6d3f9fe9148a757c1c4` |
| Y2ROOT | `f7c7ff3596d186eefe74ec1dd0a7d325c25d6863b2209ce41bda464fcb9f4cec` |
| RECOVERY | `57f77eb599fe52435108f41b6831da60a399d381d83612b56a880a9f9577273d` |
| Y2DATA (no SSH key) | `d790e47126f1cc6a2382f332368317a338a181d72ad91d588332f39fbc576743` |

## What changed

| Part | Change |
| --- | --- |
| Reborn | About & Updates → Error Reports → **Save Report** → storage, and the report is saved: the topic step is gone. The topic never changed a report's content (`report.py` collects the same sections for every topic; it was only a label in `report.json`), and a saved report goes with a GitHub issue that says what went wrong. Reborn runs `y2-platform report save --to internal\|sd` without `--topic` (`report.json` `"topic": null`); the storage rows lose their chevron. The topic picker stays for Send Report Now (hidden until there is an uploader), where it is the only thing the owner tells the receiver. Reborn `c0c5e04`. |
| Platform | No code change; `--topic` stays optional. |
| User guide | [ERROR-REPORTS](../ERROR-REPORTS.md) without the topic step (with a note for Beta 8.8). Linux `9dc60e6`. |

## Device results (owner's player)

Installed by stage + apply (package hashes equal on the player, signature checked): back after ≈40 s, `PendingHealth`
→ `Acknowledged` 7100, Reborn healthy on `1.0.0-beta.8.9`; `/data/ota-102` removed. The owner then saved a report from
the screens: SD card, 509 KB, `state: saved` at 13:30 UTC (no topic step).

## Known gaps

- Not checked on the player: Save Report to Internal Storage from the screens, a FAT32 card, a full or read-only
  card.
- Unchanged from 8.8: no upload (R3 Worker, R4 uploader), battery current with a profile, output measurements, sleep
  drain (#42), GC9503V confirmation (#45), charging on #47's player.
- One host-suite run before the build had one failure that did not repeat (the rerun and the build's suites passed);
  which test it was is not recorded.
