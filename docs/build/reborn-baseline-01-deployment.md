# REBORN-BASELINE-01 root-only deployment candidate

Native Rust Reborn implementation is ready for owner installation. It is not
physically accepted. The [boundary audit](../planning/roadmap-gap-audit.md) records
the new owner authorization while preserving GPU #34 and M5 #31 physical gates.

Built source `77e673232a586e1f027538861bcbbec3466b36fd` in Y2Reborn;
integration `08b0e7d17c453037edf64f58a1798ff4457ca0ce` in Y2Linux.
[Evidence](evidence/reborn-baseline-01/README.md) records host tests, cross/runtime
checks, offline image build, provenance and preservation validation.

Package: `/home/luca/Dokumente/Code/Y2Linux/out/REBORN-BASELINE-01`.

| Payload | SHA256 |
| --- | --- |
| `Y2ROOT.img` | `c353152dea44a554486e024a4d8a8105fbbdfd1310db5749b28d7e5b8e5c8012` |
| `fallback/Y2ROOT.img` (GPU-02) | `1dec3b9c462587d804a022ef97e45222329065d263f53c2159ce00f728db145f` |
| Required installed GPU-02 BOOTIMG; unchanged | `2e5f7e785e80dfc646e57d0ccfadff683d54a2c5303671c819ea4e42863089a9` |

Both roots are 512 MiB images. Reborn adds 7,331,840 bytes of allocated ext4 usage.
No Y2DATA image, formatting or protected-storage change. No kernel/DT change.

Follow package `INSTALL.md` or the full 34-item handoff at
`/home/luca/Dokumente/Code/Y2Reborn/docs/validation/REBORN-BASELINE-01.md`.
Verify `sha256sum -c SHA256SUMS`. The established SPFT v5.2032 Download Only
workflow must use **MT6582_reborn_root_only_scatter.txt**, with **only ANDROID**
selected. BOOTIMG, USRDATA and all other rows are unchecked/NONE. Fallback uses
its own root-only scatter and retained GPU-02 root with the current GPU-02 kernel.

After installation, first collect `rebornctl status --json`, `health --json`,
`metrics --json`, `test baseline --json` over existing pinned owner SSH. Use
Y2Reborn `tools/qualification/reborn-baseline.py` for artifact retention and
explicit test classification before requesting physical interaction.

**STOP for owner installation. REBORN BASELINE 01 has not passed physical acceptance.**
