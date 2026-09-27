# Y2Linux documentation

Start with [current platform state](CURRENT_PLATFORM_STATE.md). Latest physical
authority is [CPU Final Fix01 qualification](validation/Y2-CPU-FINAL-FIX01-PHYSICAL-QUALIFICATION.md),
not the older Platform v1 handoff. Fix01 fixes timers and real workload hints,
but normal-platform acceptance FAILS. Source publication on main is separate.

| Need | Start here |
| --- | --- |
| Latest facts, limits and next batch | [Current state](CURRENT_PLATFORM_STATE.md) |
| What happened on the device | [Fix01 physical result](validation/Y2-CPU-FINAL-FIX01-PHYSICAL-QUALIFICATION.md) |
| Current observed capabilities | [Capability ledger](validation/Y2-HARDWARE-FINAL-CAPABILITIES.md) |
| Exact candidate/fallback identities | [Candidate index](knowledge/candidate-index.md) |
| CPU source/software provenance | [Fix01 implementation](validation/Y2-CPU-FINAL-FIX01.md), [sources](validation/Y2-CPU-FINAL-FIX01-SOURCES.md) |
| API and subsystem ownership | [Platform API](architecture/platform-api-v1.md), [contract catalog](DOCUMENTATION_CATALOG.md#architecture) |
| Power/hardware acceptance gates | [Hardware gates](knowledge/platform-v1-hardware-gates.md) |
| Next scope and decisions | [Roadmap](planning/platform-v1-roadmap.md), [standing audit](planning/roadmap-gap-audit.md#standing-milestone-boundary-rule) |
| Rebuild a recorded source pair | [Reconstruction](build/platform-v1-reconstruction.md) |
| Application state | [Reborn current state](../../Y2Reborn/docs/CURRENT_REBORN_STATE.md) |
| Every retained document | [Complete catalog](DOCUMENTATION_CATALOG.md) |
| How this knowledge base stays truthful | [Knowledge rules](KNOWLEDGE_BASE.md) |

## Reading older material

Historical validation, deployment, source research and hardware captures are
retained at their original paths. Their "current", "next" and authorization
statements apply to the recorded session. They do not override the current
state or authorize another flash/test. The
[catalog](DOCUMENTATION_CATALOG.md) labels records by scope; pages outside raw
evidence carry corresponding notices. Original captures/checksums are unchanged.

The prior [CPU Final physical run](validation/Y2-CPU-FINAL-PHYSICAL-QUALIFICATION.md)
is Fix01's before-state. Hardware Final and Platform v1 completion describe
earlier software boundaries, not the currently tested pair. The
[September 23 entry review](CURRENT_PLATFORM_STATE.entry-2026-09-23.md),
[old issue specifications](planning/issues/),
[build receipts](build/evidence/) and [hardware evidence](hardware-evidence/)
preserve provenance and narrow successes/failures. Original boot, stock ROM,
partition and source research remains under [knowledge/](knowledge/).

Earlier removed rolling handoffs remain available in
[Git history](https://github.com/SchulzCode/Y2Linux/tree/89230dd3add8f1fbbe7511d392f768a5db3305c0/docs/planning).
This cleanup changes navigation and summaries, not candidate bytes, acceptance
flags, reserved memory, firmware, keys or owner data.
