# M4-POWER-02 integrated candidate receipt

Source commit `83d475ef71a3dd84f6e3cc483a7637b13520e4f7`. Production kernel `6.18.0-y2linux-m4-power-02`.
**Host checks pass; physical qualification pending. M4 remains open.**

- BOOTIMG: `out/y2linux-m4-power-02/BOOTIMG.img`, 5378048 bytes,
  SHA256 `1852dfc995953f86ef5c47349515e8d6f06478daa6c1a89efe0f223501c226c9`.
- Fallback: `out/y2linux-m4-power-02/fallback/BOOTIMG-previous.img`, 5361664 bytes,
  SHA256 `1c144874577e5cc103ee88aea346ece1e2a20d0b521054560acb7b84d0c4c706`; previously installed M4-CHARGE-01.
  Its 70-mA profile failed net-gain qualification and inhibits below 3.4 V;
  it is a kernel rollback, not a low-battery recovery solution.
- Root/data identities and original root source commit stay unchanged through
  the verified Storage06 BOOTIMG-only base. Rescue tools are rebuilt with the
  pinned production Bootlin ABI; original libraries remain byte-verified.
- 72 production/M4 tests pass, no skips. Actual ARM rescue executable exits
  with expected status 2 when loader metadata is unavailable under qemu-arm.
  Four mutated packages reject wrong root identity, root provenance, fallback
  hash and BOOTIMG address even after recalculating the checksum inventory.
- One integrated hardware image, 5378048 bytes. Host-only packaging
  corrections retained **byte-identical BOOTIMG content**. No device flash or
  new physical charging/suspend acceptance is recorded by these build logs.

Build logs retain initial rejected host packaging/resume attempts; subsequent
passes supersede those attempts without hiding them. No ROOT/DATA payload was
written or packaged. The owner's manual deployment and observed offline display
are the next acceptance boundary.
