# Session handoff — 2026-09-16, CONNECTIVITY-02 physically inspected

The owner installed CONNECTIVITY-02. Read-only SSH confirms it is running.
Calibration now completes **53 FS exchanges instead of zero**, then MD firmware
signals `MD_EX` (`control=0, id=4, check=0x45584350`). MD shutdown reports success.
No usable Wi-Fi/BlueZ adapter exists. Early regulatory loading no longer fails.
[Physical result and limits](../hardware-evidence/2026-09-16-m5-connectivity02/README.md).
**M5 stays ACTIVE / STARTUP FAILURE / PHYSICALLY UNQUALIFIED.**

## Current artifacts

- Source: `dd8446259880f60e5eb96125145bb7b623fb5929`.
- Kernel: `6.18.0-y2linux-m5-connectivity-02`.
- Package: `out/y2linux-m5-connectivity-02/`; BOOTIMG-only update, already installed
  by the owner. No new request to flash this same image.
- BOOTIMG: 6150144 bytes; SHA256 `72b17b90d5ab21c2c52f957056f483f3af9949f0607ecc98c4c87c89aa7765f9`.
- Root remains `2025.02.17-connectivity.1`, built from `022c701010c467904ab6025cd98535d3b861c771`;
  image SHA256 `add6b375d33dbfa3f36e505c9784c934c3b31e2073041f514dcd2db524408e67`.
- Y2DATA is preserved, with networks, bonds, identities and SSH authorization.
- Immediate fallback: `out/y2linux-m5-connectivity-02/fallback/BOOTIMG-previous.img`,
  SHA256 `a24b257a795b8ffea198e57344f20a514f4fa5e7148572ac8331afed9e996a96` (CONNECTIVITY-01, bootable but radios fail).
- Accepted M4 fallback pair remains `out/y2linux-m5-connectivity-01/fallback/`:
  BOOTIMG `66b6ecd5ef54da6f3ea07be2c9deda284d0a7636e9c6da4c5d72d84eca5fc010`;
  Y2ROOT `814a5b2543931e01cee2eb6f641c3b6e02317bd6308d2d663aea78f618bd554f`.

[Build evidence](../build/evidence/y2linux-m5-connectivity-02/README.md): 83 tests,
ARM rescue ABI, artifact validation and 14 package rejection checks passed.
[Targeted correction](../knowledge/m5-connectivity02-corrections.md): bounded MD
request padding normalization, private-buffer cleanup, failure context and early
signed regulatory data. No root rebuild or M4 hardware redesign.

## Continue from this failure

The next correction must identify the modem exception cause/last FS operation;
this build does not expose them. Do not interpret `hci0` registration, partial FS
progress or an MD exception as successful calibration. Once initialization works,
continue the [original coherent M5 qualification](../build/y2linux-m5-connectivity-01-deployment.md)
over existing owner-key SSH at `root@10.42.0.1`.

PC charging has fault 0 and visibly enters/releases voltage hold; internal mounts
and USB SSH work. No full M4 regression series was run. The RTC/system clock still
shows 2022-08-01 and needs setting/persistence verification. Raw captures are
private. No radio toggles, suspend, reboot or protected writes were performed.

M4 POWER-03 remains owner-accepted (#30 closed); #31 stays open. Reuse the completed
entry audit `67cbe8f`; no repeated inventory or NVRAM acquisition. Preserve this
unit's factory data. Firmware provenance, standard userspace versions and the
original installation contract remain in the first deployment receipt.
Do not start FM on this older board, GPU/lima, Y2PlayerNative or OTA implementation.
