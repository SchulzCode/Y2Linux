# CONNECTIVITY-09: permanent E2 HCI page correction

Source `f55fff48926c578fcfcd84ef8074d0693b3442e3`, kernel `6.18.0-y2linux-m5-connectivity-09`.
BOOTIMG 6150144 bytes, SHA256 `200346dff20ea1ffe769278bc8c52a19402d850a380c2054f6e93d1ef167de7a`.
Retain rootfs `2025.02.17-connectivity.7` and existing Y2DATA; fallback -08
BOOTIMG SHA256 `5b8e333cdb9fff7bf314abf905ad11a7f26d23e8dab4fd4a4cf78a0bd58b0d78`.

Fifteen targeted connectivity tests pass. The new test executes the native
quirk helper together with Linux's actual extended-feature reply handler and
page iterator: it reproduces page-2 ENOSYS, repairs fresh/stale setup state,
preserves feature/command bytes and error status, and excludes other silicon.
The emitted DT is byte-identical to the -08 DT used by that suite, SHA256
`d87a17246c7412155e4c268266799e7de9521ec7f7b1c23005b824c63b65562d`.

One clean production kernel build, ARM observer selftest, emitted config/D08,
DT, memory, sleep ABI, BOOTIMG and BOOTIMG-only package validation pass. No new
compiler warnings; the three retained donor Wi-Fi warnings are unchanged.
No Y2ROOT/Y2DATA rebuild, new firmware input or broad M4/source/ROM qualification.

[The live -08 runtime correction](../../../hardware-evidence/2026-09-17-m5-connectivity08/README.md)
already reaches standard hci0/BlueZ and successful power-on. This -09 BOOTIMG
makes the same controller-specific correction in the driver; its normal-boot
result is pending owner installation. No test module is packaged.

[Installation and continued SSH check](../../y2linux-m5-connectivity-09-deployment.md).
