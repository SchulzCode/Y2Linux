# CONNECTIVITY-02 build evidence

Source `dd8446259880f60e5eb96125145bb7b623fb5929`, kernel `6.18.0-y2linux-m5-connectivity-02`.
BOOTIMG: 6150144 bytes, SHA256 `72b17b90d5ab21c2c52f957056f483f3af9949f0607ecc98c4c87c89aa7765f9`.
The package is `out/y2linux-m5-connectivity-02/`. Its sole payload is BOOTIMG.
The owner has now installed it; see the [physical result](../../../hardware-evidence/2026-09-16-m5-connectivity02/README.md).

- 83 production/M4/connectivity tests passed (56 + 27); ARM rescue ABI and shell checks passed.
- Emitted kernel/DT/memory/module/initramfs/BOOTIMG checks passed.
- The signed regulatory database matches the installed root build exactly. Its
  detached signature verified with OpenSSL CMS against Linux 6.18's regulatory
  certificates (`-purpose any -no_check_time`); no radio identity is involved.
- BOOTIMG-only packaging validated against the CONNECTIVITY-01 system-update
  package without requiring a Y2DATA image. All 14 isolated package rejection
  cases passed; original/fallback image hashes remained unchanged.
- The resulting kernel config differs only in LOCALVERSION. M4 and DT sources
  are unchanged. The retained fullmac sources still emit compiler warnings;
  this was a successful build, not a warning-free claim.

`production-tests.log`, `artifact-validation.log`, `package-validation.log`,
`package-rejection-tests.log` and `regulatory-check.json` retain the results.
Reproduce package rejection checks from the project root with
`PYTHONPATH=. python3 docs/build/evidence/y2linux-m5-connectivity-02/package-rejection-check.py`.

Y2ROOT remains `2025.02.17-connectivity.1`, image SHA256
`add6b375d33dbfa3f36e505c9784c934c3b31e2073041f514dcd2db524408e67`.
Y2DATA is preserved, not packaged. Fallback BOOTIMG is CONNECTIVITY-01,
SHA256 `a24b257a795b8ffea198e57344f20a514f4fa5e7148572ac8331afed9e996a96`; it restores the previous bootable but radio-failing
candidate. The accepted M4 BOOTIMG/root pair remains in the CONNECTIVITY-01
package's fallback directory. These host results do not close M5.
