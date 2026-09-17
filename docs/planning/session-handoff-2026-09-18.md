# GPU physical qualification — 2026-09-18

The owner explicitly authorizes the production Linux 6.18 / Lima / Mesa GPU
platform and one integrated **Y2LINUX-GPU-01** candidate. Reborn and M6/OTA
implementation remain out of scope. Only the owner flashes the Y2. GPU-01 was manually deployed during the session;
GPU-02 is built and validated at the next manual deployment boundary. Preserve Y2DATA, all identities/preferences/bonds and RAM
exclusions. This supersedes the older handoff's GPU deferral.

Entry is clean `c70ff07`. Strict SSH using the retained private host-key pin
confirms `6.18.0-y2linux-m5-connectivity-10`, MemTotal 952480 KiB, existing
CCF/SPM/PWRAP owners and the unchanged memory map. SPM reports no error;
MFG status bit 4 is off in both `0x3f4c` status words. CPU/PMIC temperatures
are 44.200/44.543 C at inspection; these are not GPU temperatures.

[CONNECTIVITY-10 physical evidence](../hardware-evidence/2026-09-17-m5-connectivity10/README.md)
establishes calibration, controller `6582/8a01/8a00`, BTIF/AP_DMA, wlan0/cfg80211,
scans (23/12 results), radio restart and simultaneous Wi-Fi/BlueZ power-on with
zero errors/recoveries. These are settled foundations. M5 remains open for
connection/audio/reconnect/sustained coexistence qualification. The owner elects
to keep those tests pending during this session; they do not block GPU work.
M4 POWER-03 remains owner-accepted. Do not repeat M1–M5 discovery or M5 entry audit.

GPU-01 now physically proves Mali-400 MP2 r1p1, GP `0x0b070101`, PP
`0x0cd070101`, using the established 0x13010000 and IRQ202–207 contract.

**GPU-01 is now built and host-validated**, source
`7c43557a38fac6bbdb0ab5628cffe131ebda6360`. Existing CCF/SPM owners provide MFG
clock/domain/reset sequencing and runtime PM; Linux 6.18 Lima, Mesa 24.0.9
Lima/kmsro, libdrm 2.4.124, GBM/EGL/GLES and the bounded qualification utility
are integrated with the existing KMS display. RAM exclusions remain unchanged.

Read-only live clock and retained-LK-bin evidence establishes inherited MMPLL
500.5 MHz and devinfo[3] bit 19 = 0, matching this unit's stock GPU branch. No
GPU voltage or PLL retuning occurs. The observation modules are gone, but their
taint 4096 persists in this inspection boot; GPU-01 must start with taint 0.
[Physical contract evidence](../hardware-evidence/2026-09-18-gpu-contract/README.md).

All 92 regression tests, nine package rejection cases, emitted DT schema,
ARM utility ABI and exact package/root checks pass. The local package is
`out/y2linux-gpu-01`, with CONNECTIVITY-10 BOOTIMG + CONNECTIVITY-07 Y2ROOT
fallback copies. [The full 26-item receipt](../build/y2linux-gpu-01-deployment.md)
contains paths/sizes/hashes, manual steps and the coherent qualification sequence.

**GPU-01 rendering and runtime PM pass; deep suspend fails qualification.**
[Full evidence and failure localization](../hardware-evidence/2026-09-18-gpu01/README.md).
EGL 1.4, GLES 2.0 Mesa 24.0.9, Mali400, correct owner-visible output, 30.00 FPS,
3.36% CPU, 13 MiB RSS, CPU maximum 49.6°C / PMIC 48.637°C. Quiet wired audio and
radio scan/concurrent-power regression pass with zero errors/recoveries.
Static EGL context idles with MFG off and redraws on demand. Clean/TERM/KILL
application recovery passes. Strict input/render overlap needs a final check.

Installed y2-suspend incorrectly rejects `functions=0x0`; corrected only in
source and a temporary /run test copy. Actual sleep does not return the same
session: boot changes from 910f0f1b-4a58-4e6c-9c45-c44ca6f88f70 to
 dc6d812e-78ae-4c21-ba23-55b904d26275 after loss of SSH/dark display and owner's
brief Power/cable recovery. No specific reset cause can be proved from lost
tmpfs logs. A subsequent bounded pm_test=core trace proves CPU3 ACK timeout:
physical bit9 clears, old code waits bit13. Own stock/GPL confirm masks
0x800/0x400/0x200, combined0xe00. Targeted source fixes update CPU power ACK,
boot already-powered test, deep-entry guard and all-off matcher. Regressions
replay old failures and pass after correction; no other hardware policy changes.

**Do not attempt more hotplug/suspend on the present faulted boot.** It retains
CPUs 0–2, SPM broken=1, taint 0; SSH, root/data, charge watchdog and radios work.
Temporary test state is restored (pm_test none, charge auto, no RTC alarm,
radios/preferences off). The assistant has not flashed or replaced installed
production code. GPU-02 is built from source/evidence commit `7e318afbffe640c6bf9da458f61ac2108f7d5bde`;
94 tests and nine package rejection checks pass. [Full deployment receipt](../build/y2linux-gpu-02-deployment.md)
contains exact hashes, unchanged fallback pair and the targeted next procedure.
STOP for owner manual deployment. GPU-01 artifacts remain immutable.

GPU #34 stays OPEN; NOT READY TO BEGIN REBORN. After GPU-02 installation, run
only the targeted CPU/suspend/resume/display/input and post-resume regressions.
Do not repeat discovery or the entry audit. Reborn/M6/OTA remain excluded; M5
connection/audio tests remain pending by owner choice.
