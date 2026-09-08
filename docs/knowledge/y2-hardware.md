# Y2 hardware evidence baseline

Date: 2026-09-08. Related research: Y2E-101/105/110/115/120. Scope: one current device plus explicitly identified historical captures. CONFIRMED always means confirmed in the cited input/session, not every Y2 revision.

| Status | Fact | Evidence | Implication |
| --- | --- | --- | --- |
| CONFIRMED, current | MT6582, ARMv7, Cortex-A7 part 0xc07 with NEON/VFPv4 | `20260908-stock/cpuinfo.stdout` | Linux target family supported by evidence; complete Linux board support not established. |
| CONFIRMED, current | Kernel reports version 3.4.67; Android 4.4.2/API 19 | `version.stdout`, selected getprop in [baseline](stock-system-baseline.md) | Stock-family system only; exact installed image is UNKNOWN. |
| CONFIRMED, current | 7,784,103,936-byte internal eMMC user region and separate SD | [partition map](partition-map.md), capture `20260908-partitions` | Respect hardware regions and vendor metadata differences. |
| CONFIRMED, historical | CS43131 and AW87559 driver bindings on I2C bus 1 | Verified snapshot `2026-07-29_004935/hardware/i2c-identities.txt` | Separate headphone and speaker dependencies; wiring and analog behavior remain open. |
| CONFIRMED, historical | APT32F and UPDATE driver names on I2C bus 0 | Same snapshot | Name/binding does not establish controller function or firmware protocol. |
| CONFIRMED, historical | `pmic_mt6323`, input/controller nodes | Verified snapshot platform/input inventories | Useful discovery anchors for fresh passive subsystem research. |
| UNKNOWN | Physical PCB/FM revision, exact installed boot/system lineage, trusted battery readings | [recovery assessment](recovery.md), current conflicting battery service output | Required before selecting a recovery baseline. |

The archive contains engineering reports rather than electrical verification of all routes. The current session did not re-run audio/FM experiments or scan hardware buses. See [evidence provenance](evidence-index.md) for hash validation and superseded claims, [open unknowns](open-unknowns.md) for blockers, and subsystem documents for next proof requirements.
