# CONNECTIVITY-04 build evidence

Source `ac833c887d81d883827a734700639fa9be7c9f1e`, kernel
`6.18.0-y2linux-m5-connectivity-04`. Package `out/y2linux-m5-connectivity-04/`.
BOOTIMG-only, 6150144 bytes, SHA256
`31bb8597ed5a10fa089866664172de664b20fe00a2f7525d08cebc73dbb99d31`.

85 production/M4/connectivity tests (56 + 29), ARM rescue ABI/shell checks,
emitted DT/memory/kernel/module/BOOTIMG validation and 14 package rejection
cases passed. The remap regression rejects the previous register offset,
checks preserved fields and refuses a failed write before power-on. The stock
function receipt records the retained kernel evidence for offset `0x310`.

Y2ROOT remains CONNECTIVITY-03 (`1c2b085`), 536870912 bytes, SHA256
`1495cc1825ca3831fc4607cf25bc04969495f6bdfa7842d634d0917ecb42824e`.
There is no root/data payload; all Y2DATA remains in place.

The owner installed this image. [Physical inspection](../../../hardware-evidence/2026-09-16-m5-connectivity04/README.md)
finds no crash through 813 seconds and one bounded radio recovery. MD calibration
completes; the first WMT command times out and radios remain unavailable. These
results establish bounded Linux stability, not M5 completion or full M4 retesting.

The package's mechanically retained previous BOOTIMG is CONNECTIVITY-03, SHA256
`d4e53446d95d428292e1f2892207653350c85d5bf8bc56a9d033349e5643cbb0`;
**it crashes and is not a recovery recommendation**. Earlier matched root/kernel
fallback pairs are recorded in the [session handoff](../../../planning/session-handoff-2026-09-16.md).

Reproduce the isolated package rejection checks:

```sh
PYTHONPATH=. python3 docs/build/evidence/y2linux-m5-connectivity-04/package-rejection-check.py out/y2linux-m5-connectivity-04 out/y2linux-m5-connectivity-03
```
