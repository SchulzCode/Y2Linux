# CONNECTIVITY-08 build and targeted validation

Source `8b18ca35956bf3287b7d8a15fdb8d4dcddda3d49`, kernel
`6.18.0-y2linux-m5-connectivity-08`; retained rootfs `2025.02.17-connectivity.7`
from `359f7870859c546290d3a56dcdc601585f05d66f`.

BOOTIMG: 6150144 bytes, SHA256
`5b8e333cdb9fff7bf314abf905ad11a7f26d23e8dab4fd4a4cf78a0bd58b0d78`.
[Manual deployment and physical continuation](../../y2linux-m5-connectivity-08-deployment.md).

- Targeted connectivity suite: 13 tests passed before the build; the remaining
  emitted-DT test passed against the completed -08 artifacts (14 total).
  The actual STP/WMT test covers all 637 split points of the extended RF frame,
  byte delivery, CRC/status/shape/controller rejection, sequence/ACK state and
  a following ordinary command. No private RF bytes are in test vectors.
- The identical new regression rejects unchanged CONNECTIVITY-07 at the
  extended-response assertion. See `prior-source-rejection.log`.
- One clean production kernel build; no new warnings relative to -07. The
  three retained donor Wi-Fi warnings are unrelated and unchanged.
- Emitted kernel/config/D08, DT, memory bounds, sleep ABI and BOOTIMG checks pass.
  Existing verified userspace/libraries are retained; standard rescue helpers
  are rebuilt by the production builder. No Y2ROOT/Y2DATA image is rebuilt.
- Production BOOTIMG-only manifest, geometry, retained component identity,
  module ABI, bounds and hashes validate. The -07 BOOTIMG fallback is included.
  Unchanged packaging rejection tests and broad M4 qualification are not repeated.

This is a source/build result. The own-unit -07 capture proves the original
failure; -08 hardware progression to BlueZ/cfg80211 requires owner installation.
No flash, protected write or milestone closure occurred in this build step.
