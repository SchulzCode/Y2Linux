# GPU implementation — 2026-09-18

The owner explicitly authorizes the production Linux 6.18 / Lima / Mesa GPU
platform and one integrated **Y2LINUX-GPU-01** candidate. Reborn and M6/OTA
implementation remain out of scope. Stop before physical deployment; only the
owner flashes the Y2. Preserve Y2DATA, all identities/preferences/bonds and RAM
exclusions. This supersedes the older handoff's GPU deferral.

Entry is clean `c70ff07`. Strict SSH using the retained private host-key pin
confirms `6.18.0-y2linux-m5-connectivity-10`, MemTotal 952480 KiB, existing
CCF/SPM/PWRAP owners and the unchanged memory map. SPM reports no error;
MFG status bit 4 is off in both `0x3f4c` status words. CPU/PMIC temperatures
are 44.200/44.543 C at inspection; these are not GPU temperatures.

[CONNECTIVITY-10 physical evidence](../hardware-evidence/2026-09-17-m5-connectivity10/README.md)
establishes calibration, controller `6582/8a01/8a00`, BTIF/AP_DMA, wlan0/cfg80211,
scans (23/12 results), radio restart and simultaneous Wi-Fi/BlueZ power-on with
zero errors/recoveries. These are settled foundations. M5 remains open for
connection/audio/reconnect/sustained coexistence qualification. The owner elects
to keep those tests pending during this session; they do not block GPU work.
M4 POWER-03 remains owner-accepted. Do not repeat M1–M5 discovery or M5 entry audit.

The own retained stock iomem/IRQ captures prove GP, L2, GP MMU, two PP MMUs,
PP0/PP1 at `0x13010000` and GIC hwirqs 202–207. Donor GPU material is a lead,
not this unit's rendering proof. Exact silicon revision must come from a
hardware version read; no revision is inferred from the SoC name.

Next: reconstruct the stock frequency/supply/domain contract, extend the existing
CCF/SPM owners, retain existing KMS, integrate Mesa Lima/kmsro and a bounded GBM /
EGL / GLES qualification program, validate emitted artifacts, then provide the
complete manual deployment receipt. No candidate or GPU physical pass exists yet.
