# Platform v1 source reconstruction and release boundary

The candidate records an exact Y2Linux/Y2Reborn Git pair, kernel/rootfs release,
build ID, configurations, source archive hashes, patches, package versions,
ELF/FFmpeg checks and owner-provisioned firmware hashes. This is reconstructable
source identity. Byte-for-byte image reproducibility has **not** been demonstrated.
Buildroot userspace still uses the recorded maintainer host tools; the kernel and
qualification environment use the pinned isolated userspace. Host paths, tool
versions, generated metadata and filesystem construction may affect bytes.

Use adjacent `Y2Linux` and `Y2Reborn` checkouts. The candidate's `sources/*.bundle`
files retain the local commits without publishing either repository. Clone them,
then checkout the exact `build_git_commit` and `reborn_source_commit` from
`metadata/versions.json`. Do not replace the paired application with a later tree
without generating and validating a new release. Keep the owner's unrelated
working files outside those clean reconstruction checkouts.

Inputs and authority:

| Component | Exact reconstructable input |
| --- | --- |
| Linux 6.18 + isolated kernel tools | `tools/build/inputs.lock.json`, `connectivity-host.lock.json`; archive/APK URLs and hashes, source overlays in `kernel/patches/manifest.json` |
| Buildroot 2025.02.18 | `buildroot/inputs.lock.json`; exact upstream archive, hash, source epoch |
| ARM userspace compiler/sysroot | Pinned Bootlin external toolchain selected in defconfig; upstream Buildroot recipe/hash and generated package inventory |
| Package updates | `tools/production/modernize_buildroot.py`, local `buildroot/patches`, upstream LTS patches; inventory records every selected patch/hash |
| FFmpeg 9.0.2 | `Y2Reborn/FFMPEG_VERSION`, `tools/production/ffmpeg9.py` URL/hash; disabled encoders/network and actual linked-library verification |
| Rust 1.90.0 | Reborn `rust-toolchain.toml`; `tools/build/rust-toolchain.lock.json` pins official host/ARM component archives and distribution-manifest hash |
| Rust crates | Reborn `Cargo.lock`, checked-in `vendor/*/.cargo-checksum.json`, offline source replacement; `--locked --offline` |
| Local platform/Reborn sources | Exact paired Git bundles/commits, licenses and build scripts |
| Radio firmware/defaults | Owner supplies the matched stock extraction through `tools/connectivity/provision.py`; expected inventory/hashes are checked, no automatic firmware download |
| Device calibration/NVRAM | Remains on the owner's device under existing guarded provisioning; not in source bundles or release diagnostics |
| Update trust | Public registry and key IDs in source/rescue; private development/release signing keys are separate owner inputs, never device/repository artifacts |

The Rust hashes come from the official immutable
[1.90.0 distribution manifest](https://static.rust-lang.org/dist/channel-rust-1.90.0.toml).
A maintainer can download those component archives, verify each SHA-256, and use
their standard install scripts into a dedicated prefix (no system identity or
Git configuration change). Alternatively use rustup's exact 1.90.0 toolchain,
then retain its version and distribution receipt. No custom toolchain installer
or unverified moving nightly is required.

Prepare the locked host/kernel environment as documented in `environment.md`;
prepare Buildroot with `tools/production/prepare.py`. Use the recorded host
prerequisites (Linux, Python, compiler/make, bubblewrap/unprivileged namespaces,
Rust 1.90.0 and the standard Buildroot prerequisites). A fresh build is:

```sh
python3 tools/production/build.py --output out/y2linux-platform-v1-build \
  --owner-firmware OWNER_PROVISION_DIRECTORY
python3 tools/build/run.py --output out/y2linux-platform-v1-build -- \
  sh /project/tools/production/tests.sh
python3 tools/build/run.py --output out/y2linux-platform-v1-build -- \
  python3 /tmp/Y2Reborn/tools/build/qemu-check.py
```

The wrapper exposes no physical Y2 or host block devices. Buildroot downloads are
hash checked; retaining `.cache/buildroot-dl` and locked archives makes mirror
availability independent of later upstream retention. `release_inventory.py`
records exact selected downloads/expected hashes, patch hashes, host versions,
installed files, largest components and shared-library references. Missing source
hashes fail the inventory. It does not remove a plugin merely because no ELF has
a direct DT_NEEDED reference. No rootfs size optimization is claimed without
runtime evidence. Diagnostic tools remain deliberate candidate qualification
cost, including Python, iperf3, ALSA/DRM queries, strace and scratch benchmarks.

The final `metadata/release-inventory.json` covers 102 selected nonvirtual
packages and 87 strongly hashed remote download inputs. `metadata/legal-info`
contains the Buildroot license texts/manifests and its explicit warnings. The
legal-info pass succeeds; its generic collector does not save the Buildroot
source itself, Bootlin's external license files or a separate license file for
the local connectivity/updater packages. The pinned Buildroot/toolchain archives
remain reconstructable inputs, and the paired Git bundles plus repository/kernel
licenses retain the local source and SPDX declarations. Do not present these
collection warnings as a completed public-distribution legal review.
FFmpeg's `LICENSE.md` hash is pinned to the actual 9.0.2 archive; supplicant's
top-level README retains its original LTS hash (the nested README is different).

The footprint inventory sums bytes per regular pathname. Mesa's 32 repeated
hard-link names account for most of the difference between 439,025,495 reported
pathname bytes and 88,666,583 unique regular-file bytes. The 512 MiB final ext4
image uses about 112 MiB including filesystem metadata. Neither an alias nor a
library absent from DT_NEEDED is by itself evidence that a package is disposable.

`system_update.py` packages a fresh current-source pair with the established
preserve-data scatter and a hash-verified previous BOOTIMG/Y2ROOT fallback. It
refuses a dirty or mismatched source and emits no data image. The outer manual
package uses hashes; the root-only OTA bundle separately has an Ed25519 signed
manifest. Do not confuse a checksum, signature, successful QEMU run or source
bundle with secure boot or hardware qualification.

Security scope: trusted owner administration over key-only USB-bound SSH/SFTP;
no Wi-Fi owner shell, port forwarding, second SSH daemon, HTTP upload service or
writable mass-storage gadget. OTA verifies before mutation and never rewrites
active root. Network time is standard unauthenticated NTP, with explicit clock/
TLS readiness; it is not authenticated time. Root/preloader/LK secure boot is not
introduced. Most hardware services and Reborn still run as root; safe device/
D-Bus/filesystem privilege separation is deferred, not claimed achieved. D-Bus's
existing dedicated account remains. Publicly exposed multi-user hosting is out
of scope. Review malformed-media parser updates and pinned dependency advisories
as ongoing maintenance; this pass is not a claim of exhaustive vulnerability
absence. No remote service or mutable network credential is added to an image.

Owner-only firmware redistribution permission is not established. The candidate
and source bundle are local maintenance artifacts; a public binary distribution
needs its own firmware/license decision and production signing authority.
