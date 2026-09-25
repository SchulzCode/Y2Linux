# Platform v1 telemetry repair — 2026-09-25

This is the separately owner-authorized corrective pass following the
[physical qualification stop](PLATFORM-V1-PHYSICAL-QUALIFICATION.md). No repair
was applied during qualification. The installed image and its Session A FAIL /
B–D NOT_TESTED record remain unchanged. No device access or flashing occurs in
this pass.

## Changes and software validation

The health CLI's network-check import used the name `check`, making the health
function a shadowed local throughout `main`. Give the network function its own
name. Quick/full health now return their actual JSON and exit status, including
filesystem failures; the repair does not convert a failed check into success.

`/sys/power/wakeup_count` is a suspend handshake, not a nonblocking telemetry
counter. Linux `kernel/power/main.c:wakeup_count_show` calls
`pm_get_wakeup_count(..., true)`, which waits interruptibly while a wakeup source
is active (`drivers/base/power/wakeup.c`). The physical receipt found precisely
this wait with the charger source active. Query it using the existing bounded
child-command runner with a 100 ms deadline and 64-byte output limit. A timeout
returns null plus `command_timeout`, terminating and reaping the interruptible
reader; a readable zero remains zero. This adds up to about 100 ms of observation
time while a source stays active. The generic runner cannot guarantee reaping a
process stuck in an unrelated uninterruptible kernel wait. No charger, wakeup,
suspend, governor, voltage or hardware policy is changed.

Host validation: `python3 -m unittest discover -s tests -p 'test_platform*.py' -v`
passes **67 tests**. Four new regressions exercise quick/full failing health,
network dispatch, a real blocked FIFO reader with termination/reaping, and
zero/valid/invalid/missing counters. Receipts are under
`out/y2linux-platform-v1-telemetry-01-build/`. Packaged ARM userspace and image
validation are required before candidate handoff. Neither proves behavior on
the charging Y2; the first physical check after a separately authorized install
must repeat the mandatory baseline and inspect for leftover readers.

## Warning review

Evidence is the original private boot capture, boot ID
`083d4abb-a447-4ac6-a063-e8898c150bca`, installed Linux source
`d04b95aaff713edf943042d97a4c6134ca19fc24`, kernel
`6.18.0-y2linux-platform-v1-candidate-01`. Review uses the pinned Linux 6.18 source
and retained platform source. There are 28 priority 0–4 entries, including
repeated rate-limit notices; this is not a count of distinct crashes. No retained
Oops/panic/WARN stack or ext4/I/O error was found, and taint is zero.

| Message | Source-backed interpretation and remaining gate |
| --- | --- |
| `dummy_timer is not functional` (four) | `kernel/time/tick-oneshot.c:tick_switch_to_oneshot` rejects switching the current dummy clockevent to one-shot. The suffix is a `pr_cont` after `pr_info`, so its isolated priority is misleading. This is not proof of a dead timer, but effective clockevent/idle behavior and boot ordering remain to be checked (#28/#34). |
| Cache hierarchy not detected | `drivers/base/cacheinfo.c` fails cache/shared-CPU map setup. Incomplete cache description, not observed memory corruption. Cache topology and kernel description review remain (#28). |
| `mt6323-led` missing of_node | `drivers/mfd/mfd-core.c` cannot find the compatible child declared by `mt6397-core.c`. PMIC LED description mismatch; do not add a speculative hardware node solely to suppress this warning (#28/#30). |
| Memory protection not selected | `init/main.c:mark_readonly` reports absent strict kernel RWX configuration. This is a real hardening limitation; enabling it requires a separate kernel/memory validation boundary (#28/#32). |
| p8 extends beyond EOD | Retained stock partition-map clipping. Observed p5/p7 geometry, UUIDs, mounts and zero ext4 counters are consistent; no partition rewrite is justified. Durability/recovery remain untested (#33). |
| VCAMA/VRTC `get_mode` unsupported | `drivers/regulator/mt6323-regulator.c:mt6323_ldo_get_mode` logs when `modeset_mask` is zero. These rails expose that unsupported query; debugfs regulator-summary code can call it. This message alone does not prove a failed voltage transition. No rail configuration change; power qualification remains separate (#30). |
| Bluetooth extended features page 2 | `kernel/platform/connectivity/hci.c:controller_quirks` deliberately sets the existing E2 quirk after the own-device page-2 status 0x30 trace. `net/bluetooth/hci_event.c` warns when the controller advertises the inconsistent maximum. Preserve this compatibility workaround. Pairing/SBC/reconnect still need physical evidence (#31). |
| MSDC/OVL/printk suppression | Rate limiting means some diagnostic lines were omitted. It does not by itself identify a storage/display failure, and limits conclusions from the log (#28/#33). |
| Wi-Fi Start command | `kernel/platform/connectivity/wifi/common/wlan_lib.c:1427` uses an unprefixed `printk` for the normal start command. Warning priority is not an association result. Full state/DHCP/DNS tests remain (#31). |

Early temperature ENODATA precedes later CPU/PMIC readings; absolute sensor
accuracy and loaded thermal behavior remain unqualified. Orphan cleanup was
followed by clean filesystem checks and zero error counters. These observations
are retained without claiming power-loss durability or charging qualification.

The candidate keeps the same kernel and therefore does not promise to remove
these messages. Source classification does not waive the owner's warning stop
rule or promote any physical gate. Review the remaining timer/cache/hardening
items before restarting later sessions; stop again on the owner's fault rules.

## Candidate handoff

Pending image and ARM receipts. Intended scope is a preserving root overlay of
the exact UI candidate: two Python files and explicit release/source metadata,
with unchanged Reborn binaries, hardware policies and required BOOTIMG. The
fallback must be the original UI root, not an older plain Platform candidate.
