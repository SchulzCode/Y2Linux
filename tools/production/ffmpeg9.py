#!/usr/bin/env python3
"""Pin the Buildroot FFmpeg package to the reviewed FFmpeg 9 source."""
import hashlib
import re
from pathlib import Path

VERSION = "9.0.2"
ARCHIVE = f"ffmpeg-{VERSION}.tar.xz"
ARCHIVE_SHA256 = "8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e"


def apply(buildroot_source: Path, download_dir: Path) -> None:
    package = buildroot_source / "package/ffmpeg"
    mk = package / "ffmpeg.mk"
    hashes = package / "ffmpeg.hash"
    archive = download_dir / "ffmpeg" / ARCHIVE
    if not archive.is_file():
        raise FileNotFoundError(f"missing locked FFmpeg archive: {archive}")
    if hashlib.sha256(archive.read_bytes()).hexdigest() != ARCHIVE_SHA256:
        raise ValueError(f"FFmpeg {VERSION} archive hash mismatch")
    mk_text = mk.read_text()
    mk_text = re.sub(r"FFMPEG_VERSION = [0-9.]+", f"FFMPEG_VERSION = {VERSION}", mk_text, count=1)
    mk_text = re.sub(r"\n\t(?:--disable-crystalhd|--disable-dxva2|--enable-runtime-cpudetect|--disable-hardcoded-tables|--disable-mipsdspr2|--disable-mipsdsp|--disable-msa|--disable-postproc) \\\n", "\n", mk_text)
    mk_text = re.sub(r"\t--disable-small.*?\n\t--enable-hwaccels", "\t--disable-small \\\n\t--enable-hwaccels", mk_text, flags=re.S)
    mk_text = mk_text.replace("FFMPEG_CONF_OPTS += --disable-postproc\n", "")
    for line in ("FFMPEG_CONF_OPTS += --disable-omx\n", "FFMPEG_CONF_OPTS += --disable-omx-rpi\n"):
        mk_text = mk_text.replace(line, "")
    mk_text = mk_text.replace("FFMPEG_CONF_OPTS += --disable-mmal --disable-omx --disable-omx-rpi\n", "FFMPEG_CONF_OPTS += --disable-mmal\n")
    mk.write_text(mk_text)
    hash_text = hashes.read_text()
    if ARCHIVE not in hash_text:
        old = next(line for line in hash_text.splitlines() if "ffmpeg-" in line and line.endswith(".tar.xz"))
        hashes.write_text(hash_text.replace(old, f"sha256  {ARCHIVE_SHA256}  {ARCHIVE}"))
    # Buildroot 2025.02 carries fixes for the 6.1 branch. FFmpeg 9 already
    # contains those fixes, and several target unrelated hardware paths that
    # are disabled in the Y2 audio-only configuration.
    for patch in package.glob("*.patch"):
        patch.rename(patch.with_name(patch.name + ".ffmpeg6"))


if __name__ == "__main__":
    import sys
    apply(Path(sys.argv[1]), Path(sys.argv[2]))
