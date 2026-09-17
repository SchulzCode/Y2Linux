# CONNECTIVITY-09: MT6582 E2 local extended feature page limit

CONNECTIVITY-08 physically clears the RF-result EPROTO. The next exact failure
is HCI Read Local Extended Features (`0x1004`) page 2: this controller's page-1
reply advertises maximum 2, but page 2 returns status `0x30`. Linux maps that
status to `-38` and aborts initialization before BlueZ exposes the adapter.
[Own hardware trace and successful runtime test](../hardware-evidence/2026-09-17-m5-connectivity08/README.md).

After successful vendor setup/reset, apply Linux's existing
`HCI_QUIRK_BROKEN_LOCAL_EXT_FEATURES_PAGE_2` only to chip `6582`, HVR `8a01`,
FVR `8a00`, and cap `max_page` at the verified page 1. Setting the quirk alone
does not lower a false value cached by an earlier failed initialization. The
normal nonpersistent setup callback repeats the correction after function
power loss. No HCI feature or supported-command bitmap is edited. EDR, LE,
status checking, WMT framing/patch/calibration, factory, DMA and M4 stay intact.

The targeted regression combines the actual native quirk helper with the
pinned Linux extended-feature reply handler and page iterator. It reproduces
unfixed page-2 `-ENOSYS`, verifies fresh and previously failed HCI state, retains
all feature/command bytes and unrelated quirks, preserves negative status,
and leaves other controller triples untouched.

The live -08 diagnostic first reached standard hci0/BlueZ and accepted runtime
power-on. The owner-installed -09 production BOOTIMG now verifies the same
result on normal boot, without any diagnostic module or runtime correction.
Standard BlueZ power-on/off succeeds with zero core errors/recoveries and
kernel taint 0. [Physical -09 result](../hardware-evidence/2026-09-17-m5-connectivity09/README.md).
CONNECTIVITY-07 Y2ROOT/Y2DATA and the -08 BOOTIMG fallback are retained.
No Wi-Fi fix or full pairing/audio/coexistence qualification is implied;
M5 remains open. Do not repeat settled identification/factory/DMA work.
