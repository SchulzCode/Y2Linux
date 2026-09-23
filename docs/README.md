# Y2Linux documentation

Start with [current platform state](CURRENT_PLATFORM_STATE.md). The Platform v1
software candidate has separate implementation, host, ARM and image evidence;
physical and endurance qualification remain owner-controlled.

| Need | Authoritative documents |
| --- | --- |
| Current status and limitations | [Platform state](CURRENT_PLATFORM_STATE.md), [capability report](validation/PLATFORM-V1-CAPABILITY-REPORT.md) |
| Application contracts | [Platform API v1](architecture/platform-api-v1.md), [subsystem contracts](architecture/) |
| Candidate identity and validation | [Completion ledger](validation/PLATFORM-V1-COMPLETION.md) |
| Owner qualification | [Sessions A–G](validation/PLATFORM-V1-OWNER-QUALIFICATION.md), [hardware gates](knowledge/platform-v1-hardware-gates.md) |
| Rebuilding the source pair | [Reconstruction and release boundary](build/platform-v1-reconstruction.md) |
| Scope and milestone decisions | [Platform v1 roadmap](planning/platform-v1-roadmap.md), [standing audit](planning/roadmap-gap-audit.md#standing-milestone-boundary-rule) |

Technical knowledge and provenance remain in [knowledge/](knowledge/).
[Hardware evidence](hardware-evidence/), [build receipts](build/evidence/),
[early build results](build/results/) and [validation records](validation/)
retain observations, failures, identities, hashes and their original limits.
Dated milestone and issue documents preserve decisions at that time; their
"current", "next" and pending-deployment statements are not today's work queue.
Later corrections supersede findings only within their recorded scope.

The redundant September 10–18 session handoffs and rolling foundation task list
were removed during documentation cleanup. Their substantive results remain here:

| Historical topic | Retained knowledge and evidence |
| --- | --- |
| Initial five foundation tasks | [M1 research queue and original issue specifications](planning/M1-first-boot.md#historical-foundation-research-queue), [DEV-02 qualification](knowledge/y2linux-dev02-live-qualification.md) |
| September 10 audio handoff | [AUDIO-02 result](knowledge/m3-audio-01-live-result.md), [qualification procedure](build/y2linux-m3-audio-02-deployment.md), [reviewed issue snapshot](planning/session-issues-2026-09-10.json) |
| September 13 storage corrections | [Stock eMMC address correction](knowledge/storage06-addressing-correction.md), [Storage06 deployment and owner SSH result](build/y2linux-production-v1-r6-deployment.md) |
| September 15 power/M5 transition | [Power failure analysis and corrections](knowledge/m4-end-user-power.md), [owner-accepted M5 baseline](build/y2linux-m5-connectivity-01-deployment.md) |
| September 16–17 radio iterations | [CONNECTIVITY-04 failure/correction](knowledge/m5-connectivity04-corrections.md), [earlier matched fallback pairs](build/evidence/y2linux-m5-connectivity-03/README.md), [CONNECTIVITY-10 result](hardware-evidence/2026-09-17-m5-connectivity10/README.md) |
| September 18 GPU/suspend handoff | [GPU contract](knowledge/gpu-platform.md), [physical result and resume failure](hardware-evidence/2026-09-18-gpu01/README.md), [GPU-02 correction receipt](build/y2linux-gpu-02-deployment.md) |

The full superseded handoff wording remains in
[Git history before cleanup](https://github.com/SchulzCode/Y2Linux/tree/89230dd3add8f1fbbe7511d392f768a5db3305c0/docs/planning).
The [September 23 entry review](CURRENT_PLATFORM_STATE.entry-2026-09-23.md) and
[evidence ledger](planning/platform-review-evidence-2026-09-23.md) remain because
they record the starting assessment and distinguish source fixes from hardware
proof. Cleanup does not promote any capability or change the candidate artifacts.
