#!/usr/bin/env python3
"""Pin the Buildroot FFmpeg package to the reviewed FFmpeg 9 source."""
import hashlib
import os
import re
from pathlib import Path
from urllib.request import urlopen

PROJECT = Path(__file__).resolve().parents[2]
def reborn_source() -> Path:
    return Path(os.environ.get("Y2_REBORN_SOURCE", PROJECT.parent / "Y2Reborn"))


REBORN = reborn_source()
VERSION = (REBORN / "FFMPEG_VERSION").read_text().strip()
ARCHIVE = f"ffmpeg-{VERSION}.tar.xz"
ARCHIVE_URL = f"https://ffmpeg.org/releases/{ARCHIVE}"
ARCHIVE_SHA256 = "8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e"
# LICENSE.md from that same hash-verified 9.0.2 archive, not the old 6.1 text.
LICENSE_SHA256 = "2e1d16c72fd74e12063776371da757322f8b77589386532f4fd8634bde7de1af"


def ensure_archive(download_dir: Path) -> Path:
    """Acquire the one pinned upstream archive and atomically retain its hash."""
    archive = download_dir / "ffmpeg" / ARCHIVE
    if archive.is_file():
        actual = hashlib.sha256(archive.read_bytes()).hexdigest()
        if actual != ARCHIVE_SHA256:
            raise ValueError(f"FFmpeg {VERSION} archive hash mismatch: {archive}")
        return archive

    archive.parent.mkdir(parents=True, exist_ok=True)
    temporary = archive.with_name(archive.name + f".part-{os.getpid()}")
    digest = hashlib.sha256()
    try:
        with urlopen(ARCHIVE_URL, timeout=120) as response, temporary.open("wb") as output:
            while chunk := response.read(1024 * 1024):
                digest.update(chunk)
                output.write(chunk)
            output.flush()
            os.fsync(output.fileno())
        if digest.hexdigest() != ARCHIVE_SHA256:
            raise ValueError(f"downloaded FFmpeg {VERSION} archive hash mismatch")
        os.replace(temporary, archive)
    finally:
        temporary.unlink(missing_ok=True)
    return archive


def apply(buildroot_source: Path, download_dir: Path) -> None:
    package = buildroot_source / "package/ffmpeg"
    mk = package / "ffmpeg.mk"
    hashes = package / "ffmpeg.hash"
    ensure_archive(download_dir)
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
        hash_text = hash_text.replace(old, f"sha256  {ARCHIVE_SHA256}  {ARCHIVE}")
    hash_text, count = re.subn(r'^sha256\s+[0-9a-f]{64}\s+LICENSE\.md$',
                              f'sha256  {LICENSE_SHA256}  LICENSE.md', hash_text, flags=re.M)
    if count != 1:
        raise ValueError('expected FFmpeg license hash missing')
    hashes.write_text(hash_text)
    # Buildroot 2025.02 carries fixes for the 6.1 branch. FFmpeg 9 already
    # contains those fixes, and several target unrelated hardware paths that
    # are disabled in the Y2 audio-only configuration.
    for patch in package.glob("*.patch"):
        patch.rename(patch.with_name(patch.name + ".ffmpeg6"))


if __name__ == "__main__":
    import sys
    apply(Path(sys.argv[1]), Path(sys.argv[2]))
