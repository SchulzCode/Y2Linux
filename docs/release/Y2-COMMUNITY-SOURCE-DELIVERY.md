# Source delivery and distribution gate

This pass distinguishes technical codec support, local engineering packaging and
permission to publish. No public-distribution approval is inferred from successful
compilation, an OSS library license, a disabled endpoint or a legal-info archive.

The preserving package records exact Linux/Reborn commits, kernel/Buildroot inputs,
root/release versions, BOOTIMG/Y2ROOT hashes, fallback identities, firmware file
expectations, platform API and delivered capabilities. The capability record lists
selected build symbols and installed configuration separately from enabled,
experimental, hardware-conditional and physically qualified state. A fresh build
never manufactures a physical qualification record.

`tools/production/privacy.py` scans the **actual ext4 image**, through read-only
host extraction, for protected/private filenames and credential/key material.
`system_update.py` refuses packaging when the scan fails. The existing partition
allowlist checks remain independent. The public-distribution check fails separately
for unapproved firmware and compiled optional codecs, even when their endpoint is
disabled. This engineering candidate remains owner-local.

For a later public release, retain and supply the exact corresponding sources,
patches, configuration and build/install scripts, not just a commit URL or a binary
SDK. Preserve copyright/license notices and include the selected packages' license
texts. Record the chosen source-delivery method and review obligations for the
actual GPL/LGPL versions and linking arrangements. Reborn's native media membrane
uses shared FFmpeg libraries; actual ELF dependencies, configuration and source
archives belong in its receipt. This is an engineering checklist, not a legal
clearance determination. The [GNU licensing FAQ](https://www.gnu.org/licenses/gpl-faq.en.html)
provides primary guidance; applicability and codec/patent/certification decisions
remain for the owner/legal review.

Run Buildroot `legal-info` and preserve its README warnings, manifests, source
archives and licenses. Its collection has limits: external toolchain and local
packages may require additional source material. Include the pinned Buildroot
archive itself, Linux source archive and overlays, both source snapshots/bundles,
Cargo lock/vendor checksums, exact Bootlin toolchain source expectations, codec
source ledger and local source SPDX/licenses. The
[Buildroot manual](https://buildroot.org/downloads/manual/manual.html#legal-info)
explicitly describes the collection and its limitations.

Reconstruction uses the pinned dependency archives and adjacent clean source
checkouts described in `docs/build/platform-v1-reconstruction.md`. Current-source
kernel/Buildroot/Reborn builds and dependency closure must pass before sealing.
Source identity is reconstructable; byte-identical filesystem and complete clean
rebuild equivalence have not been demonstrated. No such claim is made.

Unresolved public decisions remain: matched MediaTek firmware/defaults rights;
AAC/aptX/aptX-HD/LDAC redistribution/certification questions from the existing
codec ledger; complete external-toolchain source delivery; release signing/channel
ownership. The owner-local candidate does not remove these features to hide the
questions.
