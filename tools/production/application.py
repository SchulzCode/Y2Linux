"""Canonical manifest identity for the built-in Reborn application."""
import re
import subprocess
from pathlib import Path


def source_version(cargo_manifest):
    workspace = re.search(
        r"(?ms)^\[workspace\.package\]\s*(.*?)(?=^\[|\Z)", cargo_manifest
    )
    if workspace is None:
        raise ValueError("Y2Reborn workspace package version is missing")
    version = re.search(r'^version\s*=\s*"([^"]+)"', workspace.group(1), re.M)
    if version is None:
        raise ValueError("Y2Reborn workspace package version is missing")
    return version.group(1)


def source_version_at_commit(repository, commit):
    """Resolve the application version from the exact recorded source commit."""
    if not re.fullmatch(r"[0-9a-f]{40}", commit or ""):
        raise ValueError("Reborn source commit is missing or malformed")
    manifest = subprocess.check_output(
        ["git", "show", f"{commit}:Cargo.toml"], cwd=Path(repository), text=True
    )
    return source_version(manifest)


def receipt(version):
    return {
        "name": "Reborn",
        "version": version,
        "binary": "/usr/bin/reborn",
        "control_binary": "/usr/bin/rebornctl",
        "media_library": "/usr/lib/reborn/libreborn_media.so",
        "state_root": "/data/reborn",
        "partition": None,
    }


def validate(manifest):
    """Validate current receipts; return false for explicitly historical ones."""
    if "reborn_version" not in manifest:
        return False
    version = manifest["reborn_version"]
    if version is None and manifest.get("installation_profile") != "boot-only":
        raise ValueError("current rootfs must identify its built Reborn version")
    if manifest.get("application") != receipt(version):
        raise ValueError("application receipt does not identify the built-in Reborn files")
    return True
