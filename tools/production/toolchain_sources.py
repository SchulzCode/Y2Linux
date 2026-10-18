#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""Corresponding source of the prebuilt Bootlin toolchain's target libraries.

The images carry glibc (LGPL-2.1+) and GCC's runtime libraries (libgcc_s, libstdc++,
libatomic: GPL-3.0 with the runtime exception) from the external Bootlin toolchain, whose
sources Buildroot's legal-info does not save ("not saved" in manifest.csv; V1 readiness
A7). This tool fetches exactly the archives the toolchain's own summary.csv names, checks
them against the hashes Buildroot pins for the same versions, and packs them with that
summary into one reproducible archive, kept in .cache/downloads/toolchain-sources/.
`release.py build` attaches it to each release; `publish` links an identical earlier one.

    python3 tools/production/toolchain_sources.py        # fetch (once) and pack
"""
import csv
import hashlib
import io
import json
import sys
import tarfile
import urllib.request
from pathlib import Path

PROJECT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(PROJECT))
from tools.production.release import deterministic_tar  # noqa: E402

BUILDROOT = PROJECT / '.cache/sources/buildroot-2026.08'
TOOLCHAIN = PROJECT / '.cache/buildroot-dl/toolchain-external-bootlin/armv7-eabihf--glibc--stable-2026.08-1.tar.xz'
CACHE = PROJECT / '.cache/downloads/toolchain-sources'
# Archive name -> (Buildroot hash file, mirrors). sources.buildroot.net keeps every
# archive Buildroot pins, including the git snapshots it generates (glibc "-git4").
SOURCES = {
    'glibc': ('package/glibc/glibc.hash', ['https://sources.buildroot.net/glibc/'],
              'the C library: LGPL-2.1-or-later (programs GPL-2.0-or-later)'),
    'gcc-final': ('package/gcc/gcc.hash', ['https://mirrors.kernel.org/gnu/gcc/gcc-{version}/',
                                           'https://sources.buildroot.net/gcc/'],
                  'libgcc_s, libstdc++, libatomic: GPL-3.0 with the GCC Runtime Library Exception'),
}


def summary():
    """The toolchain's summary.csv (bytes) and its rows by package name."""
    with tarfile.open(TOOLCHAIN) as archive:
        member = next(m for m in archive.getmembers() if m.name.endswith('/summary.csv'))
        data = archive.extractfile(member).read()
    rows = {}
    for row in csv.DictReader(io.StringIO(data.decode())):
        rows.setdefault(row['PACKAGE'], row)
    return data, rows


def pinned(hash_file, name):
    """(algorithm, hex digest) Buildroot records for `name`."""
    for line in (BUILDROOT / hash_file).read_text().splitlines():
        fields = line.split()
        if len(fields) == 3 and fields[2] == name and fields[0] in ('sha256', 'sha512'):
            return fields[0], fields[1]
    raise SystemExit(f'{hash_file}: no sha256/sha512 for {name}')


def fetch(name, version, hash_file, mirrors):
    algorithm, expected = pinned(hash_file, name)
    target = CACHE / name
    if not target.exists():
        CACHE.mkdir(parents=True, exist_ok=True)
        for mirror in mirrors:
            url = mirror.format(version=version) + name
            try:
                with urllib.request.urlopen(url, timeout=60) as response:
                    data = response.read()
            except OSError as error:
                print(f'{url}: {error}', file=sys.stderr)
                continue
            if hashlib.new(algorithm, data).hexdigest() == expected:
                partial = target.with_suffix('.part')
                partial.write_bytes(data)
                partial.rename(target)
                break
            print(f'{url}: {algorithm} mismatch', file=sys.stderr)
        else:
            raise SystemExit(f'{name}: no mirror delivered the pinned archive')
    if hashlib.new(algorithm, target.read_bytes()).hexdigest() != expected:
        raise SystemExit(f'{target}: does not match the {algorithm} Buildroot pins')
    return target


def archive():
    """Path of Y2Linux-sources-toolchain-<hash>.tar, packing it when missing."""
    data, rows = summary()
    root = CACHE / 'toolchain-sources'
    files = []
    for package, (hash_file, mirrors, _) in SOURCES.items():
        row = rows[package]
        files.append(fetch(row['SOURCE ARCHIVE'], row['VERSION'], hash_file, mirrors))
    if root.exists():
        for path in root.iterdir():
            path.unlink()
    root.mkdir(parents=True, exist_ok=True)
    for path in files:
        (root / path.name).write_bytes(path.read_bytes())
    (root / 'summary.csv').write_bytes(data)
    (root / 'README.txt').write_text(
        'Corresponding source of the target libraries Y2Linux images carry from the prebuilt\n'
        f'Bootlin toolchain {TOOLCHAIN.name.removesuffix(".tar.xz")} (https://toolchains.bootlin.com):\n'
        + ''.join(f'  {path.name}\n    {package} {rows[package]["VERSION"]}; {SOURCES[package][2]}\n'
                  for package, path in zip(SOURCES, files))
        + 'summary.csv is the toolchain\'s own list of every component, version, licence and source.\n'
          'The archives are byte-identical to the ones Buildroot pins (package/glibc/glibc.hash,\n'
          'package/gcc/gcc.hash in Buildroot 2026.08).\n')
    epoch = int(json.loads((PROJECT / 'buildroot/inputs.lock.json').read_text())['source_date_epoch'])
    packed = CACHE / 'packed.tar'
    deterministic_tar(packed, root, sorted(path.name for path in root.iterdir()), epoch)
    digest = hashlib.sha256(packed.read_bytes()).hexdigest()
    final = CACHE / f'Y2Linux-sources-toolchain-{digest[:12]}.tar'
    for old in CACHE.glob('Y2Linux-sources-toolchain-*.tar'):
        if old != final:
            old.unlink()
    packed.rename(final)
    return final


if __name__ == '__main__':
    print(archive())
