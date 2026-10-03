# Y2 C3 MT6582 UART admission correction

## Scope and observed baseline — 2026-10-03

This pass addresses C3 UART admission only. Candidate02's physical C1/C2,
parking, timers, five OPPs, hotplug and media results remain authoritative in
[the physical qualification](Y2-CPU-IDLE-COMPLETION-PHYSICAL.md).
C2 produced 6,772 entries and 50.669913 seconds in 60.048084 seconds (84.38%),
with zero clock restore failures. C3 has **never been entered**; no dormant
hardware failure or hardware impossibility has been demonstrated.

Fresh read-only SSH admission is under
`out/cpu-c3-uart-physical/20261003T164546Z/`. Installed identity remains
Linux `db0234e7519c559031da6f427869ab683ffe0f3c`, Reborn
`b71b468860233faa0a42b8448ec5777fa952b8e3`, kernel
`6.18.0-y2linux-cpu-idle-02`, root `2025.02.18-platform-v1.19`, build
`Y2LINUX-CPU-IDLE-COMPLETION-02`, boot
`f155196b-64d4-45a4-88b3-27755a1a8926`. Checkout admission HEAD `96f8ca3`
is a later receipt, not the installed source. Prior removed probes explain
taint 4096; no probe or C3 trial was run in this pass. Owner documentation
edits are preserved separately. Normal restored radio/OPP policy adds legitimate
preflight reasons; the prior settled receipt, rather than that active-policy
snapshot, establishes UART1 as the sole remaining static blocker.

## Exact stock contract

All vendor references below are MT6582 at commit
`d53dd75c3ff77cac3f5be58fddfe660e94f94d64`; no later SoC layout is used.
Retained downloads and SHA256 receipts are in `out/cpu-c3-uart-research/` and
`out/cpu-idle-completion-research/manifest.json`.

| Question | Source-backed answer |
| --- | --- |
| What does R7 request? | `SPM_POWER_ON_VAL1` bit0 requests UART sleep/clock-off through SPM. It is not a PERI gate write. |
| Who ACKs? | Hardware exposes `SPM_PCM_REG13_DATA` bit20. Software polls it, never synthesizes or writes ACK. Public C source does not establish the electrical RTL aggregation. |
| Global or particular port? | There is one request/ACK pair with no port selector. Per-port participation is configured by the UART driver. Do not call the global ACK a UART1-specific signal. |
| Must UART clocks already be gated? | Default DPIDLE mask includes UART1 bit17, UART2/3 bits18/19 and excludes UART0 bit16. Driver-authorized PIO ports remove their own clock bit from that mask. An active eligible UART clock is therefore permitted before the handshake. |
| What is checked first? | `dpidle_can_enter` checks voltage/early-suspend policy, hotplug count, clock-manager masks and future timer. Clock manager also rejects live MSDC clocks independently. |
| How do owners interact? | UART startup opts non-DMA TX/RX into DPIDLE, enables UART_SLEEP_EN, then enables interrupts. DMA configurations stay blocked. SPM subsequently requires global ACK before PCM power/run/WFI. |
| Is our rule invented? | The old raw PERI mask is copied from stock, with extra MSDC guards. The defect is omitting stock's conditional driver integration, not the existence of bit17 in the default mask. |
| Is UART1 the Y2 console/boot logger? | Linux and retained stock kernel console are hardware UART0 at 0x11002000. Vendor one-based UART1_BASE means that same UART0. Retained LK selects hardware UART0 or UART3 conditionally; it does not prove hardware UART1 at 0x11003000 is used. Live loader branch/pin routing remain unproved. |

Primary source anchors:

