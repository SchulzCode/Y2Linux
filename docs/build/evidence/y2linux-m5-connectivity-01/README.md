# M5-CONNECTIVITY-01 build evidence

Source: `022c701010c467904ab6025cd98535d3b861c771`. **Host validation passed; physical radio qualification pending.**
The owner installs this candidate manually. No assistant flash/deployment,
protected write or physical connectivity pass occurred in this implementation session.

- 81 production/M4/connectivity tests, emitted kernel/DT/initramfs checks,
  actual ARM EABI and radio-userspace version/plugin checks.
- 16 isolated package rejection cases, including data/protected selections,
  altered component/firmware/fallback receipts and damaged fallback bytes.
- Complete new rootfs and both fallback images verified; no Y2DATA payload.
- Earlier targeted host runs also exercised this unit's already acquired
  factory images: original inputs unchanged, partial-run recovery and stable
  persistent identities. Only these Boolean outcomes are public; raw inputs,
  generated identities and record hashes remain private.

Firmware is explicitly owner-supplied. No vendor redistribution permission is
established; this evidence directory contains hashes/configuration/logs, no
firmware, device calibration, credentials or bonds. The inherited GPL fullmac
source still emits unused/declaration warnings; no compiler error or detected
uninitialised/bounds/type violation is accepted as a build pass.

[Deployment receipt](../../y2linux-m5-connectivity-01-deployment.md) contains
the full 26-field handoff, exact image identities and qualification procedure.
`result.json` distinguishes build results from unperformed physical tests.
