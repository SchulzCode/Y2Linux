# GPU-02 retained build evidence

Source/evidence commit `7e318afbffe640c6bf9da458f61ac2108f7d5bde`.
**Host validation PASS; corrected physical suspend qualification PENDING.**
[Deployment receipt](../../y2linux-gpu-02-deployment.md) provides the full platform
contract, payload/fallback hashes, preservation rules and targeted next checks.
[GPU-01 physical evidence](../../../hardware-evidence/2026-09-18-gpu01/README.md)
records working hardware rendering and the observed blockers.

The previous build workspace was cloned with Btrfs reflinks into an independent
GPU-02 build directory; existing GPU-01 payloads remain unchanged. From clean
source HEAD, the kernel stage and Buildroot post-build/root stage were rerun:

```sh
python3 tools/production/build.py --output out/y2linux-gpu-02-build \
  --resume kernel --owner-firmware evidence-private/20260915-m5-entry/owner-radio-provision
python3 tools/build/run.py --output out/y2linux-gpu-02-build -- \
  sh /project/tools/production/tests.sh
python3 tools/production/system_update.py --build out/y2linux-gpu-02-build \
  --base out/y2linux-m5-connectivity-10 \
  --fallback-rootfs out/y2linux-m5-connectivity-07/Y2ROOT.img \
  --output out/y2linux-gpu-02
```

- `production-tests.log`: **56 + 38 = 94**, including independent stock CPU masks,
  bounded ACK failures, suspend zero-format cases and emitted GPU DT mutations.
- `configuration-receipt.json`: actual emitted kernel/Mesa options, compared
  against the new `.config` and Mesa Meson introspection.
- `root-correction-check.json`: raw ext4 contains the exact corrected helper,
  GPU-02 marker and matching source commit.
- `arm-graphics-check.log`: real ARM dynamic linking/help, bounded CLI and ARM
  shell syntax; not an emulator hardware-rendering claim.
- `gpu-schema-validation.log`: actual emitted node passes dtschema2026.6; full
  DTB matches GPU-01 byte for byte.
- `package-validation.log`: clean ext4, identity/ABI, fallback and Y2DATA
  preservation checks. `package-rejection-tests.log`/`.py`: nine rejected
  package mutations, with candidate images opened only for reading.
- `layout.json`, `rescue-manifest.json`, `versions.json`, `manifest.json`:
  emitted artifact identities and ownership bounds.
- `SHA256SUMS`: inventory for **`out/y2linux-gpu-02`**, whose images are not
  committed here. `RECEIPT-SHA256SUMS` covers this evidence directory separately.

The package installation guide is replaced by the complete GPU-02 deployment
receipt with absolute documentation links, then package hashes are regenerated.
No payload bytes change during documentation finalization. Linux/Lima, Mesa,
libdrm, qualification-tool and pinned source license receipts remain in the
package's `metadata/graphics-licenses`. No proprietary Mali userspace is shipped.
