# CPU Final Fix 01 implementation and candidate receipt

Owner-authorized correction of the five integration failures in the completed
[physical qualification](Y2-CPU-FINAL-PHYSICAL-QUALIFICATION.md). This pass changes
software, produces one preserving candidate and does not access or flash a Y2.
Starting Linux `aadcaa40a486cda555c859f131a2dcdb664b2233`; starting Reborn
`afcf9ffa1bc45073e97520d592c0284ce73fefcf`. Existing user documentation and UI
asset changes remain outside implementation commits and isolated build checkouts.

## Timer correction

The old combined predicate required GPT6 **and GPT4** control/clock/IRQ state to
be pristine. Exact retained Y2 preloader bytes at file offsets0xec0a..0xec18
write GPT4 control0, CLEAR2, clock0, then control0x31. Its delay code and retained
LK at0x81e08a1c use GPT4 count0x10008048. Requiring GPT4 control0 therefore
rejects this legitimate boot state before GPT6 starts. This is a binary-backed
root cause; the failed image did not log the actual predicate operand. The
physical register attempt printed addresses without values, so it is not claimed
as a retrospective live GPT4 readback.

GPT6 now has separate DT/resource, feature, reserved-bit, control readback,
GPT2 reference, GPT6 rate and CNTP counter-rate predicates with names and register snapshots.
A valid running13MHz GPT6 is adopted without clearing/stopping its counter;
known stopped/other valid control states are safely prepared. Reserved state
and real counter mismatch still fail. On failure modified state is restored.

Only the original Linux GPT resource/IRQ owner reclaims GPT4. It validates and
stops the inherited channel, verifies clock/control/compare, then stops/masks
GPT1. Failure preserves GPT1 broadcast. The parent handler checks the selected
channel's status. Normal Linux oneshot clockevents and tick broadcast remain.
PPI29 DT identity,13MHz frequency, per-CPU CNTFRQ readback and advancing counter
are required. CPUHP registration errors roll back through upstream cleanup and
are reported; deeper paths require successful event registration. Linux owns
highres/NO_HZ selection. No custom tick suppression is added; actual runtime
state comes from timer_list (NO_HZ mode2 means highres, not disabled).

## Reborn and worker leases

`writeln!` formatted directly into File and split a parser transaction into
`Interactive`, separator and duration writes. Each complete class/duration/newline
payload is now constructed first and passed to one write_all. Tests require
whole payloads for all eight classes and reject fragmentation.

Playback publishes before decoder/device startup and renews each second, with
heavy leases for configured crossfade/EQ. The actual scanner owns and renews a
separate scoped lease, including short scans; scan no longer replaces playback.
Artwork retains its scoped decode lease. Interaction belongs to Runtime and is
closed directly at ScreenSleep. Successful writes/clear reset the reopen backoff,
so waking the display within10seconds cannot suppress new interaction leases.
Independent descriptor lifetime tests keep playback open while interaction
closes. Platform transfer/throughput/export/purge workers own bounded renewing
NetworkTransfer/Maintenance descriptors. Applications never choose MHz.

## SLIDLE correction

| PERI bit | Clock/current owner | Expected idle state and remediation |
| --- | --- | --- |
| 11 | APDMA / I²C DMA | Off between transfers; balanced CCF disable now gates it |
| 12 | MSDC0 / eMMC | Off after host runtime autosuspend; inherited bypass removed |
| 13 | MSDC1 / SD | Off after host runtime autosuspend; inherited bypass removed |
| 14 | MSDC2 / no Y2 DT consumer | Stale loader gate may be swept only with SDC_STS and DMA_CFG idle |
| 21 | I²C0 / wheel | Off between transactions; inherited bypass removed |
| 22 | I²C1 / DAC | Off between control transactions; inherited bypass removed |
| 23 | I²C2 / no Y2 DT consumer | Stale loader gate may be swept only with START and APDMA EN idle |

Those are exactly the set bits of0xe07800. BTIF bit20 is retained in the
eligibility mask; it was absent in this observation. USB, AFE, CONSYS, display
and UART are not silently removed from an applicable mask/domain check. Clock
status remains authoritative. Busy unowned engines retain their inherited gate
and blocker; no DMA engine is reset to improve idle counters. Gate operations
now share the bus transition lock. Exact bus value is restored on both entry
failure and wake; readback failure disables SLIDLE and falls back to WFI.

All three pinned MT6582 trees require hotplug_cpu_count1 for stock SLIDLE.
The conservative coordinator waits for screen-off, no live non-Idle lease,
<=10% activity,598MHz and admitted local events. Following a60second restore
hold and30seconds quiet it parks one secondary at most every5seconds. Input,
frequency demand, display wake and workload acquisition restore only its own
parked cores; workload/display paths restore synchronously before acknowledgement.
Suspend restores them before freezing. Manual offlines are not claimed/restored.
No parking during legitimate playback/scan/transfer leases. Faults latch the
coordinator off. Counters and blocker bit/name/owner strings are exposed.

