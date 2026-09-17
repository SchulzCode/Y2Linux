# GPU-01 retained build evidence

Source: `7c43557a38fac6bbdb0ab5628cffe131ebda6360`.
**Host validation PASS; physical rendering PASS; deep suspend qualification FAILED.**
[Physical result](../../../hardware-evidence/2026-09-18-gpu01/README.md).
The owner performs deployment; no assistant flash occurred.

The [complete deployment receipt](../../y2linux-gpu-01-deployment.md) contains
all platform fields, both payloads, fallback hashes, manual steps and the
single ordered physical procedure. [The GPU contract](../../../knowledge/gpu-platform.md)
describes implementation and the provisional standard Reborn interface.

Build command, from the canonical repository:

```sh
python3 tools/production/build.py --output out/y2linux-gpu-01-build \
  --resume kernel --owner-firmware evidence-private/20260915-m5-entry/owner-radio-provision
python3 tools/build/run.py --output out/y2linux-gpu-01-build -- \
  sh /project/tools/production/tests.sh
python3 tools/production/system_update.py --build out/y2linux-gpu-01-build \
  --base out/y2linux-m5-connectivity-10 \
  --fallback-rootfs out/y2linux-m5-connectivity-07/Y2ROOT.img \
  --output out/y2linux-gpu-01
```

The same build workspace was resumed for configuration/fault-containment fixes;
only the final integrated candidate was packaged. Final kernel/root source
receipts match. The generated package's installation guide was then replaced
with the full deployment receipt, its links made absolute, and package checksums
regenerated. Image bytes and source receipts are unchanged by that documentation.

- `configuration-receipt.json`: actual kernel/Mesa build options and test counts.
- `layout.json`: emitted kernel/rescue/DT/BOOTIMG validation and hashes.
- `production-tests.log`: **56 + 36 = 92** passing tests, including GPU power
  ACK fault containment, exact UAPI and rejected DT/IRQ/memory mutations.
- `arm-graphics-check.log`: ARM dynamic linking/help/bounded CLI and telemetry
  syntax. It does not establish hardware EGL execution.
- `gpu-schema-validation.log`, `schema-tools.txt`: dtschema 2026.6, actual emitted
  GPU node and exact DTB hash.
- `package-validation.log`: ext4, source/identity/fallback/scatter and new
  graphics ELF plus raw-ext4/tar verification.
- `package-rejection-tests.log`, `package-rejections.py`: nine isolated failures
  correctly rejected; existing candidate/fallback images were opened for reading.
- `manifest.json`, `SHA256SUMS`: the **local package's** complete manifest and
  checksum inventory; images themselves are not committed here. Run that
  SHA256SUMS file from `out/y2linux-gpu-01`, not from this receipt directory.

`RECEIPT-SHA256SUMS` separately covers the files in this receipt directory.
The owner-only connectivity firmware/provenance restrictions are retained.
Mali Android userspace, persistent credentials and calibration dumps are absent
from these public evidence files. Remaining M5 connection/audio tests stay
pending by owner choice; GPU #34 and Reborn readiness await the targeted suspend correction and remaining physical checks.
