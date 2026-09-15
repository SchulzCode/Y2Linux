# Session handoff — 2026-09-15, M4-POWER-03

**POWER-03 built and host-validated. Stop for owner manual BOOTIMG deployment.**
M4 #30 remains OPEN / ACTIVE / PARTIAL. The latest owner request is focused M4
completion; **do not start M5**. Its completed entry audit is historical context,
not the next implementation task. No assistant flash or protected writes.

## Concrete candidate

Source commit: `16875c4acc813b54c781167883227cf289f4dfcd`.
Kernel: `6.18.0-y2linux-m4-power-03`.
Package: `/home/luca/Dokumente/Code/Y2Linux/out/y2linux-m4-power-03/`.

| Artifact | Bytes | SHA256 |
| --- | ---: | --- |
| `BOOTIMG.img` | 5408768 | `66b6ecd5ef54da6f3ea07be2c9deda284d0a7636e9c6da4c5d72d84eca5fc010` |
| `fallback/BOOTIMG-previous.img` — POWER-02 | 5378048 | `1852dfc995953f86ef5c47349515e8d6f06478daa6c1a89efe0f223501c226c9` |

[Exact deployment and one coherent physical session](../build/y2linux-m4-power-03-deployment.md)
and [build receipt](../build/evidence/y2linux-m4-power-03/README.md).
74 production/M4 tests, ARM rescue checks and 12 package rejection cases pass.
Y2ROOT/Y2DATA require no update or migration. POWER-02 fallback is bootable but
has the known charging failures; do not label it a low-battery recovery fix.

## What changed

- Stop at the first 4.175-V sample, confirm voltage-limited completion while
  inhibited, recharge below 4.110 V for 60 seconds. The 4.2-V fault guard remains.
- Program only the actual stock 4.3-V battery OVP selector; keep enable/detector.
  Save first-fault registers before inhibit overwrites them. Fault recovery
  requires real unplug, verified stop and safe inputs.
- MT6323 RTC uses the stock 1968 epoch for time/alarm and preserves spare bits.
  RTC-to-system time and standard `/proc/sys` boot identity are enabled.
- Read MT6582 thermal efuses as words; reject POWER-02's observed truncation.
- Kernel MUSB runtime reference spans attached sessions, with release on detach
  and resume before reconnect. POWER-02's userspace `on` pin is release-scoped.
- Preserve existing SPM, minimal offline display/charging and fixed-voltage OPP
  architecture. Qualification now checks real resume/residency counters plus
  boot/process/mount/data continuity. No lower active stock OPP voltage is known.

[Detailed implementation and safety distinctions](../knowledge/m4-end-user-power.md).
No physical POWER-03 behavior is yet qualified, including thermal accuracy,
charging gain/full/recharge, offline poweroff or deep suspend.

## Live baseline and access

Last inspected device is still POWER-02 on internal p5/p7 root/data with owner
SSH. Its voltage fault 0x8 remained latched; the focused read found battery about
3.74 V. Earlier OVP 0x10 remains unexplained. The fresh thermal acquisition read
shows truncated `0x73`/`0xfd` words. The 2082 RTC result follows from the wrong
epoch. Earlier inspected SPM entries/resumes were both zero. These failures
supersede older issue wording; never infer physical completion from this build.

SSH: `root@10.42.0.1`, client identity `~/.ssh/y2linux_ed25519`.
Strict private host pin:
`evidence-private/20260915-m5-entry/known_hosts`.
It matches the previously retained September 14 device key; default known_hosts
was not changed. Keep fingerprints, private/raw logs and calibration out of Git.
The usual host USB interface is `enp8s0f3u2`, host `10.42.0.2`.

## After owner reports it running

Run the linked qualification as one session through normal interfaces. The
owner handles SPFT, cables, Power and audible/visible/input observations; the
agent runs SSH commands and detached Y2DATA recorders. Start with identity,
offline history, UTC and plausible thermal readings, then sustained charging,
OPPs, s2idle/deep Power/deep RTC, resume regressions, reboot/hardware-off and
actual full/recharge/low-battery conditions. Do not reboot and label it resume.
Make targeted fixes only for concrete failures. Unavailable or unrun mandatory
gates remain open. Do not close #30 until the hardware passes.

Existing root/data, audio, storage, display/input, stock loaders and protected
calibration must remain intact. M5, GPU/lima, Y2PlayerNative and OTA implementation
remain outside this session. At genuine M4 completion update final physical
evidence, roadmap and #30, commit/push main clean, then stop.
