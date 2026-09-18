#!/usr/bin/env python3
"""Fail closed on the FFmpeg component manifest reported by Reborn."""
import argparse
import json
import re
from pathlib import Path

REQUIRED_DEMUXERS = {"flac", "mp3", "mov", "ogg", "wav", "aac", "aiff", "ape", "wv"}
REQUIRED_DECODERS = {
    "flac", "mp3", "mp3float", "aac", "alac", "vorbis", "opus", "ape", "wavpack",
    "pcm_s16le", "pcm_s16be", "pcm_s24le", "pcm_s24be", "pcm_s32le", "pcm_s32be",
    "pcm_f32le", "pcm_f32be",
}
REQUIRED_ARTWORK = {"mjpeg", "png", "webp"}
REQUIRED_PARSERS = {"flac", "mpegaudio", "aac", "opus", "vorbis"}
REQUIRED_FILTERS = {
    "abuffer", "abuffersink", "aformat", "aresample", "volume", "equalizer", "alimiter",
    "acrossfade", "amix", "atrim", "afade", "asetnsamples",
}
EXPECTED_LIBRARIES = {
    "libavcodec": "63.1.101", "libavformat": "63.1.101", "libavutil": "61.1.101",
    "libavfilter": "12.1.101", "libswresample": "7.1.101", "libswscale": "10.1.101",
}
EXPECTED_DEMUXERS = REQUIRED_DEMUXERS
EXPECTED_PARSERS = REQUIRED_PARSERS
EXPECTED_FILTERS = REQUIRED_FILTERS - {"abuffer", "abuffersink"}


def check(value: dict) -> list[str]:
    failures: list[str] = []
    if "9.0.1" not in value.get("version", ""):
        failures.append(f"runtime version is {value.get('version')!r}, expected FFmpeg 9.0.1")
    libraries = value.get("libraries", {})
    for name, version in EXPECTED_LIBRARIES.items():
        if libraries.get(name) != version:
            failures.append(f"{name} is {libraries.get(name)!r}, expected {version}")
    for field, required in {
        "demuxers": REQUIRED_DEMUXERS,
        "audio_decoders": REQUIRED_DECODERS,
        "artwork_decoders": REQUIRED_ARTWORK,
        "parsers": REQUIRED_PARSERS,
        "filters": REQUIRED_FILTERS,
    }.items():
        actual = set(value.get(field, []))
        missing = sorted(required - actual)
        if missing:
            failures.append(f"missing {field}: {', '.join(missing)}")
    if set(value.get("protocols", [])) != {"file"}:
        failures.append(f"protocol policy is {value.get('protocols')!r}, expected ['file']")
    if value.get("encoders"):
        failures.append(f"unexpected encoders: {value['encoders']!r}")
    if value.get("muxers"):
        failures.append(f"unexpected muxers: {value['muxers']!r}")
    configuration = value.get("configuration", "")
    for option in ("--disable-network", "--disable-avdevice", "--disable-programs"):
        if option not in configuration:
            failures.append(f"runtime configuration lacks {option}")
    return failures


def configured_components(path: Path, suffix: str) -> set[str]:
    pattern = re.compile(rf"^#define CONFIG_([A-Z0-9_]+)_{suffix} 1$")
    return {
        match.group(1).lower()
        for line in path.read_text().splitlines()
        if (match := pattern.match(line))
    }


def check_buildroot(output: Path) -> list[str]:
    """Verify the generated FFmpeg build itself before image packaging.

    This is intentionally independent of the Buildroot .config: it reads the
    FFmpeg 9.0.1 generated component header and the target rootfs libraries.
    The ARM runtime manifest is checked separately by the Reborn QEMU test.
    """
    failures: list[str] = []
    source = output / "build/ffmpeg-9.0.1"
    components = source / "config_components.h"
    config = source / "config.h"
    if not components.is_file() or not config.is_file():
        return [f"missing generated FFmpeg configuration under {source}"]
    target = output / "target"
    target_libs = {
        name: target / "usr/lib" / f"{name}.so.{version}"
        for name, version in EXPECTED_LIBRARIES.items()
    }
    for name, path in target_libs.items():
        if not path.is_file():
            failures.append(f"missing target {name} {path.name}")
    for name in ("ffmpeg", "ffplay", "ffprobe"):
        if (target / "usr/bin" / name).exists():
            failures.append(f"unexpected target CLI {name}")
    if list((target / "usr/lib").glob("libavdevice.so*")):
        failures.append("unexpected target libavdevice")
    try:
        config_text = config.read_text()
        if re.search(r"^#define CONFIG_(?:NETWORK|AVDEVICE|PROGRAMS) 1$", config_text, re.M):
            failures.append("network, avdevice, or programs enabled in generated config")
    except OSError as error:
        failures.append(f"cannot read generated FFmpeg config: {error}")

    actual = {
        "demuxers": configured_components(components, "DEMUXER"),
        "decoders": configured_components(components, "DECODER"),
        "parsers": configured_components(components, "PARSER"),
        "filters": configured_components(components, "FILTER"),
        "protocols": configured_components(components, "PROTOCOL"),
        "encoders": configured_components(components, "ENCODER"),
        "muxers": configured_components(components, "MUXER"),
    }
    for name, required in {
        "demuxers": EXPECTED_DEMUXERS,
        "parsers": EXPECTED_PARSERS,
        "filters": EXPECTED_FILTERS,
    }.items():
        missing = sorted(required - actual[name])
        if missing:
            failures.append(f"generated FFmpeg missing {name}: {', '.join(missing)}")
    allowed_decoders = REQUIRED_DECODERS | REQUIRED_ARTWORK | {"webp_anim"}
    missing_decoders = sorted((REQUIRED_DECODERS | REQUIRED_ARTWORK) - actual["decoders"])
    if missing_decoders:
        failures.append(f"generated FFmpeg missing decoders: {', '.join(missing_decoders)}")
    unexpected_decoders = sorted(actual["decoders"] - allowed_decoders)
    if unexpected_decoders:
        failures.append(f"unexpected generated decoders: {', '.join(unexpected_decoders)}")
    if actual["protocols"] != {"file"}:
        failures.append(f"unexpected generated protocols: {sorted(actual['protocols'])}")
    if actual["encoders"]:
        failures.append(f"unexpected generated encoders: {sorted(actual['encoders'])}")
    if actual["muxers"]:
        failures.append(f"unexpected generated muxers: {sorted(actual['muxers'])}")
    unexpected_demuxers = sorted(actual["demuxers"] - EXPECTED_DEMUXERS)
    if unexpected_demuxers:
        failures.append(f"unexpected generated demuxers: {', '.join(unexpected_demuxers)}")
    return failures


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("manifest", type=Path, nargs="?")
    parser.add_argument("--buildroot-output", type=Path)
    args = parser.parse_args()
    if args.buildroot_output:
        failures = check_buildroot(args.buildroot_output)
        if failures:
            raise SystemExit("FFmpeg generated build rejected:\n- " + "\n- ".join(failures))
        print(json.dumps({"passed": True, "source": str(args.buildroot_output)}, indent=2))
        return 0
    if not args.manifest:
        parser.error("provide a runtime manifest or --buildroot-output")
    value = json.loads(args.manifest.read_text())
    failures = check(value)
    if failures:
        raise SystemExit("FFmpeg production manifest rejected:\n- " + "\n- ".join(failures))
    print(json.dumps({"passed": True, "version": value["version"], "libraries": value["libraries"]}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
