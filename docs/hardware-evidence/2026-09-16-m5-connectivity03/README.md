# CONNECTIVITY-03: MD completion followed by kernel crash

The owner installed the CONNECTIVITY-03 BOOTIMG and Y2ROOT. Early authenticated
SSH verifies kernel/root markers and the corrected helper's SHA256
`098c12fbc028d7f3d7f02dc130e7c31c9a0db386df8a88dde81dcb6542ec02da`.
At uptime 48.502808 seconds, the kernel reports:

```text
MD1 calibration boot: stage=2 FS=841 restore=1 open=0 result=0 poweroff=0
```

Immediately afterward, SSH stops responding and the owner's photographs show
kernel faults on idle CPUs in timer/RCU interrupt processing. This is a crash,
not suspend or a successful connectivity boot. It establishes successful MD
completion and shutdown only; WMT/RF readiness and radio interfaces are unproven.

The preceding private trace identified request 53 as OPEN with flags `0x1010400`
and a doubled path separator before the existing factory seed. Normalizing empty
separator components fixes that failure without changing the seed or accepting
traversal. The build/host replay is in the [build evidence](../../build/evidence/y2linux-m5-connectivity-03/README.md).

[The targeted correction](../../knowledge/m5-connectivity04-corrections.md)
identifies the wrong CONN EMI register offset from retained stock instructions.
The multi-CPU fault pattern and timing are consistent with incorrect remote
memory translation; the lost SSH stream does not identify every corrupt access.

Private captures: `evidence-private/20260916-m5-connectivity03/` contains the FS
trace; `evidence-private/20260916-m5-connectivity03-crash/boot-016.txt` contains
the early boot stream. The two owner photographs are retained privately there.
No raw factory data, identities or command traffic is published. No assistant
flash/protected write occurred. CONNECTIVITY-03 BOOTIMG is not a usable fallback.
