# Knowledge-base authority and maintenance

<!-- knowledge-base-scope: maintained; baseline02-sync 2026-10-03 -->

## Current authority map — Baseline02, 2026-10-03

The [Baseline02 seal](validation/Y2-BASELINE-02.md) identifies compiled Linux
`8584ccd85f052f351fe51650b2348c83ca894ed4` and Reborn `b71b468860233faa0a42b8448ec5777fa952b8e3`.
Later documentation HEADs and published `main` are separate identities.
The [UART01 physical receipt](validation/Y2-CPU-C3-UART-PHYSICAL.md) proves
bounded C1/C2/C3 behavior on Linux `e9e8d63f9c94232c2b6627881e0967583e202dac`;
it does not qualify Baseline02's new automatic cold boot. Baseline02 is sealed,
software-tested and PHYSICAL_NOT_RUN. Current summaries, feature JSON, release
plan, contracts and navigation follow that distinction; historical receipts and
raw captures retain their original claims.

This synchronization uses retained evidence only. Private `out/` paths are
literal local references, not public hyperlinks. Source delivery now includes
paired source tar archives and locked upstream archives; reconstruction and
legal collection do not establish byte-identical rebuild or distribution
permission. See [source delivery](release/Y2-COMMUNITY-SOURCE-DELIVERY.md).


This repository and adjacent Y2Reborn describe one product. Start with
[platform state](CURRENT_PLATFORM_STATE.md) and
[application state](../../Y2Reborn/docs/CURRENT_REBORN_STATE.md). The complete
[Linux catalog](DOCUMENTATION_CATALOG.md) and
[Reborn catalog](../../Y2Reborn/docs/DOCUMENTATION_CATALOG.md) cover retained pages.

## Authority order

1. Exact-image physical receipts for what actually happened on that device.
2. Current state/capability summaries derived from those receipts.
3. Current production source/configuration for implemented/enabled behavior.
4. Exact-pair build/software checks for their stated validation tier.
5. Historical source research, candidate records and proposals within their dates.

PASS applies to the stated bounded behavior. FAIL, PARTIAL and NOT_TESTED remain
distinct. No source test, enabled configuration, new boot or merge to main
certifies full same-boot restoration or production readiness. Report measured,
configured, estimated and unavailable values separately. Never turn invalid SRAM
record fields into real register readings or USB presence into active charging.

## Update procedure

After a new implementation or physical result, update both current-state pages,
affected maintained contracts/knowledge summaries, capability ledger and roadmap.
Link exact commits/kernel/root/boot/evidence and state the scope that supersedes
an earlier result. Keep earlier observations, failures and identities intact.
Before a milestone/hardware/memory/production boundary follow the
[standing audit](planning/roadmap-gap-audit.md#standing-milestone-boundary-rule).
Documentation maintenance itself performs no build, flash or new hardware test.

Use the catalogs to find every document. Maintained contracts describe source
ownership, historical notices bound old task/permission statements, and raw
hardware/build evidence is preserved without textual edits. Asset references
remain design material, not running-screen or hardware proof. Keep existing
paths stable; replace missing private-artifact hyperlinks with literal paths
and explain their local-only availability. Do not delete history to hide defects.

## Repository boundaries

Publish reviewed summaries and source. Keep `out/`, caches, generated candidates,
raw private captures, owner radio firmware, keys, bonds, calibration and NVRAM
local. Do not clean these as if they were disposable documentation clutter.
Preserve fallback images/checksums and reserved memory/loader/storage boundaries.
Do not prune other worktrees or change Git identity during knowledge cleanup.

`main` names published/integrated source; owner-local candidate branches may be
newer. Baseline02 compiled Linux `8584ccd8` / Reborn `b71b4688` identify its
images; later documentation revisions do not change those bytes.
Documentation HEADs can advance without changing an installed candidate. Static
catalogs should be refreshed when adding/retiring a page. Check local links and
heading anchors with `python3 tools/development/check-docs.py` from Y2Linux,
and run `git diff --check` for
documentation changes; do not rebuild unchanged platform binaries.
