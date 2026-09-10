# M3 entry evidence, 2026-09-10

`live-entry.txt` is the narrow read-only SSH check on qualified DEV-02.
The command also attempted `/proc/asound/cards`; stderr was:
`cat: can't open '/proc/asound/cards': No such file or directory`.
No codec/PMIC writes, playback, reconnect test or reboot occurred.

`issues.json` is the pre-activation GitHub snapshot. Owner subsequently explicitly
activated #29 and deferred #27 under #28; current bodies reflect that decision.

`stock-*-proc-audio.txt` are unchanged original Y2Player hardware records from
`out/afe-runtime/2026-07-30_135519/`, retained narrowly for source/routing review.
Their date and Android provenance are distinct from today's Linux device state.
They are not native M3 qualification. See the canonical architecture review.
