# Platform power contract v1

Normal `/sbin/poweroff`, `/sbin/reboot`, Reborn user requests and future update
reboots enter `y2-platform shutdown poweroff|reboot --reason user|service|update`.
A private Unix socket uses SO_PEERCRED. Only root may request a transition; only
the recorded Reborn PID/start-time can acknowledge the current request UUID.
There is one monotonic deadline, never extended by duplicate requests.

`/run/y2/shutdown.json` publishes schema, current boot ID, request ID, intent,
reason, requested time, deadline, client identity and acknowledgement. Reborn
checks every 250 ms, stops audio, checkpoints session, stops scanning, checkpoints
WAL and closes the single database connection. Ready means those steps completed;
a failed step reports Failed. A missing/failed acknowledgement cannot block the
platform beyond the ten-second grace. The platform stops services (two bounded
three-second calls), records the journal (one second per attempt), syncs (five
seconds), then requests BusyBox init's standard shutdown (two seconds). No forced
reboot syscall or charger/PMIC programming is introduced. Kernel uninterruptible
I/O or final hardware poweroff cannot be guaranteed by a userspace deadline.

The dedicated supervised policy process runs independently of Reborn. Five short
exits exhaust its restart budget. Its once-per-second heartbeat becomes
Unavailable after five seconds; the status API never retains a stale Normal.
Restart during the same boot resumes a pending intent at its original deadline;
no intent is replayed across boots. Journal orderly_shutdown means orderly
software preparation reached its final record, not observed hardware power loss.

Battery observation uses BAT0 voltage_now/present. USB presence is not assumed to
supply positive net battery current. Default: thresholds disabled, PHYSICAL_GATE.
No numeric production threshold is provided. An owner-qualified private
`/data/system/platform/power-policy.json` may supply schema=1, enabled=true,
critical_uv < low_uv < recover_uv, critical_samples (1–60), grace_seconds (3–30),
and qualification_reference. The latter is an evidence reference, not software
certification. Restart the policy service after deliberate configuration. Missing
or invalid sensors/configuration report Unavailable, never fabricated SOC or
battery temperature. Debounce/hysteresis implement Normal/Low/Critical/
ShutdownPending; no charger limits are changed.

Owner qualification must establish pack identity, safe observation conditions,
voltage sag under representative peak load, warning and shutdown reserve,
sampling/debounce and recovery hysteresis. Test app acknowledgement, hung app,
blocked database and sync failure first with synthetic host fixtures. Only then,
under the separate owner-controlled power session, qualify selected thresholds,
poweroff/reboot and the behavior of USB-connected low voltage. Do not discharge
an unknown pack to infer a limit.

Suspend remains disabled for ordinary operation. See the explicit
[hardware gates and existing scoped activity lease](../knowledge/platform-v1-hardware-gates.md).
The owner qualification helper requires `y2-suspend --owner-qualify`; this flag
does not confer qualification. Screen blanking never invokes it.
