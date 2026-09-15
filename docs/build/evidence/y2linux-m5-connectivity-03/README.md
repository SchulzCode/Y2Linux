# CONNECTIVITY-03 build evidence (crashing candidate)

Source `1c2b08563c85a3443f8983f59329a2772c767fb8`, kernel
`6.18.0-y2linux-m5-connectivity-03`. Package `out/y2linux-m5-connectivity-03/`.

| Image | Bytes | SHA256 |
| --- | ---: | --- |
| BOOTIMG | 6150144 | `d4e53446d95d428292e1f2892207653350c85d5bf8bc56a9d033349e5643cbb0` |
| Y2ROOT | 536870912 | `1495cc1825ca3831fc4607cf25bc04969495f6bdfa7842d634d0917ecb42824e` |

84 production/M4/connectivity checks, eight isolated ARM userspace checks,
artifact/root validation and 16 package rejection cases passed. Replaying this
unit's first 53 FS requests with a synthetic seed reproduces the old failure and
verifies the path correction on host and Cortex-A7 emulation. The forced local
Buildroot package rebuild installs the corrected calibration binary, SHA256
`098c12fbc028d7f3d7f02dc130e7c31c9a0db386df8a88dde81dcb6542ec02da`.
The helper check receipt names the earlier source `5864250`; the final `1c2b085`
only corrects fallback wording and retains that binary, also verified over SSH.

**Physical failure supersedes these offline checks:** MD calibration completes,
then Linux crashes when CONN starts. See the
[physical result](../../../hardware-evidence/2026-09-16-m5-connectivity03/README.md).
Do not deploy this BOOTIMG or recommend it as recovery. Its corrected Y2ROOT is
retained by CONNECTIVITY-04 and later BOOTIMG updates. Y2DATA was preserved.

The package's fallback is a matched CONNECTIVITY-02 BOOTIMG / CONNECTIVITY-01
Y2ROOT pair: Linux boots but radios fail earlier. Hashes:
`72b17b90d5ab21c2c52f957056f483f3af9949f0607ecc98c4c87c89aa7765f9` /
`add6b375d33dbfa3f36e505c9784c934c3b31e2073041f514dcd2db524408e67`.
Use the corresponding root when restoring that pair; the old kernel with the
new calibration helper would still contain the bad CONN remap.
