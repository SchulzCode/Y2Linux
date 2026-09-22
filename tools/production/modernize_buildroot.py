#!/usr/bin/env python3
"""Apply the reviewed userspace package pins to the locked Buildroot tree.

The Buildroot release remains the 2025.02 LTS line. These package updates are
applied after extracting that exact release, so the resulting source tree is
fully reproducible without carrying a private Buildroot fork.
"""
from pathlib import Path

BLUEZ_VERSION = "5.87"
BLUEZ_SHA256 = "26bdcf2cebd7310c6f598850606b037ef0c515fe6608ebc54d22c50c4c32b35f"
SBC_VERSION = "2.2"
SBC_SHA256 = "a1ada76ef35e5af9c2fbd063754dc9e37a8d989417c6eb1ecebb089b1383ae9e"
BLUEZ_ALSA_VERSION = "5.0.0"
BLUEZ_ALSA_SHA256 = "e1249cbebd24925c977814f62adab71f2ebd771e28e6a4ac58bb1ac97ce3beb5"
SQLITE_VERSION = "3.53.4"
SQLITE_TAR_VERSION = "3530400"
SQLITE_SHA256 = "0e9483900e92cd5de8fd48d16bf9200145a61f7fd5be542a5ac81d8a9516eb9c"
ALSA_LIB_VERSION = "1.2.16.1"
ALSA_LIB_SHA256 = "f740db7f488255944ffd4428416ee3390a96742856916433df468c281436480e"
ALSA_UTILS_VERSION = "1.2.16"
ALSA_UTILS_SHA256 = "092399d5e8749a1d5e188e393157521cec4b75693b60ebb79bbce728cff2232c"
DROPBEAR_VERSION = "2026.94"
DROPBEAR_SHA256 = "e098034a843699200c8c977a991fff73159735bf795d5f72ef672c41a6b1ae81"


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text()
    if old in text:
        path.write_text(text.replace(old, new, 1))
    elif new not in text:
        raise ValueError(f"expected pin missing from {path}: {old}")


def replace_hash(path: Path, old_name: str, new_line: str) -> None:
    lines = path.read_text().splitlines()
    replaced = False
    result = []
    for line in lines:
        if old_name in line:
            result.append(new_line)
            replaced = True
        else:
            result.append(line)
    if not replaced and new_line not in lines:
        raise ValueError(f"expected source hash missing from {path}: {old_name}")
    path.write_text("\n".join(result) + "\n")


