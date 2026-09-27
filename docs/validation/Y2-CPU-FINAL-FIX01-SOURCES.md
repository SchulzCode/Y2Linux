# CPU Final Fix01 source and binary ledger

Retained sources are read without reacquiring ROM/recovery/calibration. New
bounded analysis receipts reside privately in `out/cpu-final-fix01/source/` and
are included in the personal candidate's source collection.

| Reference | Narrow use |
| --- | --- |
| [Sprout d53dd75c](https://android.googlesource.com/kernel/mediatek/+/d53dd75c3ff77cac3f5be58fddfe660e94f94d64/arch/arm/mach-mt6582/) | mt_gpt, generic timer, mt_idle single-CPU/clock eligibility, sleep/normal PCM, CIRQ and dormant ordering |
| [Independent MT6582 revision3be93a68](https://android.googlesource.com/kernel/mediatek/+/3be93a68c209393cfe24a842e2d5896a17ea37dc/arch/arm/mach-mt6582/) | Cross-check GPT6 ownership, SLIDLE masks, suspend vs dormant topology and wake sequencing |
| [Huawei3fec42dd](https://github.com/ferhung-mtk/android_kernel_huawei_h30t00/tree/3fec42ddadbefcf95c8fea4a10affb1c0abf5231/kernel-3.4/mediatek/platform/mt6582/kernel/core) | Independent clock group bit IDs, MT6323 VPROC_CON5/VOSEL/VOSEL_ON/NI feedback, stock OPP and SPM/PWRAP commands |
| Exact retained Y2 FM kernel SHA2567287acf1b397d243c927dde5c6ef9d5d871c9c158207cdd98e0e820923f4c197 | mt_gpt_init, generic_timer_setup, CPU shutdown/dormant, sleep PCM, PMIC initialization and SRAM map. Existing bounded extracts plus new mt_map_io/ram_console/CPU/reset extracts; addresses below |
| Exact Y2 preloader and LK | Raw bounded extracts and SHA256s in source/stock/bootloader-extracts.json. GPT4 initialization at0020f8f8 and retained-console guard at0020b9ce; LK GPT4 count reads at81e08a1c |
| [Linux6.18](https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/?h=v6.18) | Locked local source: arm_arch_timer CPUHP/CPU PM, clockevents/tick highres/NO_HZ, ARM suspend.c/sleep.S/proc-v7.S, device PM GFP unwind, RTC/MFD/EINT, MMC/I²C runtime PM. Overlay base/result hashes remain exact |
| [Cortex-A7 DDI0464F](https://documentation-service.arm.com/static/602cf701083323480d479d18) | Generic Timer/CP15 and physical timer PPI routing, reset/coherency constraints. No adjacent-SoC register constants imported |

The preloader writes GPT4 control0/CLEAR2/clock0/control31 at original file
0xec0a..0xec18. Literals at0xec38/0xec3c are10008040/10008044; extraction verifies
both and the31 write against raw bytes. LK reads10008048. This establishes the
legitimate inherited GPT4 condition independently of a missing live register
receipt. Linux must transfer this owner rather than require reset state.

MT6323 VPROC_CON5 bit1 selects VOSEL_ON/220 or software VOSEL/21e. NI_VPROC_VOSEL
is224. The exact stock PMIC_INIT_SETTING_V1 ARM call atc04b8538 writes216 field
(mask1,shift1,value1), explaining the stock kernel's later220 command slots.
Y2Linux's physical216=0 is the unchanged loader mode, not stock kernel init.
No software-mode bank switch is guessed from an adjacent SoC.

Exact mt_map_io(c095bf48) descriptor atc09875b8 maps f9000000 to physical
00100000 for64KiB. ram_console_early_init(c097287c) mapsf900dc00,size1c00.
Retained preloader compares0010dc00 magic43474244 before its console-field writes;
Fix01's distinct5932504d magic preserves the new journal across that branch.
No PMIC boot flag, RTC spare, loader or new DRAM allocation is used.

PERI bits11/12/13/14/21/22/23 are APDMA/MSDC0/MSDC1/MSDC2/I2C0/I2C1/I2C2
in the independent MT6582 clock header. Stock SLIDLE mask00f00800 remains,
with existing stricter MSDC7800 mask. GPIO/EINT, USB, UART and AFE masks are not
substituted from MT6589/6592. Unused MSDC2 and I²C2 busy tests use their own
exact MT6582 register windows and non-clearing status; active engines stay on.

Source inspections establish register/software contracts. Fresh software builds
and synthetic tests establish reproducibility and error handling. Only the next
owner-controlled physical receipts can establish actual timer/idle/DVFS/wake
behavior of this exact candidate.
