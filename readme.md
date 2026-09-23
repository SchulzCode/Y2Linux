# Y2Linux

Linux platform for the Innioasis Y2, built with Linux 6.18 and Buildroot. It
provides storage, power, connectivity, audio, recovery and diagnostics contracts
for [Y2Reborn](https://github.com/SchulzCode/Y2Reborn), the native music player.
The platform preserves the stock preloader/LK, rescue initramfs, BOOTIMG layout,
Y2ROOT/Y2DATA separation and standard Linux driver interfaces.

**Y2Linux Platform v1 Candidate is ready for owner-controlled physical
qualification.** Core software contracts are implemented, host tested, ARM built
and image validated. This integrated candidate is not physically or endurance
qualified; earlier hardware results do not qualify a later image.

Start with the [current platform state](docs/CURRENT_PLATFORM_STATE.md),
[completion and validation ledger](docs/validation/PLATFORM-V1-COMPLETION.md)
and [capability report](docs/validation/PLATFORM-V1-CAPABILITY-REPORT.md).
The [owner qualification plan](docs/validation/PLATFORM-V1-OWNER-QUALIFICATION.md)
groups the remaining work into sessions A–G.

| Area | Platform v1 software |
| --- | --- |
| Observation | Versioned JSON telemetry, bounded health checks, service readiness, CPU/memory/thermal measurements and boot evidence |
| Storage | Internal root/data identity, SD mount generations, ext4/FAT/exFAT, free-space reserves, safe scratch benchmarks and actual Reborn SQLite/library workloads |
| Power and time | Bounded shutdown intent/acknowledgement, configurable low-battery mechanism, RTC sanity, network time and kernel entropy readiness |
| Wi-Fi | Association/IP/route/DNS readiness, distinct failure states, single-owner reconnect and throughput qualification tools |
| Bluetooth | BlueZ + BlueALSA, SBC, actual negotiated PCM observation, bounded reconnect, Reborn AVRCP and gated Auto codec selection |
| USB | ACM and Ethernet, key-only USB-bound SSH/SFTP, staged file transfer with space/hash checks |
| Audio | ALSA/ASoC/CS43131 sink contract; the product profile remains S16 stereo at 44.1 kHz |
| Recovery and updates | Signed root-only OTA, rescue backup/readback/restore, boot-health acknowledgement, scoped resets and private state export |
| Qualification | Storage/library/resource benchmarks, passive endurance collection, source/package inventories and exact candidate identity |

On an installed Platform v1 image, these commands expose the application-facing
contract without initiating playback, radio connections or updates:

```sh
y2-status --json
y2-status cpu --interval 1
y2-status memory --reborn --pss
y2-health --json
y2-platform capabilities --json
y2-platform space
y2-platform update status
```

The [Platform API v1 contract](docs/architecture/platform-api-v1.md) defines
record identity, units, unavailable measurements, source generations and service
readiness. Capabilities distinguish **implemented**, **enabled** and **qualified**.
Evidence remains separate at each level: `IMPLEMENTED`, `HOST_TESTED`,
`ARM_BUILT`, `IMAGE_VALIDATED`, `PHYSICALLY_QUALIFIED`, `ENDURANCE_QUALIFIED`.

Explicit limits are recorded in the
[hardware gates](docs/knowledge/platform-v1-hardware-gates.md):

- Low-battery shutdown thresholds remain disabled pending pack/load evidence.
  Measured battery current, SOC and pack temperature are unavailable.
- Deep suspend, deeper cpuidle, AP watchdog recovery and retained panic/reset
  causes remain unqualified or blocked by missing evidence.
- USB host/OTG and USB Audio host support are unavailable pending board/VBUS proof.
- Optional AAC, aptX, aptX HD and LDAC A2DP encoders are absent. Auto has no
  production-eligible codec until matching qualification is recorded; manual SBC
  is available for owner qualification.
- True S24/S32 and preserved 24-bit internal output, 88.2/96 kHz and advanced
  CS43131 features remain gated. Driver support for 48 kHz does not enable it in
  the product profile; 32-bit I2S slots do not establish 32-bit sample precision.
- Automatic BOOTIMG OTA is excluded. Root-only OTA uses development trust;
  production signing and trusted rescue key changes remain owner-controlled.

The local candidate is `out/y2linux-platform-v1-candidate/`, release
`1.0.0-candidate.1`, built from Y2Linux `d04b95a` and Reborn `6c8aa12`. Later
documentation commits do not change those compiled identities. The package
contains BOOTIMG/Y2ROOT, a preserve-data scatter, fallback images, manifests and
hashes, a signed development root update, source bundles, tools and validation
receipts. It contains **no replacement Y2DATA image** and is not a public binary
release. Deployment and physical qualification belong to the owner.

For builds, use adjacent Y2Linux/Y2Reborn checkouts and follow
[source reconstruction](docs/build/platform-v1-reconstruction.md). It documents
the pinned Buildroot, kernel/toolchain, FFmpeg, Rust and package inputs, exact
source pair, owner-provided radio firmware and separate signing keys. Source
identity is reconstructable; byte-for-byte image reproducibility has not been
demonstrated. Firmware redistribution permission is not established.

The [Platform v1 roadmap](docs/planning/platform-v1-roadmap.md) and
[standing milestone audit](docs/planning/roadmap-gap-audit.md#standing-milestone-boundary-rule)
govern later scope changes. After owner acceptance of the advertised core,
feature development returns to Reborn and Y2Linux moves into platform maintenance.
The [documentation index](docs/README.md) separates current contracts from retained
historical knowledge and evidence. Superseded session task lists have been
consolidated into the linked records.

Reviewed evidence belongs in Git. Private captures, owner firmware, generated
images and caches remain outside tracked source. Never commit private signing or
SSH keys, calibration or NVRAM. Protected partitions, stock loaders and reserved
memory retain their existing boundaries.
