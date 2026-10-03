# Y2Linux documentation

Baseline02 is the newest sealed candidate. UART01 supplies the latest physical
CPU-idle evidence: C1/C2/C3 work, including 21 bounded C3 trials and 3,624 entries
in normal policy. Baseline02 adds automatic qualified C3 boot activation;
new-image hardware and cold-boot qualification remain PHYSICAL_NOT_RUN.

| Need | Start here |
| --- | --- |
| Current facts and limits | [Current platform state](CURRENT_PLATFORM_STATE.md) |
| Baseline02 versions, hashes, checks and owner handoff | [Baseline02 receipt](validation/Y2-BASELINE-02.md) |
| Authoritative CPU-idle hardware results | [UART01 qualification](validation/Y2-CPU-C3-UART-PHYSICAL.md) |
| Exact candidate and rollback pair | [Candidate index](knowledge/candidate-index.md) |
| CPU-idle implementation and progression | [Completion source](validation/Y2-CPU-IDLE-COMPLETION.md), [physical progression](validation/Y2-CPU-IDLE-COMPLETION-PHYSICAL.md), [UART fix](validation/Y2-CPU-C3-UART.md) |
| Current capability scope | [Capability ledger](validation/Y2-HARDWARE-FINAL-CAPABILITIES.md) |
| Full release gaps | [Feature audit](release/Y2-COMMUNITY-BETA-FEATURE-AUDIT.md), [beta plan](release/Y2-COMMUNITY-BETA-PLAN.md) |
| API and ownership | [Platform API](architecture/platform-api-v1.md), [power contract](architecture/platform-power-v1.md) |
| Next milestones | [Roadmap](planning/platform-v1-roadmap.md), [standing audit](planning/roadmap-gap-audit.md#standing-milestone-boundary-rule) |
| Rebuild / source delivery | [Reconstruction](build/platform-v1-reconstruction.md), [source delivery](release/Y2-COMMUNITY-SOURCE-DELIVERY.md) |
| Application documentation | [Reborn state](../../Y2Reborn/docs/CURRENT_REBORN_STATE.md) |
| Every retained page / authority rules | [Complete catalog](DOCUMENTATION_CATALOG.md), [knowledge rules](KNOWLEDGE_BASE.md) |

Historical reports remain at their original paths. Their “current image”, “next
step” and authorization statements apply to their dated sessions. A current
summary does not rewrite raw evidence or grant new hardware authorization.
Full system suspend, electrical battery measurement and other release gates
remain separate from the physically qualified runtime C3 architecture.


## Historical navigation snapshot

The following text retains earlier dated decisions and results. Its “current”,
“next” and candidate labels belong to those sessions; the Baseline02 summary
above takes precedence. Historical failures and seal-time NOT_RUN receipts
remain evidence for their exact images.

---

# Y2Linux documentation

Start with the [complete community beta audit](release/Y2-COMMUNITY-BETA-FEATURE-AUDIT.md) and [selective RC1 plan](release/Y2-COMMUNITY-BETA-PLAN.md). They reconcile current source, sealed images and physical evidence at the2026-10-02 snapshot. The latest broad hardware report is [CPU Final Fix02 qualification](validation/Y2-CPU-FINAL-FIX02-PHYSICAL-QUALIFICATION.md); newer storage observations are narrower and do not establish whole-system acceptance.

| Need | Start here |
| --- | --- |
| Latest facts, limits and next batch | [Current state](CURRENT_PLATFORM_STATE.md) |
| What happened on the device | [Fix02 physical result](validation/Y2-CPU-FINAL-FIX02-PHYSICAL-QUALIFICATION.md) |
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
