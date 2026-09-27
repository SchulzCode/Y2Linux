# Platform power contract v1

**2026-09-28 status:** This page describes software behavior, with current
hardware limits in [platform state](../CURRENT_PLATFORM_STATE.md). Fix01's first
devices-stage suspend request lost recovery; charger refusal was not truly
exercised because BAT0 was Not charging. Full RTC/Power same-boot wake is
NOT_TESTED. Do not read the contract below as a passed suspend qualification.

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
supply positive net battery current. The **current shipped schema-2 policy is
enabled but provisional**: critical/low/recover values are 3.4/3.5/3.6 V, five
critical samples and ten-second grace. Its source text explicitly identifies a
conservative stock-boundary floor, not a measured discharge curve. Provisional
voltage-derived SOC is not calibrated pack current/capacity. A private
`/data/system/platform/power-policy.json` can override the shipped policy; the
loader accepts schema 1 or 2 with ordered voltage thresholds, critical_samples
(1–60), grace_seconds (3–30) and an evidence reference/source. Schema 2 also
validates ordered warning/critical/shutdown SOC values; SOC-triggered shutdown
requires hardware or calibrated-estimate confidence. An evidence string is not
software certification. Missing/invalid sensors report Unavailable. Debounce/
hysteresis implement Normal/Low/Critical/ShutdownPending; no charger limit
changes. [Power evidence](../knowledge/hardware-final-power-evidence.md) explains
missing pack current, pack temperature and measured reserve qualification.

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
