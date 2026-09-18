# Reborn radio scan UI correction

**Ready for owner manual installation; physical qualification pending.** This is
a root-only application correction over the installed REBORN-SPLASH-01 system.

Scan now enables its radio, reports Starting/Scanning, publishes results, and
reports empty or failed scans. Off is distinguished from unavailable; radio
errors stay on their own screens. Discovery is bounded. New `rebornctl wifi`
and `rebornctl bluetooth` scan/on/off commands exercise the actual UI workers.

Built source `a8ae8b6723311b677662759082c55ad226441669`; integration
`640ecb828af1ba48edcc6203701edb89f66eb0ac`. The exact images, tests, implementation,
manual install/rollback and SSH qualification procedure are recorded at
`/home/luca/Dokumente/Code/Y2Reborn/docs/validation/REBORN-RADIO-UI-01.md` and in
the package's `INSTALL.md`.

Package: `/home/luca/Dokumente/Code/Y2Linux/out/REBORN-RADIO-UI-01`.

| Payload | SHA256 |
| --- | --- |
| `Y2ROOT.img` | `79b12172645c4755fdf3219fc6f99971d94de62c42f807a3613a43516d1f7df8` |
| `fallback/Y2ROOT.img`, exact installed splash root | `d40e9337e3a1149d12fdd240a59918b0d2c48e4e6a9974ab5b470685250d900c` |
| Existing BOOTIMG, unchanged | `8671de2900fd05cc80a9eb7a3541d5bbdab7e5dd96c18f73f44ca1b13beb19a5` |

Use **MT6582_reborn_root_only_scatter.txt**, **Download Only**, **ANDROID only**.
Keep BOOTIMG, USRDATA and all other rows unchecked/NONE; preserve Y2DATA. Do not
reuse the preceding splash package's two-image scatter. Fallback is also root-only.

The [validation receipts](evidence/reborn-radio-ui-01/README.md) record 64 Rust
tests, 20 daemon checks, four tooling tests, eight ARM runtime/shell checks,
Clippy, an offline production build, raw ext4 checks and exact content comparison.
Only two application binaries and three build identity files change. Root usage
increases by 52 KiB; the kernel, native libraries, services, firmware, splash,
charging program and partition geometry remain unchanged. No live install or
radio setting change was performed while developing this correction.