## PMIC/DVFS correction

Physical CPU Final saw VPROC_CON5/216=0, active software VOSEL/21e=48hex,
bin0 and1.15V. The old code demanded hardware VOSEL_ON mode(bit1) and rejected
this loader state. Retained stock PMIC_INIT_SETTING_V1 atc04b8528..c04b8538
sets216 bit1 during **stock kernel** initialization; its later cpufreq slots220
therefore do not prove that Linux's unchanged loader state uses220.

Both legitimate MT6323 modes are supported without changing mode. The canonical
SPM/PWRAP owner programs normal slots0/1/2 to the currently selected21e or220
bank, with stock selectors88/80/72. NI_VPROC_VOSEL/224 confirms active feedback
before admission and after every settled request. Whole-word, bin, arbitration,
slot/readback, normal PCM handshake and clock guards remain. Sleep slots5/6/7
use that same active bank and retain1.15V throughout. No unvalidated sleep voltage.

598/747.5/1040MHz retain1.15V;1196 uses1.20V and1300 uses1.25V. Increase is
voltage/readback/settle before clock; decrease is clock/readback before voltage.
Existing rollback, conservative fault ceiling and thermal QoS authority remain.
Diagnostics report selected/actual bank, selectors, stage, first error and
admission ceiling. Unsupported bins or failed feedback cannot admit high OPPs.

## Full suspend correction and receipts

Staged CPU restoration, USB quiescence/W1C handling and the complete-isolation
single CONSYS retry are preserved. No second reconnect owner is introduced.
Full sleep uses the exact Y2597-word suspend PCM; runtime dormant keeps its
separate480-word PCM and stays experimental/default off.

CPU0's physical Boot ROM vector now points to a stackless ARM entry which writes
an SRAM reset-dispatch stamp before branching to Linux cpu_resume_arm. Vector
address/enable and instruction visibility are checked at entry. Linux retains
ownership of CP15/MMU, VFP, GIC and architectural save/restore. MCU_BIU retention
restore remains. Full shutdown and dormant's L2 reset-invalidate suppression
remain distinct; no duplicate vendor context save is imported.

SPM admission verifies PCM pointer/length and final wake/timer/watchdog/power-IO
readbacks before kick. Wake mask includes stock KP and EINT; software wake request
is cleared without clearing a pending RTC/EINT. The MFD verifies armed RTC's
selected interrupt mask and unwinds failures. It snapshots RTC enable, PMIC wake
masks and non-clearing PMIC status, never consumes RTC_IRQ_STA. Power key remains
IRQ5; RTC isIRQ20; both use the established PMIC EINT25 route. Source-backed
CIRQ clone/mask and restored-GIC replay ordering is retained. Normal PCM is
reinstalled after resume and every abort, including early notifier refusal.

Two checksum/sequence SRAM slots commit their magic last; any interrupted write
leaves a previous valid stage. Exact stock mt_map_io and ram_console_early_init
prove physical0010dc00..0010f7ff as the retained-console allocation. Only260bytes
are claimed, with no DRAM carveout. Preloader's retained-console branch checks
magic43474244; the journal uses5932504d and does not enter that mutating branch.
The old record is exposed separately on the next boot. Warm-reset electrical
retention still needs device verification; cold power cannot preserve SRAM.
Durable atomic/fsynced receipts on Y2DATA cover safe pre/post boundaries. There
are no filesystem/PMIC-spare writes in the unsafe interval. Full entry refuses
if the SRAM diagnostic owner failed to initialize.

Markers cover request/sync/devices/secondaries/CIRQ/RTC/UART/PCM/context/SPM
entry/return/normal PCM/timer/CIRQ replay/CPU/device/radio/Reborn/complete. The
reset stamp distinguishes Boot ROM dispatch from context return. Snapshots include
wake reason/raw status, PCM pointer/length/control, mask/timers/watchdog,
CIRQ control, vector/enable, CPU power copies and CA7 cache configuration. Radio
restore failures remain errors; a fresh Reborn control response is required before
its ready marker. Diagnostics expose current, previous and durable receipts.

The charger -EBUSY guard remains. Linux6.18 restricted GFP only after successful
dpm_prepare but restored it on every dpm_resume_end. The failed-prepare path was
unbalanced. Restriction now occurs after prepare on both outcomes, preserving
prepare allocation behavior and balancing normal/abort unwind without WARN/taint.

## Qualification and software evidence

The owner-run [SSH harness](../../tools/development/qualify-cpu-fix01.py) defaults
to a plan. `--run` pins installed source identities to package manifest, captures
all independent checks, polls/reconnects automatically, and never treats a new
boot as resume. Battery sleep uses the owner's existing Wi-Fi SSH configuration;
automatic RTC failure requests one Power press only after timeout. Power wake
has a separate deliberate press and RTC recovery backstop. No flash/reboot code.

Current-source validation results, built identities, artifact hashes, preserving
allowlist and exact fallback are recorded in the final candidate manifest and
validation receipts. Software tests do not claim physical qualification.
