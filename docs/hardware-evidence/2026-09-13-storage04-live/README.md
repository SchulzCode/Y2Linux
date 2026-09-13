# Storage04 host inspection — 2026-09-13

Owner reports manually flashing `out/y2linux-production-v1-r4/` and identifies
the earlier failure to turn on as an empty battery. This is owner evidence;
no battery voltage, capacity or charging measurement was obtained here.
Charging management remains unimplemented/unqualified.

## Device diagnostics after host permission correction

The owner granted access to the ACM node. A 20-second LOG1 capture succeeded
with the expected `Y2LOG1 Y2LINUX-PLATFORM` header; a second bounded read continued
the queued stream. See [initial capture metadata](acm/capture.json),
[initial raw log](acm/raw.bin), [follow-up metadata](acm-followup/capture.json)
and [follow-up raw log](acm-followup/raw.bin). The follow-up starts with queued
records rather than a new protocol header; it is continuation evidence, not a
second independently validated handshake. Logs contain historical snapshots and
kernel timestamps from this boot, not solely measurements at host capture time.

- Kernel `6.18.0-y2linux-storage04`; CPUs `0-3` online; `MemTotal=954380 KiB`
  (932.0 MiB), with captured `MemAvailable` around 900–901 MiB. This is an idle
  observation, not a memory stress test.
- Internal MMC `FNX2MB` is detected as `mmcblk0`, 7634944 KiB / 15269888 sectors.
  The partition snapshot has no `mmcblk0pN` devices. The driver reports
  `partition read sector=0 bytes=4096 signature=0000` at 0.441709 seconds.
  The instrumentation reads bytes 510 and 511 of the returned first sector;
  these do not match the MBR signature `55aa`. This does **not** establish why
  those bytes were returned or prove an erased/corrupted physical partition
  table. No raw physical readback has been obtained.
- At 41.163161 seconds the kernel log explicitly reports:
  `Y2RESCUE: missing/invalid internal Y2ROOT or Y2DATA; staying in rescue`.
  The earlier mount snapshot contains only RAM and virtual filesystems.
  Normal Buildroot boot and internal-root/data mounting did not succeed.
- USB ACM transport works for this inspection. The captured `usb0` counters
  are zero. Production Ethernet addressing and Dropbear startup are in the
  normal rootfs init scripts, which this failed boot does not reach. Thus
  the host's missing carrier/SSH timeout is consistent with rescue, not proof
  of a separate Ethernet driver defect.
- The power snapshot provides only `y2-usb-presence`, `ONLINE=1`. No battery
  charge percentage, voltage or charging-state measurement is available.
- Display/DRM state and registered navigation, wheel, keypad and audio-related
  input devices are present. This capture does not qualify their full behavior
  or audio playback. Repeated CMD8/CMD55 errors name the removable controller
  `11240000.mmc`; they are distinct from the internal controller `11230000.mmc`.

Inspection identifies the immediate boot blocker. The next diagnostic boundary
is read-only verification of the actual internal metadata/addressing against the
expected layout, preserving table/protected bytes. Do not rewrite a partition
table from the signature observation alone. The missing matching SSH key remains
a separate access prerequisite once normal userspace can boot. No build, flash,
device block write, reboot, charging experiment or milestone transition occurred.

## Initial host inspection (before permission correction)

Read-only host observation at approximately 18:56 Europe/Berlin:

- USB `0525:a4aa` enumerates as a CDC Ethernet/ACM composite device. Its
  manufacturer descriptor reports `Linux 6.18.0-y2linux-storage04 with musb-hdrc`.
  This establishes the advertised running kernel identity, not an image hash or
  successful internal-root handover.
- Host interface `enp8s0f3u2` is administratively up but has `NO-CARRIER`, no IP
  address and NetworkManager state unavailable. SSH to `10.42.0.1` timed out;
  no SSH session or remote inspection command ran.
- `/dev/ttyACM0` exists, owned by root:uucp with mode 0660. A guarded attempt
  to open it for the existing LOG1 diagnostic protocol failed with permission
  denied before sending the request. Noninteractive sudo requires a password.
  No serial data was captured.
- The documented private key at
  `out/y2linux-production-private/id_ed25519` is missing. The existing
  `~/.ssh/y2linux_ed25519.pub` does not match the public key in the packaged
  Y2DATA template (comparison uses key type and key material). Current on-device
  authorization is unknown; the template comparison does not establish it.

See [USB events](host-usb-kernel.txt), [network state](network.txt),
[NetworkManager state](networkmanager.txt), [USB inventory](usb.txt) and
[inspection metadata](inspection.json).

At this initial point, device-side storage, RAM/CPU state and boot stage were
unverified. The subsequent diagnostics above supersede those limitations where
explicitly evidenced.
