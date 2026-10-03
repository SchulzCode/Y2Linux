# Y2 CPU idle completion

Owner-authorized CPU completion pass,2026-10-03. Entry baseline Linux
`131ee4621cd583c955182f994adac5a594fb823f` / Reborn
`7f9df397ab3809d52e2f1073ca93a305246e0c3a` is verified against the installed
kernel/root/release and sealed package. Current boot
`cee326c1-a5b0-447a-8eb0-dc3f39f7e2c2`, taint0. No flash or push is permitted.
Preexisting owner documentation changes are preserved separately from this pass.

## Physically established foundation

[Raw qualification and results](Y2-CPU-IDLE-COMPLETION-PHYSICAL.md): C1 WORKING
on all four CPUs. C2 WORKING after natural parking:7182 additional entries,
50.763741s residency in60.404076s, exact clock restore failures0, eMMC/SD integrity
and timer continuity. Freeze both algorithms and the proven MMC transport/PM
patch; do not import the reverted later storage experiment. Three hotplug cycles
prove both physical power-status copies; all five guarded OPPs and real wired
playback pass. Historical full-system suspend failure remains a separate gate.

## Source changes and ownership

| Defect/prerequisite | Resulting source behavior | Evidence/guard |
| --- | --- | --- |
|Screen off left DRM scanout active|Reborn's existing DRM master synchronously disables its CRTC, retaining its current GBM/EGL buffer; wake restores the same owned mode before lighting the screen|Pending flip bounded; failed disable/wake rolls back screen model; first-frame/LK handoff covered; no synthetic suspend|
|Permanent shared display clock holds|MT6582 mutex now owns SMI_COMMON/SMI_LARB0/MUTEX in a balanced bulk lifecycle alongside its32k clock|Lima retains an independent SMI_COMMON reference; other SoCs unchanged; loader clocks protected until real owner handoff|
|Inherited unused MM clocks|Stopped CRTC/mutex owner retires BLS enables (Linux's Y2 route bypasses BLS), then acquires/releases only demonstrably quiet unsupported clocks through CCF|GREQ/DMA/CMDQ/engine checks, coupled operands snapshotted before any gate; unpowered windows never read; live/unknown engines retained, never reset|
|Inactive PERI/INFRA clocks|Expose exact MT6582 NFI/NLI,PWM,UART1–3,SPI0,L2C_SRAM/TRNG/CPUM gates to normal CCF unused-clock ownership|UART0 console/thermal/AUXADC/EFUSE retained; active UART/DMA/NAND/PWM/SPI state stays blocked; NAND register access requires both stock clocks enabled|
|Stopped audio clocks retained loader gates|Real AFE runtime owner can release AUDIO/AUDINTBUS/INFRA_AUDIO at reference zero|Initial unused sweep remains protected; existing AFE DMA/PCM lifecycle preserved|
|USB0 gate leaked despite suspended MUSB|USB glue claims a real CCF bus clock only after safe initialization; generic MUSB runtime PM saves endpoints, checks detached/session/FIFO/all8DMA/IRQ idle, gates, then restores clock before any MAC context access|Physical receipt12 proves current leak; ordinary role none/device; inherited gate protects failed probe; parent rate remains honestly unknown; no USB PLL/PHY retune|
|Wrong unconditional DISP-power rejection|C3 admits powered DISP only when the complete exact stock DISP0/1 masks are quiet|PERI/INFRA masks remain intact; MD/CONN/MFG/ISP/VDEC and physical secondary power bits remain strict|
|C3 runtime control|Default C3 remains off; positive dormant_budget permits that many actual cpu_suspend calls; -1 is deliberately qualified normal policy|Qualification budget requires owner-armed existing RGU10–30s; journal captures exact stage; bounded first failure cannot be repeatedly hammered|
|Opaque admission/context|Read-only preflight exposes every static prerequisite and real topology/OPP/clock/domain/PCM/vector/stash operand; status cpu decodes C2 and C3 owners|Dynamic deadline checked at actual admission and finisher; unknown/missing counters are not success|
|Timer handoff|Linux remains sole GPT4 broadcast programmer; require13MHz one-shot, IRQ armed, not pending, compare in future, at least26000ticks|64-bit ticks*1000/13 conversion, past/wrapped/near/repeating/incorrect-clock rejection; no second clockevent owner|
|Local timer context|Per-CPU save-valid state rejects incorrect CNTFRQ; restore disables CNTP, restores13MHz/future64-bit compare/control and verifies writable state|Unentered CPU_PM_ENTER_FAILED does not restore an uninitialized buffer; restore faults quarantine C3|
|CIRQ correctness|Exact155 interrupts64–218, five banks/tail0x07ffffff; clone mask/sensitivity/polarity readback before GIC masking; replay pending after Linux GIC restore; verify masks and disable|Exact MT6582 offsets and enable/edge-only ordering; no newer-SoC FLUSH layout; clone/restore faults quarantine C3|
|CPU/cache/context|Linux cpu_suspend/cpu_resume owns registers, CP15/MMU/idmap/VFP/GIC; validate its actual CPU0 stash/physical buffer, resume vector and PCM storage; retain/restore CA7_CACHE_CONFIG bit4 and MCU_BIU with readback|A7 integrated coherency, existing stock-backed Linux hotplug; no A9 SCU transplant or arbitrary power writes|
|Runtime PCM/restore|Keep independently stock-matched480-word DPIDLE program, retained INFRA/DDRPHY; verify arm operands and restore normal28-word program after entry/abort|No invented instructions or system-suspend PCM substitution; real reset return and fully successful restore counted separately|
|Workload guard|IRQ-safe aggregate of existing successful non-Idle leases protects deep entry|Expiry/renewal/failure/release ownership tested; no mutex taken inside idle; original QoS/thermal/parking policy preserved|

C1/C2 registration and fallback, coordinator30-second quiet policy, one-at-a-time
CPU3→2→1 parking and1→2→3 owned restore, hysteresis/pressure hold, schedutil,
thermal authority and598/747.5/1040/1196/1300MHz remain intact. The C3 entry
frequency rule is exactly598000 or747500kHz, including rejection of unknown0;
it does not reduce active performance. No root/data reset, partition/memory
layout change, /dev/mem, userspace MMIO, firmware invention or clock-bit masking.

## Exact source ledger and confidence

The retained Y2 PCM, CIRQ/context/SPM disassembly and prior source ledger are
reused rather than repeatedly recapturing unchanged ROM/recovery provenance.
[CPU Final source ledger](Y2-CPU-FINAL-SOURCES.md) names retained function/offset
and independent binary matching. Additional narrow downloads and SHA256 receipts
are in `out/cpu-idle-completion-research/manifest.json`.

- [Google MT6582 d53dd75c BSP](https://android.googlesource.com/kernel/mediatek/+/d53dd75c3ff77cac3f5be58fddfe660e94f94d64/arch/arm/mach-mt6582/): `mt_idle.c` full clock masks/minimum26000ticks, `mt_spm_sleep.c` runtime DPIDLE480 words (distinct from SODI/system PCM), `mt_dormant.c` and `cpu_dormant.S` CA7/BIU/L2 context, `mt_cirq.c` exact bank map and clone/replay. Direct exact-SoC evidence, independently retained-Y2 matched.
- [Google MT6582 3be93a68 BSP](https://android.googlesource.com/kernel/mediatek/+/3be93a68c209393cfe24a842e2d5896a17ea37dc/arch/arm/mach-mt6582/): independent version cross-check; no adjacent-SoC assumptions imported.
- [MT6582 clock manager](https://android.googlesource.com/kernel/mediatek/+/d53dd75c3ff77cac3f5be58fddfe660e94f94d64/arch/arm/mach-mt6582/mt_clkmgr.c): named PERI/INFRA/DISP gates and `cg_bootup_pdn`0xb0e0/0x2fef7fd. The bring-up shutdown routine is evidence that these are clock gates used while the CPU executes; applying it to unconsumed Linux idle/test clocks is an explicit inference, not evidence of arbitrary L2 power control. CA7 SRAM-retention configuration is separate and unchanged.
- [MT6582 USB PHY](https://android.googlesource.com/kernel/mediatek/+/d53dd75c3ff77cac3f5be58fddfe660e94f94d64/drivers/misc/mediatek/usb20/mt6582/usb20_phy.c): `usb_enable_clock` owns MT_CG_PERI_USB0 through balanced clock-manager operations; physical supported-role receipt corroborates the missing Linux owner.
- [MT6582 display register/engine sources](https://android.googlesource.com/kernel/mediatek/+/d53dd75c3ff77cac3f5be58fddfe660e94f94d64/drivers/misc/mediatek/dispsys/mt6582/): `ddp_cmdq.c`, `ddp_cmdq_debug.c`, `ddp_reg.h`, `ddp_bls.c`, `dpi_reg.h`, WDMA/RSZ/TDSHP sources. WROT ROT_EN+0x7c is explicitly named by debug code; reset status+0x14 alone is insufficient. RDMA idle monitor+0x408 mask0x7ff00=0x100 is used before clock disable. CMDQ has seven thread enables; DPI EN/status offsets0/0x40, LARB GREQ0x450, BLS bits0/16. These are exact MT6582 operands; unsupported activity always retains clocks.
- [BQ/ubports MT6582 NAND](https://github.com/ubports/kernel_krillin/blob/874057f3c28735d606385361fb9a4cd4545ceba6/mediatek/platform/mt6582/kernel/drivers/nand/mtk_nand.c): `nand_enable_clock`/`nand_disable_clock` owns both NFI and NLI; MT6582-path `mt6575_nand.h` defines MASTERSTA+0x210, CON+8, STA+0x60, FIFO+0x64. Legacy filename does not change actual SoC relevance.
- [MT6582 PWM](https://android.googlesource.com/kernel/mediatek/+/d53dd75c3ff77cac3f5be58fddfe660e94f94d64/drivers/misc/mediatek/pwm/mt6582/mt_pwm_hal.c) and [SPI](https://android.googlesource.com/kernel/mediatek/+/d53dd75c3ff77cac3f5be58fddfe660e94f94d64/drivers/misc/mediatek/spi/mt6582/spi.c): exact activity operands. SPI's real CMD/STATUS1 offsets0x18/0x20; any unknown nonzero state is busy.
- [Cortex-A7 TRM](https://documentation-service.arm.com/static/5f042da1dbdee951c1cd8c11): dormant/reset/coherency/cache constraints, with Linux's own A7 implementation supplying the port. [Linux v6.18 ARM suspend](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/arch/arm/kernel/suspend.c?h=v6.18), `sleep.S` and `proc-v7.S` are the locked local code; generic save block is36+12bytes, actual stash/physical-idmap and reset-return ownership are used directly.

## Automated next physical qualification

Run the single host harness after owner flash. Default displays a plan without
contacting hardware. `--run` verifies exact manifest and running Reborn identity,
then launches one durable private device job. A mutation is launched exactly
once even if SSH disappears; only read-only observations retry over independently
pinned Wi-Fi. Sparse observers preserve idle windows. If both transports are
intentionally off, the device job continues and restores supported USB role and
radios in finally. An RGU reset is a failed trial, captured from retained stage,
not a successful same-boot wake; there is no automatic reflash/retry.

The harness covers GPT6/GPT4/PPI29/highres/NO_HZ receipts and per-core1ms wake,
three physical hotplug cycles, all five bounded OPPs, screen-on/off metrics,
natural quiet/ownership parking, C2 useful residency/restore, fsynced uncached
SHA256 on eMMC and whichever SD is actually inserted, all C3 prerequisites,
too-close deadlines, one checked C3 timer-wake followed by19 only after success,
context/timer/CIRQ/GPT wake checks, screen/workload wake, real silent playback,
USB bidirectional1MiB SHA256 transfers with independent Wi-Fi observation,
filesystem/kernel IRQ faults and thermal state. It restores owner policies;
`--enable-qualified-runtime` leaves C3 enabled through ordinary cpuidle policy
only after20 successful bounded trials and an additional runtime-residency window.

It does not automatically remove SD or create a radio peer, and does not claim
physical external input-edge latch qualification from a synthetic key injection.
All such unobserved scenarios stay explicit. No electrical battery improvement
is claimed. Full-system suspend is excluded from this CPU-only runner because
its historical resume defect is a separate owner scope and hardware gate.

## Software qualification and package

Fresh targeted native owner/fault tests pass; fresh Reborn workspace tests and
clippy pass. Production kernel/config/DT/modules/ABI, Buildroot/Reborn ARM,
QEMU/ELF/dependencies, complete regression suite and preserving-package receipts
will be appended after the clean, committed build. One coherent fresh candidate,
no reuse-userspace, retains only BOOTIMG and Y2ROOT payloads and unchanged Y2DATA.
The exact Hardware02 fallback remains required. C3 physical category at handoff:
**SOFTWARE_READY_NEEDS_NEW_FLASH**; entry/success/residency remain unobserved until
that candidate is installed and the harness runs.

Fresh ARM linking caught a64-bit diagnostic division using an unavailable
userspace runtime symbol; the kernel variant now uses div_u64 while native
fixtures retain identical13MHz arithmetic. This was corrected before a complete
build or candidate was packaged. Baseline receipt18 adds three bidirectional
1MiB USB SHA256 passes with an independently reachable Wi-Fi observer.

Final source fault review adds fail-closed partial-DPI handoff (no unclocked
partner register read), counted/quarantined CIRQ enable-readback failure, and
all-five-OPP qualification again after C3 before restoring schedutil for the
actual playback test. Native dropped-clone/enable writes and partial coupled
clock fixtures pass. These refinements precede final packaging/qualification.