def apply(buildroot_source: Path) -> None:
    bluez = buildroot_source / "package/bluez5_utils"
    headers = buildroot_source / "package/bluez5_utils-headers"
    sbc = buildroot_source / "package/sbc"
    bluealsa = buildroot_source / "package/bluez-alsa"
    sqlite = buildroot_source / "package/sqlite"
    alsa_lib = buildroot_source / "package/alsa-lib"
    alsa_utils = buildroot_source / "package/alsa-utils"
    dropbear = buildroot_source / "package/dropbear"

    replace_once(bluez / "bluez5_utils.mk", "BLUEZ5_UTILS_VERSION = 5.79", f"BLUEZ5_UTILS_VERSION = {BLUEZ_VERSION}")
    replace_once(headers / "bluez5_utils-headers.mk", "BLUEZ5_UTILS_HEADERS_VERSION = 5.79", f"BLUEZ5_UTILS_HEADERS_VERSION = {BLUEZ_VERSION}")
    replace_hash(
        bluez / "bluez5_utils.hash",
        "bluez-5.79.tar.xz",
        f"sha256  {BLUEZ_SHA256}  bluez-{BLUEZ_VERSION}.tar.xz",
    )
    for patch_name in (
        "0001-gdbus-define-MAX_INPUT-for-musl.patch",
        "0002-Leave-config-files-writable-for-owner.patch",
        "0003-input-fix-HID-compilation-w-o-HoG.patch",
        "0004-input-fix-HoG-compilation-w-o-HID.patch",
    ):
        patch = bluez / patch_name
        if patch.exists():
            patch.unlink()

    replace_once(sbc / "sbc.mk", "SBC_VERSION = 2.0", f"SBC_VERSION = {SBC_VERSION}")
    replace_hash(
        sbc / "sbc.hash",
        "sbc-2.0.tar.xz",
        f"sha256  {SBC_SHA256}  sbc-{SBC_VERSION}.tar.xz",
    )

    replace_once(
        bluealsa / "bluez-alsa.mk",
        "BLUEZ_ALSA_VERSION = 4.3.1",
        f"BLUEZ_ALSA_VERSION = {BLUEZ_ALSA_VERSION}",
    )
    replace_once(bluealsa / "bluez-alsa.mk", "--enable-cli", "--enable-ctl")
    # BlueALSA 5 has more optional A2DP/profile switches. Keep every one
    # disabled in the product package; a future codec decision must change the
    # Buildroot configuration deliberately and add its own evidence.
    marker = "\t--disable-debug-time \\\n"
    text = (bluealsa / "bluez-alsa.mk").read_text()
    disables = (
        "\t--disable-asha \\\n"
        "\t--disable-faststream \\\n"
        "\t--disable-lc3plus \\\n"
        "\t--disable-lc3-swb \\\n"
        "\t--disable-ldac \\\n"
        "\t--disable-lhdc \\\n"
    )
    if "--disable-ldac" not in text:
        if marker not in text:
            raise ValueError("BlueALSA configure option anchor missing")
        (bluealsa / "bluez-alsa.mk").write_text(text.replace(marker, disables + marker, 1))
    replace_hash(
        bluealsa / "bluez-alsa.hash",
        "bluez-alsa-4.3.1.tar.gz",
        f"sha256  {BLUEZ_ALSA_SHA256}  bluez-alsa-{BLUEZ_ALSA_VERSION}.tar.gz",
    )
    replace_once(
        bluealsa / "bluez-alsa.hash",
        "956564dcf06ba65cd7a5eb6cdfd695e2ab0f09ea9684e4eaf079f5d533bd206d",
        "47a67fe6b44d4aff90c837ebce38814bf89fdf5efd6bc6c9c078f960c35f61b8",
    )

    replace_once(sqlite / "sqlite.mk", "SQLITE_VERSION = 3.50.4", f"SQLITE_VERSION = {SQLITE_VERSION}")
    replace_once(sqlite / "sqlite.mk", "SQLITE_TAR_VERSION = 3500400", f"SQLITE_TAR_VERSION = {SQLITE_TAR_VERSION}")
    replace_once(sqlite / "sqlite.mk", "https://www.sqlite.org/2025", "https://www.sqlite.org/2026")
    sqlite_mk = sqlite / "sqlite.mk"
    sqlite_text = sqlite_mk.read_text()
    sqlite_text = sqlite_text.replace(
        "# 0002-CVE-2025-70873.patch\nSQLITE_IGNORE_CVES += CVE-2025-70873\n\n",
        "",
    )
    sqlite_text = sqlite_text.replace(
        "# 0003-CVE-2026-11822.patch\nSQLITE_IGNORE_CVES += CVE-2026-11822 CVE-2026-11824\n\n",
        "",
    )
    sqlite_mk.write_text(sqlite_text)
    replace_hash(
        sqlite / "sqlite.hash",
        "sqlite-autoconf-3500400.tar.gz",
        f"sha256  {SQLITE_SHA256}  sqlite-autoconf-{SQLITE_TAR_VERSION}.tar.gz",
    )
    for patch_name in (
        "0001-disable-rpath.patch",
        "0002-CVE-2025-70873.patch",
        "0003-CVE-2026-11822.patch",
    ):
        patch = sqlite / patch_name
        if patch.exists():
            patch.unlink()

    replace_once(alsa_lib / "alsa-lib.mk", "ALSA_LIB_VERSION = 1.2.13", f"ALSA_LIB_VERSION = {ALSA_LIB_VERSION}")
    alsa_lib_text = alsa_lib.joinpath("alsa-lib.mk").read_text()
    alsa_lib_text = alsa_lib_text.replace(
        "ifneq ($(BR2_PACKAGE_ALSA_LIB_ALISP),y)\nALSA_LIB_CONF_OPTS += --disable-alisp\nendif\n",
        "",
    )
    alsa_lib.joinpath("alsa-lib.mk").write_text(alsa_lib_text)
    replace_hash(
        alsa_lib / "alsa-lib.hash",
        "alsa-lib-1.2.13.tar.bz2",
        f"sha256  {ALSA_LIB_SHA256}  alsa-lib-{ALSA_LIB_VERSION}.tar.bz2",
    )
    # The remaining Buildroot patch targets no-MMU platforms. Y2's Cortex-A7
    # build has an MMU, and its old context no longer applies to ALSA 1.2.16.1.
    # Do not carry an unrelated no-MMU delta into this target package.
    for patch_name in (
        "0001-Don-t-use-fork-on-noMMU-platforms.patch",
        "0002-configure-Make-sequencer-dependent-on-rawmidi.patch",
    ):
        patch = alsa_lib / patch_name
        if patch.exists():
            patch.unlink()

    replace_once(alsa_utils / "alsa-utils.mk", "ALSA_UTILS_VERSION = 1.2.13", f"ALSA_UTILS_VERSION = {ALSA_UTILS_VERSION}")
    alsa_utils_text = alsa_utils.joinpath("alsa-utils.mk").read_text()
    alsa_utils_text = alsa_utils_text.replace(
        "$(@D)/alsactl/alsa-restore.service",
        "$(@D)/alsactl/conf/alsa-restore.service",
    ).replace(
        "$(@D)/alsactl/alsa-state.service",
        "$(@D)/alsactl/conf/alsa-state.service",
    )
    alsa_utils.joinpath("alsa-utils.mk").write_text(alsa_utils_text)
    replace_hash(
        alsa_utils / "alsa-utils.hash",
        "alsa-utils-1.2.13.tar.bz2",
        f"sha256  {ALSA_UTILS_SHA256}  alsa-utils-{ALSA_UTILS_VERSION}.tar.bz2",
    )

    replace_once(dropbear / "dropbear.mk", "DROPBEAR_VERSION = 2026.93", f"DROPBEAR_VERSION = {DROPBEAR_VERSION}")
    replace_hash(
        dropbear / "dropbear.hash",
        "dropbear-2026.93.tar.bz2",
        f"sha256  {DROPBEAR_SHA256}  dropbear-{DROPBEAR_VERSION}.tar.bz2",
    )
    patch = dropbear / "0001-scp-fix-build-with-gcc-14.x.patch"
    if patch.exists():
        patch.unlink()


if __name__ == "__main__":
    import sys

    apply(Path(sys.argv[1]))
