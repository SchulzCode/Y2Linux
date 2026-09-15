# Session handoff — 2026-09-15, M5 entry audit

**M5 entry audit completed; no M5 deployment candidate. M4 remains
ACTIVE/PARTIAL with a current charging failure.** Start with the
[fresh physical result](../hardware-evidence/2026-09-15-m5-entry/README.md) and
[complete connectivity entry audit](../knowledge/m5-connectivity-entry.md).
This supersedes the September 13 handoff and older issue #31 entry wording.

## Repository and live platform

Entry HEAD was `bbbbbf20e96860089b015c19501b7d39638c5419`, equal to origin/main,
with prior POWER-02 USB/evidence changes. Those reviewed changes are preserved
in `ef285f1`. Configured Git author/committer and authenticated GitHub account
`SchulzCode` are unchanged. No attribution trailers were added.

Current BOOTIMG is built from `83d475ef71a3dd84f6e3cc483a7637b13520e4f7`:

| Artifact | Bytes | SHA256 |
| --- | ---: | --- |
| `out/y2linux-m4-power-02/BOOTIMG.img` | 5378048 | `1852dfc995953f86ef5c47349515e8d6f06478daa6c1a89efe0f223501c226c9` |
| `out/y2linux-m4-power-02/fallback/BOOTIMG-previous.img` | 5361664 | `1c144874577e5cc103ee88aea346ece1e2a20d0b521054560acb7b84d0c4c706` |
| Retained Storage04/06 `Y2ROOT.img` | 536870912 | `814a5b2543931e01cee2eb6f641c3b6e02317bd6308d2d663aea78f618bd554f` |

The first two local files were checked; the complete installed BOOTIMG
image-length readback also matches. Y2ROOT hash is the retained package identity,
not a fresh live-root hash: the installed USB startup edit is additive. The
CHARGE-01 fallback failed net gain at 70 mA; it is not a qualified charging fix.
No images were rebuilt and no install/flash is requested.

Live Linux is `6.18.0-y2linux-m4-power-02`, internal p5/p7 ext4 read-write,
no SD block device, four CPUs, `Y2Audio`, USB ECM/ACM and strict owner-key SSH.
The apparent host-key change was resolved against the previously retained exact
September 14 fingerprint; default known_hosts was left untouched. Private pin
and receipts are in ignored `evidence-private/20260915-m5-entry/`.

## Material new evidence

Charging stopped at uptime **747.328296 s**, sample **4.200073 V**, with
`Y2_FAULT_VOLTAGE=0x8`. At the fresh 1363.10-s inventory it remained inactive;
voltage was about 4.067 V. Source latches that bit at the 4.2-V software guard.
No fault clear, charging setting or physical state was changed. This is a
separate failure from September 14's OVP `0x10`, which is still unexplained.

SPM `entries=0 resumes=0` on this boot. RTC still reports 2082; CPU/PMIC thermal
accuracy remains unresolved. Thus M4 is not a completed power platform with one
small qualification residue. Preserve its implementation and finish its gates
within #30; do not rebuild the power platform under M5.

Own stock metadata confirms CONSYS_MT6582/BTIF with TX/RX DMA and wake IRQs.
The older-board/FM conclusion stands. Silicon HVR/FVR and analog-die stepping
are not yet confirmed. Relevant five-file stock firmware metadata/hashes are
recorded; patch `_1` loads before `_0` by header sequence. Generic modem firmware
matches the external 592-record cold-calibration experiment, but external
calibration is not usable for this unit.

Own NVRAM and PROTECT_F/S were acquired read-only after exact image/geometry
checks; bytes and identifying hashes remain private. Current Y2DATA lacks
Android `/data/nvram`. The raw backup format/record mapping and valid radio
addresses are unresolved. Do not assume that missing plaintext paths mean data
loss. No protected writes, Android boot or external calibration replay occurred.

## Continue at the actual boundary

1. Resolve the current #30 charging failure and retain real M4 acceptance;
   current evidence does not meet the owner's M5 entry requirement.
2. Decode this unit's protected calibration records without writing them and
   establish actual controller/revision/address ownership. Use the retained
   inventory; do not repeat M1/M2/M3 research or firmware provenance from zero.
3. The subsequent M5 implementation must use one kernel connectivity owner,
   AHB/cfg80211 Wi-Fi, STP/BTIF/HCI Bluetooth, BlueZ, a lightweight ALSA audio
   bridge, persistent Y2DATA state, explicit PM and real coexistence/recovery.
   Current Buildroot has none of these radio packages enabled.
4. Only a complete integrated candidate gets the owner's 26-field first
   deployment handoff. Stop for manual deployment, then one coherent physical
   qualification session and targeted fixes for observed failures.

M5 cannot close. FM reception is excluded for this older board. GPU/lima,
Y2PlayerNative, final UI, streaming apps and OTA implementation remain outside
this session. All physical flashes belong to the owner.
