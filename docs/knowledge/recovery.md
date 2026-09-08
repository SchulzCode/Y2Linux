# Recovery capability and next evidence boundary

Date: 2026-09-08. Task: [Y2E-120](https://github.com/SchulzCode/Y2Linux/issues/5). Status: **M0 NOT PASSED**. This is a capability/gap assessment, not an executable restore procedure.

## Capability matrix

| Requirement | Current evidence | Decision |
| --- | --- | --- |
| Identifiable stock-family package | OriginalFirmware boot/recovery/system/scatter hashes recorded; original download provenance unknown | Present, authenticity/board compatibility unproven |
| Alternate Y2 package | `y2_v3.2.0_FM-20260813/` has different boot/recovery/kernel/system hashes but identical scatter | Separate candidate; do not mix images or infer installation from dates |
| Per-device boot/recovery copies | None established by this research | BLOCKED: packaged images are not current-device dumps |
| Per-device calibration/NVRAM/protect copies | No verified device-specific backup set located in the scoped inventory | BLOCKED: generic factory files do not replace personalization |
| Safe access to exact ranges | Normal boot/recovery metadata reconciles; block nodes are restricted; special regions unresolved | BLOCKED: no raw reads, guessed ioctl or privilege workaround |
| Independent backup retention | Private research captures retained on host; no independently retained partition backups established | BLOCKED |
| Flashing host/tool/DA | Windows flasher/DA candidates found; no compatible tested combination established | BLOCKED |
| Emergency reachability | Only authorized normal Android USB/ADB observed | UNKNOWN for stock recovery, preloader and BROM; no mode transition attempted |
| Exact stock restore procedure | Historical instructions exist but no same-device readback/restore evidence established | INCOMPLETE |
| Actual stock boot/recovery restore rehearsal | Not performed or evidenced here | BLOCKED pending prerequisites and explicit authorization |
| Recovery power conditions | Framework battery output has conflicting fields | UNKNOWN; need trustworthy passive power evidence before an operator procedure |

## Tool/package candidates discovered offline

No `flash_tool`, `SPFlashTool`, `mtk` or `mtkclient` executable was on PATH. ADB and fastboot are installed; a host fastboot executable does not prove this device supports that mode or a temporary boot command.

`Y1Software/flash_tool.exe` is a Windows PE32/i386 GUI executable, SHA-256 `8aa16765f811f9b665dd1dc110f68081a4bcd8efa19832cb49b23cfd031881df`. Several bundled download agents were hashed; see [artifact-manifest.tsv](artifact-manifest.tsv). Exact flasher release/version and DA compatibility remain UNKNOWN. Strings include a build time and a message recommending a different version for old platforms; neither identifies a tested recovery version. No executable, DLL, download agent or updater was run.

The accompanying package scatter is **MT6572**, with different image sizes, while `BromAdapterTool.ini` names MT6582. These conflicting package/config clues are not evidence of Y2 compatibility. The Y1 firmware/scatter must not be used as a Y2 restore set. Tool reuse, if considered later, requires independent compatibility and command-semantics analysis.

An existing source checkout at `build/inspect-innioasis-updater` identifies Git revision `07d220d62de8a02b6eac9d8c06b3427388ad8195` and contains mtkclient/DA sources. This is an offline research candidate, not an approved acquisition tool. Inspecting source is separate from connecting a tool or uploading code to the device.

The additional Y2 FM package has boot SHA-256 `5ef1bdf28481ee0bf5f3528c1ddd91cf3f4d2d5f39e4d0ea049a8137a30f6af6`, recovery `319ae8113b7741c13a0ab6254575c9a0e70336c4290fca49dd33abb4f85f29b0`, and the same scatter as OriginalFirmware. Its images differ. This makes exact installed-build and physical-revision identification a prerequisite for selecting a fallback.

## Conditions for a later acquisition/restore procedure

Before any device readback tool is executed, identify its exact executable/source revision, download agent if any, host/driver, supported MT6582 protocol, every command sent during connection, address-space/offset/length semantics and whether any connection step writes persistent state or runs a payload. A tool advertising readback does not make its whole connection path passive. The present non-root block-node permissions provide no approved route.

For each backup object, record private device identity, hardware revision, source region/offset/length, consistent capture state, command/status, complete host byte length and SHA-256, and independent verification. Repeat hashes alone do not prove a range is correct. Mutable protect/NVRAM data needs a consistency strategy; do not unmount or freeze filesystems without a separately reviewed procedure. Retain a second verified copy on independent storage before any restore. Capture provenance must distinguish package images, exact device dumps, expanded sparse files and full-partition contents.

A future operator runbook must pin the compatible stock package, device/revision, tool/DA/host, expected USB identities and connection sequence, trusted battery/power conditions, timeouts, target list, abort conditions, readback and post-restore checks. All these currently unresolved fields must remain visibly unresolved. Ordinary permitted restore targets should be bounded to the specifically reviewed stock boot/recovery operation; exclude preloader/LK, partition tables, calibration and formatting. Do not deliberately corrupt the device to demonstrate recovery. If the tool cannot honor the allowed target boundary, stop and redesign the procedure.

Reachability must eventually be proven independently of a working Android installation, but actual mode transitions/tool connections and restoration are separate operator steps with explicit authorization after their procedures and backups are ready. A successful boot after a write is insufficient without readback and checks that storage, controls, audio, radios and calibration remain intact.

## Three next proofs, in priority order

1. **Recovery provenance and acquisition-method review:** reconcile owner restoration history/backups, OriginalFirmware versus the Y2 FM package, exact flasher/DA/source capability and region semantics. Deliver one reviewed non-destructive acquisition proposal, or a precise blocker. No device-tool connection yet.
2. **Bootloader handoff/address proof:** inspect the exact LK/preloader artifacts offline to explain the header/runtime address difference and ATAG/DT/memory handoff. No patch, payload execution, DTS or guessed load address.
3. **Passive subsystem baseline:** scope one subsystem at a time, beginning with contradictory battery reporting and hardware identity. Reuse verified archived audio/input evidence; then collect only known-safe standard metadata needed for display/input, power and radio/firmware identification. No arbitrary sysfs register dumps or radio toggles.

These are research directions for the next rolling-wave boundary, not implementation issues and not authorization for the operations they investigate. No implementation task is ready at this point.

## Gate checklist

- [x] Artifact provenance catalog and generated-package integrity limitations recorded.
- [x] Current non-root identity/access baseline and error semantics recorded.
- [x] Normal boot/recovery partition metadata reconciled; special cases explicit.
- [x] Exact package boot/kernel/ramdisk structure and derivative lineage established.
- [x] Recovery capability assessment and prioritized next proofs recorded.
- [ ] Current-device boot/system lineage and physical revision verified.
- [ ] Bootloader handoff, memory reservations and relevant critical hardware unknowns resolved sufficiently for a concrete experiment.
- [ ] Compatible exact recovery tool/DA/host/path established independently of Android.
- [ ] Complete device-specific boot/recovery/calibration backups verified and independently retained.
- [ ] Trustworthy power and capture-consistency prerequisites established.
- [ ] Explicitly authorized stock restoration rehearsal, readback and functional/calibration checks recorded.

The five initial research tasks can close with their reports while this milestone stays open. Research completion is not recovery proof and does not authorize an experimental Linux boot.
