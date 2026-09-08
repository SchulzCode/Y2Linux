# Y2B-245 — Real screen result and sleep failure

Owner-reported hardware evidence, recorded 2026-09-08 in
[#20](https://github.com/SchulzCode/Y2Linux/issues/20#issuecomment-5592238984):

```text
WDT: STOPPED
FB: GUARD OK
PID1: 1
BEAT: 0
STAGE: STOP: SLEEP ERROR
PROC MOUNT RC: 0
SYSFS MOUNT RC: 0
LAST ERR: SLEEP -38
SLEEP RC: -38
```

This is a real device report, not the synthetic screen preview. It confirms
Linux 6.18 reaches initramfs PID1, the guarded inherited framebuffer text renderer
works, and proc/sysfs mounts return success. The displayed watchdog/guard status
records checks at the successful frame write. No completed sleep, increasing
heartbeat/frame sequence, timer accuracy or additional runtime fields are claimed.
M1's Linux-plus-PID1 objective remains achieved; periodic execution is unproven.
No new flash/restore chronology or stock verification is inferred from this report.

## Focused cause: missing time32 ABI, not POSIX timers

Audited local upstream v6.18 (`7d0a66e4bb9081d75c82ec4957c50034cb0ea449`):

- `arch/arm/tools/syscall.tbl:179`: syscall 162 uses `sys_nanosleep_time32`.
- `kernel/time/hrtimer.c:2204`: that implementation is gated by
  `CONFIG_COMPAT_32BIT_TIME` and calls `hrtimer_nanosleep` with CLOCK_MONOTONIC.
- `kernel/sys_ni.c:337`: `COND_SYSCALL(nanosleep_time32)` provides the weak fallback.
- `arch/Kconfig:1466`: time32 is a visible bool; allnoconfig disables it despite
  its normal 32-bit default. It is independent of `CONFIG_POSIX_TIMERS`.

The tested `out/m1-y2b245-diagnostics/kernel.config` disables both options.
Actual ELF `sys_call_table[162] = 0xc00309a8`, identical to `sys_ni_syscall` and
weak `sys_nanosleep_time32`. This explains the immediate -38 / ENOSYS without
requiring a timer, UART or framebuffer redesign. Existing mocked syscall tests
validated error handling but could not detect a missing kernel implementation.

[Y2B-250 #21](https://github.com/SchulzCode/Y2Linux/issues/21) adds only
`CONFIG_COMPAT_32BIT_TIME=y` to the kernel fragment. POSIX_TIMERS stays disabled.
The new validator inspects the real linked table and rejects an ENOSYS target or
weak fallback, including when config text incorrectly claims support. D08,
CPU0, watchdog, framebuffer, PID1 source and loader packaging are unchanged.

The fix still needs a device test: real syscall presence does not independently
prove timer interrupts/wakeup. The current task prepares a new offline candidate;
it does not authorize flashing it or beginning M2.
