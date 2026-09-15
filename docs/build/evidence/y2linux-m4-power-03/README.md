# M4-POWER-03 build receipt — 2026-09-15

Source `16875c4acc813b54c781167883227cf289f4dfcd` builds kernel
`6.18.0-y2linux-m4-power-03`. **Physical deployment/acceptance is pending.**

Build output: `out/y2linux-m4-power-03-build/`.
Package: `out/y2linux-m4-power-03/`.

| File in package | Bytes | SHA256 |
| --- | ---: | --- |
| `BOOTIMG.img` | 5408768 | `66b6ecd5ef54da6f3ea07be2c9deda284d0a7636e9c6da4c5d72d84eca5fc010` |
| `fallback/BOOTIMG-previous.img` | 5378048 | `1852dfc995953f86ef5c47349515e8d6f06478daa6c1a89efe0f223501c226c9` |

The 74 production/M4 tests pass with no skips or failures. ARM rescue ABI,
production/qualification shell syntax, configured kernel/DT, existing reserved
memory and loader-envelope checks pass. The package rejects 12 isolated
identity/geometry/preservation/fallback mutations. These use output copies and
perform no hardware writes. The compiler emitted no warnings/errors.

Current rescue tools were rebuilt with the pinned production Bootlin ARMv7
hard-float toolchain. Y2ROOT/Y2DATA images were neither rebuilt nor written;
installed root/data references and schema 1 remain unchanged. The BOOTIMG package
contains no root/data payload. POWER-02 is the fallback and retains its known
charging defects; it is not a qualified recovery charger.

[Machine-readable identity and commands](result.json) ·
[Production tests](production-tests.log) ·
[Package validation](package-validation.log) ·
[Rejection cases](package-rejection-tests.log) ·
[Manual deployment and physical session](../../y2linux-m4-power-03-deployment.md).
