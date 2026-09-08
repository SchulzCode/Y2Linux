# Y2Linux

Canonical Linux/platform repository for the Innioasis Y2: hardware evidence,
reverse engineering, recovery, upstream Linux 6.18 research and later bring-up.
The native Rust/C application belongs to [Y2PlayerNative](https://github.com/SchulzCode/Y2PlayerNative).
The Android Y2Player is a behavioral and hardware-research reference, not a source-port base.

Current phase: **M0 — Evidence & Recovery Baseline. NOT PASSED.**

- [M0 research plan and results](docs/planning/M0-evidence-and-recovery.md)
- [Hardware evidence](docs/knowledge/y2-hardware.md)
- [Artifact provenance](docs/knowledge/evidence-index.md)
- [Boot chain](docs/knowledge/boot-chain.md) and [partition map](docs/knowledge/partition-map.md)
- [Recovery capabilities and gate](docs/knowledge/recovery.md)
- [Open unknowns and decisions](docs/knowledge/open-unknowns.md)
- [Canonical M0 milestone](https://github.com/SchulzCode/Y2Linux/milestone/1)

Research and architectural decisions precede implementation. Research checklists
are not implementation handoffs; only a fully researched, bounded intended change
may become `state:luna-ready`. Preserve the configured user's Git identity and
authenticated GitHub account; never alter identity or add model attribution.

Raw device captures, binaries, firmware and personalized data stay in ignored
`evidence-private/`, outside Git. Committed documents contain reviewed findings,
hashes and source locators. A local evidence copy is not an independent backup.
No experimental Linux boot or destructive device operation is authorized by the
current research results.
