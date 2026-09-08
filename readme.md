# Y2Linux

Canonical Linux/platform repository for the Innioasis Y2: hardware evidence,
reverse engineering, recovery, upstream Linux 6.18 research and later bring-up.
The native Rust/C application belongs to [Y2PlayerNative](https://github.com/SchulzCode/Y2PlayerNative).
The Android Y2Player is a behavioral and hardware-research reference, not a source-port base.

Current status: **M1 core achieved on the physical Y2: Linux 6.18, initramfs and native PID1 with increasing BEAT/uptime.** M0 residual questions and broader platform work remain separate.

- [Roadmap coverage, gaps and milestone audit rule](docs/planning/roadmap-gap-audit.md)

- [Actual runtime hardware result and evidence limits](docs/knowledge/m1-runtime-hardware-result.md)
- [Next five foundation research issues, planning only](docs/planning/next-five-platform-foundations.md)

- [Offline build result, hashes and exact D08 intervals](docs/build/first-boot-result.md)
- [Reproduce the offline build](docs/build/first-boot.md)
- [M1 checkpoints](docs/planning/M1-first-boot.md) and [hardware launch gates](docs/knowledge/first-boot-launch-gates.md)

- [M0 research plan and results](docs/planning/M0-evidence-and-recovery.md)
- [Hardware evidence](docs/knowledge/y2-hardware.md)
- [Artifact provenance](docs/knowledge/evidence-index.md)
- [Boot chain](docs/knowledge/boot-chain.md) and [partition map](docs/knowledge/partition-map.md)
- [Recovery capabilities and gate](docs/knowledge/recovery.md)
- [Open unknowns and decisions](docs/knowledge/open-unknowns.md)
- [Canonical M0 milestone](https://github.com/SchulzCode/Y2Linux/milestone/1)

Research and architectural decisions precede implementation. The owner authorized
direct M1 implementation by the lead with no delegation; work proceeds through
small researched issues. No task in this wave is `state:luna-ready`. Preserve the configured user's Git identity and
authenticated GitHub account; never alter identity or add model attribution.

Raw device captures, firmware and personalized data stay in ignored
`evidence-private/`; build caches/products use ignored `.cache/` and `out/`. Committed documents contain reviewed findings,
hashes and source locators. A local evidence copy is not an independent backup.
No experimental Linux boot or destructive device operation is authorized by the
current research results.
