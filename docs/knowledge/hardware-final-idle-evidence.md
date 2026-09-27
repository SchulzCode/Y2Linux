# Hardware Final idle source boundary

The current `mt6582-wfi` driver registers one architectural WFI state. Local
PPI29 timers and high-resolution/tickless operation do not themselves authorize
bus clock changes, dormant CPU context loss or SPM runtime idle.

The pinned MediaTek source at commit
`d53dd75c3ff77cac3f5be58fddfe660e94f94d64` was reviewed alongside the retained
exact Y2 FM kernel. New source receipts and bounded function extracts are under
private `out/hardware-final/source-idle/`; the established stock kernel identity
is in `evidence/m4-power-source.json`.

| State | Exact source/board evidence | Current missing requirements |
| --- | --- | --- |
| WFI | Existing architectural entry retains CPU context and platform ownership | Physical residency, timer continuity, latency and power measurements |
| Slow idle (`slidle`) | Exact `slidle_handler` at `0xc003d0e8` calls bus DCM before DSB/WFI and reverses it afterwards. `bus_dcm_enable` at `0xc003ce44` writes TOPCKGEN offset4 to `0x8f`; disable at `0xc003ce64` clears bit7. Stock condition table at `0xc09b1588` is PERI mask `0x00f00800`, other nine groups 0 | Current CCF owner has no serialized DCM transaction or idle eligibility API. Establish inherited register state, source-backed non-sleeping MSDC/clock checks and hotplug exclusion before a new state can safely enter. Stock requires one online CPU and no active PTP work. Current four-core operation is ineligible. Measure entry/residency, wake latency, timer continuity, peripheral integrity and power benefit before enabling by default |
| Deep idle (`dpidle`) | Exact stock 480-word PCM matches the non-MT6333 BSP branch byte for byte (hash below). Runtime entry retains infrastructure/DDRPHY but changes system-clock/DRAM behavior and may enter CPU dormant mode | Requires one CPU, early-suspend voltage eligibility, all stock clock blockers, CIRQ pending-interrupt preservation, GPT4 deadline handoff, audio-bus clock ownership, bus DCM and dormant context integration. None is supplied merely by the existing system-suspend driver |
| SODI/multi-core idle | BSP includes separate display/video-mode and hotplug guards and per-CPU GPT handoff | Exact Y2 state/PCM reconciliation and display contract are incomplete. The vendor GPT assignment includes GPT1, already owned by Linux's timer driver; direct import would conflict |

Slow idle is the shallower candidate: unlike dpidle it needs no PCM, CIRQ,
CPU context loss or GPT deadline handoff. This distinction avoids overstating
its blockers. Its current blocker is integration and physical acceptance of
shared bus control, not proof that the silicon lacks the state. Neither state
is marked unsupported solely because this implementation is absent.

The exact stock deep-idle PCM is 1920 bytes with SHA-256
`b0f8d879456e72ae62daf09ba8359761aee12c3bcc687f47f870af402fce8f49`.
It matches the 480-word branch of `mt_spm_sleep.c`, with event-vector offsets
0/9/28/68 and session 2; the other branch has 484 words and does not match.
The implemented suspend PCM has 597 words and different event vectors. It must
not be reused as a runtime idle program.

Vendor `dpidle_can_enter` requires a local deadline of at least 26000 ticks
(2 ms at 13 MHz), one online CPU and the early-suspend voltage condition.
`clkmgr_idle_can_enter` first rejects active MSDC clocks, then tests all clock
groups against the state's masks. Before WFI, the runtime path enables bus DCM,
moves the audio internal bus to its 26 MHz source and arms GPT4 with the remaining
local-timer deadline. After wake it restores that deadline and bus sources.
The stock `spm_go_to_dpidle` masks/clones GIC state into CIRQ, enables CIRQ,
then flushes/disables it and restores masks on every exit. Current DTS has no
CIRQ node; a compiled generic irq-mtk-cirq object is not active registration or
a runtime-idle hook. Missing these steps can lose a timer deadline or IRQ.

GPT6 is explicitly 64-bit in the retained `mt_gpt.c` (`GPT_FEAT_64_BIT`, high
counter at offset 0x18). The ARM timer's 56-bit clocksource mask is therefore not
by itself a 32-bit-counter bug. The disabled vendor `CONFIG_SYSCNT_ASSIST`
diagnostic checks high-word synchronization after low-word wrap. A physical
continuity run must cross the approximately 330.38-second low-word rollover.
Current DT has `arm,no-tick-in-suspend`, so Linux does not mark this clocksource
suspend-nonstop; absence of `always-on` separately marks local clockevents
C3STOP. Runtime idle must preserve the free-running system counter or provide
an evidenced clocksource alternative. These properties do not prove wake.

No new idle state, DCM write or PCM was enabled by this source audit.
