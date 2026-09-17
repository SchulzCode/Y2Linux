# CONNECTIVITY-08: consume the MT6582 E2 RF calibration result

The [own-unit CONNECTIVITY-07 capture](../hardware-evidence/2026-09-17-m5-protocol/README.md)
locates EPROTO at opcode `0x14`, after both patches/reset replies, when a valid
630-byte RF calibration result exceeds the 256-byte WMT response buffer.
The controller reports success. This is independent of the already-working
factory reader, DMA interrupts and controller identification.

Keep the 256-byte command buffer and ordinary exact reply checks. Only during
an outstanding `01 14 01 00 01` request expecting the six-byte calibration
success event may MT6582/HVR `8a01`/FVR `8a00` return the observed 630-byte shape.
Require full STP mode, exact outer/inner lengths, event type/opcode, status zero,
and operation 1. Validate the entire CRC, consume the full frame and advance
sequence/ACK normally. Retain only the six-byte result prefix plus its real wire
length for WMT completion; do not retain/log/persist RF data or synthesize a
shorter wire length. All other oversized, unsolicited or failed results fail.

Both the ordinary six-byte reply and the observed extended result remain valid
under those checks. No global opcode-0x14 bypass like the donor's calibration
workaround is introduced. Patches, reset order, rails, firmware, own factory
contents, M4 and the memory/storage layout stay unchanged.

WMT failures now report opcode, parameter, mode, request/response/expected sizes
and at most six event prefix bytes. STP failures report parser byte counts.
Successful full-STP negotiation, each patch/reset and RF calibration are logged,
so a subsequent failure can be located without exposing radio traffic or RF data.

The executable regression exercises the actual STP and WMT functions with a
synthetic 630-byte event matching the physical header, every split point and
single-byte delivery, sequence/ACK retirement, duplicate suppression and a
following ordinary command. It rejects wrong silicon, length, status, operation,
opcode, unsolicited results, CRC damage, truncated frames and send failure;
the existing six-byte path remains checked. Synthetic RF bytes replace all
private payload bytes. The previous implementation rejects this regression.

Build a production CONNECTIVITY-08 BOOTIMG-only update using CONNECTIVITY-07's
verified userspace and BOOTIMG fallback. Preserve Y2ROOT/Y2DATA and stop for the
owner's established manual installation procedure. Then inspect the new stage
logs and drive the standard BlueZ/cfg80211 paths until a usable adapter appears
or the next specific failure is captured. Sysfs hci0 alone is not success.
M5 remains open; no broad audit or unchanged provenance repeat is required.
