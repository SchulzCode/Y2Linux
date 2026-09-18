#!/usr/bin/env python3
"""Pin the Buildroot FFmpeg package to the Reborn FFmpeg 9.0.1 source."""
import hashlib
import re
from pathlib import Path

VERSION = "9.0.1"
ARCHIVE = f"ffmpeg-{VERSION}.tar.xz"
ARCHIVE_SHA256 = "cf38e0e28c7e5605942c4a77755349b0145804a397af37eb1fb4c77cb237f635"


def apply(buildroot_source: Path, download_dir: Path) -> None:
    package = buildroot_source / "package/ffmpeg"
    mk = package / "ffmpeg.mk"
    hashes = package / "ffmpeg.hash"
    archive = download_dir / "ffmpeg" / ARCHIVE
    if not archive.is_file():
        raise FileNotFoundError(f"missing locked FFmpeg archive: {archive}")
    if hashlib.sha256(archive.read_bytes()).hexdigest() != ARCHIVE_SHA256:
        raise ValueError("FFmpeg 9.0.1 archive hash mismatch")
    mk_text = mk.read_text()
    mk_text = mk_text.replace("FFMPEG_VERSION = 6.1.5", f"FFMPEG_VERSION = {VERSION}")
    mk_text = re.sub(r"\n\t(?:--disable-crystalhd|--disable-dxva2|--enable-runtime-cpudetect|--disable-hardcoded-tables|--disable-mipsdspr2|--disable-mipsdsp|--disable-msa|--disable-postproc) \\\n", "\n", mk_text)
    mk_text = re.sub(r"\t--disable-small.*?\n\t--enable-hwaccels", "\t--disable-small \\\n\t--enable-hwaccels", mk_text, flags=re.S)
    mk_text = mk_text.replace("FFMPEG_CONF_OPTS += --disable-postproc\n", "")
    for line in ("FFMPEG_CONF_OPTS += --disable-omx\n", "FFMPEG_CONF_OPTS += --disable-omx-rpi\n"):
        mk_text = mk_text.replace(line, "")
    mk_text = mk_text.replace("FFMPEG_CONF_OPTS += --disable-mmal --disable-omx --disable-omx-rpi\n", "FFMPEG_CONF_OPTS += --disable-mmal\n")
    mk.write_text(mk_text)
    hash_text = hashes.read_text()
    if ARCHIVE not in hash_text:
        old = next(line for line in hash_text.splitlines() if "ffmpeg-6.1.5.tar.xz" in line)
        hashes.write_text(hash_text.replace(old, f"sha256  {ARCHIVE_SHA256}  {ARCHIVE}"))
    # Buildroot 2025.02 carries fixes for the 6.1 branch. FFmpeg 9 already
    # contains those fixes, and several target unrelated hardware paths that
    # are disabled in the Y2 audio-only configuration.
    for patch in package.glob("*.patch"):
        patch.rename(patch.with_name(patch.name + ".ffmpeg6"))


if __name__ == "__main__":
    import sys
    apply(Path(sys.argv[1]), Path(sys.argv[2]))
