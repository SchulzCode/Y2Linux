# REBORN-BASELINE-01 candidate evidence

Host/cross/rootfs validation only; no Reborn installation or physical acceptance.
The owner reports GPU-02 installed; pinned read-only SSH confirms kernel/root,
DRM/input/ALSA/storage/power interfaces and the BlueALSA ObjectManager API.

- Built Reborn source: `77e673232a586e1f027538861bcbbec3466b36fd`.
- Built integration: `08b0e7d17c453037edf64f58a1798ff4457ca0ce`.
- Unchanged kernel source: `7e318afbffe640c6bf9da458f61ac2108f7d5bde`.
- [Validation summary](validation-summary.json): 50 Rust, 20 host daemon, 3 tooling,
  8 ARM runtime/shell checks and 94 platform regressions pass; Clippy passes.
- [Offline rootfs build log](offline-rootfs.log): networking disabled by
  `bwrap --unshare-net`, cached Buildroot sources/SDK and vendored Cargo sources.
- [Rootfs checks](rootfs-checks.json), [manifest](package-manifest.json),
  [package checksums](package-SHA256SUMS), [versions](versions.json).
- [Platform regression log](platform-tests.log) includes 56 + 38 tests and ARM ABI.
- Reborn repository `docs/validation/evidence/` retains host test logs, ARM runtime
  JSON, ELF checks and read-only device receipts.

Installable package: `/home/luca/Dokumente/Code/Y2Linux/out/REBORN-BASELINE-01`.
The only selected partition is ANDROID/Y2ROOT. BOOTIMG is unchanged; Y2DATA and
protected partitions are unselected. Fallback is the exact GPU-02 root.

Full 34-item implementation/installation handoff:
`/home/luca/Dokumente/Code/Y2Reborn/docs/validation/REBORN-BASELINE-01.md`.
Stop for owner manual installation. Reborn Baseline 01 is not physically passed;
GPU-02 same-boot resume, M5 peer/association and all new Reborn device behavior
remain qualification gates. Earlier platform exclusions are superseded only by
the owner's explicit Reborn implementation request, not by these host results.
