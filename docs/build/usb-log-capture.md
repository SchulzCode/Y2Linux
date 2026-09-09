# Capture the first Y2 CDC ACM trial

For [M2-USBACM-03](m2-usbacm-03-result.md), Linux host, standard Python 3 only.
Run from the repository **before powering on the test image**:

```sh
python3 tools/observation/usb_log_capture.py --output out/m2-usbacm-03/capture-01
```

Run on the real host with its `/dev` and `/sys`. A sandbox with private device
nodes can report no matching device even when the host enumerates the Y2.
USBACM-03 has now [enumerated successfully](../knowledge/m2-usbacm-hardware-result.md#usbacm-03-host-enumeration-confirmed);
capture-01 failed on sandbox visibility and capture-02 failed on tty permissions.
On this host the port is root:uucp 0660 and luca lacks access. For the next trial,
the owner can run this one capture in their terminal, supplying the local sudo
password there (not in chat):

```sh
sudo python3 tools/observation/usb_log_capture.py --output out/m2-usbacm-03/capture-03
```

This does not change groups, udev rules or global device permissions. It will
create root-owned capture files. Keep prior capture directories intact.

The new output directory must not already exist. The tool waits up to 90s for
one USB-backed ttyACM device with g_serial 0525:a4a7 and CDC ACM class/subclass.
This host wait does not extend the Y2's 60s-from-power-on observation limit.
It refuses multiple matches; optionally specify `--device /dev/ttyACM0` after
identifying the correct port. Selection still validates USB identity.
No device is flashed, reset, mounted or exported by this tool.

Boot the Y2 **without the USB cable**. Wait for `ATTACH USB CABLE NOW`, then
attach the data cable to this PC promptly. Leave it attached during this first
trial. If the prompt never appears, return a readable full-screen photo, including
POLL/PW/V, WACS and G rows if displayed. A failed live PMIC poll can replace
the prompt before it is noticed. Do not try connected-start recovery. The old connected-start PHY6a=BE guard remains.

If enumeration works, the tool opens raw 115200/8N1 without flow control, asserts
ACM DTR, sends only `LOG1\n`, and collects for at most 45s/1MiB. The baud setting
is ACM line coding, not a UART wiring requirement. It accepts no shell commands.
Capture may end on the candidate's 50s active-window disconnect; preserve the
usual manual BOOTIMG restoration by 60s from power-on. Do not reconnect during
this first trial: controller reconnect is not implemented or qualified yet.

The host user must already have permission to open the tty. If it fails, retain
`failure.json`; no host group, service or udev rule is changed automatically.
Do not substitute an unrelated tty or change permissions globally.

Return the whole capture directory:

- `capture.json`: USB parent/path, descriptors, identity, start time, byte count,
  SHA-256, stop reason, LOG1 header check and exit code.
- `raw.bin`: exact returned bytes, including native `/dev/kmsg` records and
  PID1 startup/heartbeat status. The first line should identify M2-USBACM-03.
- `chunks.jsonl`: monotonic receive timing and byte offsets.
- `failure.json`, if enumeration/identity/open failed.

A valid header and bytes are transport evidence, not automatic M2 qualification.
The report always leaves `hardware_qualified=false`; an evidence review must
establish early real kernel messages, increasing PID1 beats, timing, stability
and any GAP/ERR markers. If enumeration fails, the stage/error/IRQ/DEVCTL screen
and USB host diagnostics locate the failed part of the same combined attempt.
Keep one readable full-screen photo near BEAT 10 and another late in that same
boot, within 60s, if practical; note power-on and cable-attachment timing.

The host tool opens only a verified CDC ACM port and writes one log request plus
standard line-control operations. It provides log inspection, not ADB or an
interactive device shell. Its automated tests use PTYs/fake sysfs exclusively.