- [mt_idle.c](https://android.googlesource.com/kernel/mediatek/+/d53dd75c3ff77cac3f5be58fddfe660e94f94d64/arch/arm/mach-mt6582/mt_idle.c), lines329–425:
  default PERI `0x02fe87fd`, conditional mask helpers and admission.
- [mt_clkmgr.c](https://android.googlesource.com/kernel/mediatek/+/d53dd75c3ff77cac3f5be58fddfe660e94f94d64/arch/arm/mach-mt6582/mt_clkmgr.c), lines3319–3345:
  live MSDC guard and group-state intersection with current condition masks.
- [UART startup](https://android.googlesource.com/kernel/mediatek/+/d53dd75c3ff77cac3f5be58fddfe660e94f94d64/drivers/misc/mediatek/uart/uart.c), lines1545–1558:
  non-DMA mask admission and sleep enable.
- [MT6582 UART implementation](https://android.googlesource.com/kernel/mediatek/+/d53dd75c3ff77cac3f5be58fddfe660e94f94d64/drivers/misc/mediatek/uart/mt6582/platform_uart.c),
  lines1737–1741 and2135–2144: sleep enable and clock-ID policy hooks;
  [register header](https://android.googlesource.com/kernel/mediatek/+/d53dd75c3ff77cac3f5be58fddfe660e94f94d64/drivers/misc/mediatek/uart/mt6582/platform_uart.h)
  defines UART_SLEEP_EN at +0x48.
- [mt_spm_sleep.c](https://android.googlesource.com/kernel/mediatek/+/d53dd75c3ff77cac3f5be58fddfe660e94f94d64/arch/arm/mach-mt6582/mt_spm_sleep.c),
  lines602–622,866 and1176–1312: request, ten10us waits, timeout restores
  prior POWER_ON_VAL1 and returns UART_BUSY; both DPIDLE variants request
  after PCM fetch and before register/power/run/WFI; wake clears request.
- [Y2 retained binary analysis](../knowledge/observation-path.md): FM UART
  settings at `0xc04a736c`, console switch at `0xc04a9f3c`, LK selection at
  `0x81e00de8` / `0x81e00fc8`. Input hashes, offsets and confidence are retained
  there; no broad new reverse engineering or loader change is needed.

## Implemented admission and ownership

The old path already contained a bounded SPM handshake, but preflight never
allowed retained UART1 to reach it. The corrected path is:

`cpuidle -> static prerequisites -> existing clock owner -> CIRQ -> Linux
CPU/cluster PM -> watchdog -> PCM reset/fetch -> UART request/ACK -> unchanged
480-word DPIDLE power/run -> cpu_suspend/cpu_resume -> checked unwind`.

`uart-idle-policy.h` permits conditional UART1 deferral only with a normal
LCR register bank, IER0, no DMA enable/unknown bits and a known sleep-enable
value. It uses no LSR assumption: LSR0 neither proves active transmission nor
prevents hardware from answering the sleep request. All other clock blockers
remain effective, including MMC/APDMA/I2C/SPI. The existing clock lock remains
held throughout entry and unwind. CPU0-only/IRQ-disabled entry excludes the
existing process-context unused-clock worker. UART1 has no DT serial consumer.

The clock owner temporarily enables UART1's source-backed UART_SLEEP_EN,
verifies readback, and restores its exact previous value on every unwind.
LCR/IER/DMA and the PERI gate must remain unchanged. This is bounded sleep
adoption, not proof that an inherited divisor or inactive IRQ permits shutting
the UART down. No clock gate, reset, FIFO, data, baud or pinmux write is added.
The existing stricter late-unused UART gate guard remains unchanged.

Mainline 6.18's 8250 MTK driver lacks this MT6582 sleep-enable operation.
Patch0061 supplies it through UART0's normal driver only for exact
`mediatek,mt6582-uart` PIO ports. DMA ports and other SoCs do not opt in.
Failed sleep-enable readback leaves C3 blocked; console probe remains usable.

`spm-uart-policy.h` records real request/readbacks, polls through100us and
restores exact prior POWER_ON_VAL1 on refusal. Existing owned requests reject
without another write. No ACK means no DPIDLE PCM run and no cpu_suspend call.
A qualified refusal consumes the positive budget once. Restore faults latch
the existing broken-path protection. UART request/ACK use existing retained
SRAM stage markers; watchdog is active before the handshake, with no journal
layout or memory expansion.

SPM state and `y2-platform status cpu` expose request/ACK, attempts/successes/
timeouts/restore failures/result/rejection, POWER_ON_VAL1 before/request/after,
R13 before/ACK/live bits, conditional ownership and UART1 gate before/after.
Static readiness explicitly defers dynamic ACK to the real entry transaction.

## Validation and next physical boundary

Fresh focused locked-host suite: **100 tests pass**. Actual C functions execute
under UBSAN with injected ACK boundaries, absent ACK, dropped request/clear/
sleep writes, bank/IRQ/DMA/unknown owners, unchanged gate and all unrelated
blocker bits. Tests distinguish480-word DPIDLE from28-word normal cleanup PCM.
Existing CPU/context/CIRQ/timer/deadline/hotplug/OPP/fallback tests are retained.
Diagnostics and actual device harness failure/quarantine are tested.

Candidate identity: `Y2LINUX-CPU-C3-UART-01`, kernel
`6.18.0-y2linux-cpu-c3-uart-01`, root `2025.02.18-platform-v1.20`, release
`1.0.0-cpu-c3-uart-candidate.1`. API1, partition/data schemas and Reborn source
remain unchanged. Fresh kernel, Buildroot/Reborn ARM and preserving package
validation receipts will be attached at handoff; no prior binaries are reused.

The single intended candidate is `out/y2linux-cpu-c3-uart-candidate/`, only
BOOTIMG and Y2ROOT, with previous accepted images as optional fallback.
Y2DATA and protected partitions are preserved. No flash or push is performed.
C3 stays default disabled/budget0. Its handoff state is
**SOFTWARE_READY_NEEDS_NEW_FLASH**, not physically working.

After owner installation, use the packaged SSH harness without probes. It
checks exact installed identity, C1/C2/timers/hotplug/storage/five OPPs, then
waits for natural screen-off/radios-off/USB-detached CPU0 at <=747.5MHz.
The first trial has budget1, retained evidence,10s RGU and25ms timer wake.
Require new UART request/ACK, attempts/entries/resumes/successes +1, positive
residency, same boot, clean context/timer/CIRQ/gate/storage restores and a
responsive system. Only then allow20 additional bounded trials at varied
deadlines. A failure disables C3 and stops repetition; the precise stage is
captured. No runtime policy promotion or battery-life claim precedes evidence.
