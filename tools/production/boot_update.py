#!/usr/bin/env python3
"""Package a normal production BOOTIMG update retaining installed root/data.

Uses the same build.py, production config, DT, rescue builder and artifact checks.
No device access, diagnostic kernel, partition repair or root/data generation.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import shutil
import stat
import struct
import subprocess
import sys

PROJECT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(PROJECT))
from tools.production.layout import (digest, make_boot_scatter, require, TARGETS,
                                     CAPACITY, addressing_contract, make_readback_plan)


def stage_userspace(base, build):
    """Recover the hash-checked rescue binaries; do not rebuild or edit ext4."""
    from tools.validation.dev_artifacts import rescue_entries
    from tools.validation.formats import gunzip
    from tools.validation.bootimg import check as check_bootimg
    m = json.loads((base/'manifest.json').read_text())
    original = next(p for p in m['payloads'] if p['target_partition'] == 'BOOTIMG')
    require(digest(base/'BOOTIMG.img') == original['raw']['sha256'], 'base BOOTIMG hash')
    image = (base/'BOOTIMG.img').read_bytes()
    ksize, = struct.unpack_from('<I', image, 8)
    rsize, = struct.unpack_from('<I', image, 16)
    ro = 2048 + (ksize + 2047)//2048*2048
    kernel = image[2560:2048+ksize]
    ramdisk = image[ro+512:ro+rsize]
    check_bootimg(image, kernel, ramdisk, json.loads((base/'metadata/layout.json').read_text()))
    entries = rescue_entries(gunzip(ramdisk, 0x810000))
    recorded = json.loads((base/'metadata/rescue-manifest.json').read_text())
    require(set(entries) == set(recorded), 'base rescue inventory')
    target = build/'buildroot/target'
    target.mkdir(parents=True, exist_ok=True)
    for name, (mode, raw) in sorted(entries.items(), key=lambda item: item[0].count('/')):
        info = recorded[name]
        require(info['mode'] == mode and info['bytes'] == len(raw) and
                info['sha256'] == hashlib.sha256(raw).hexdigest(), 'base rescue entry '+name)
        path = target/name
        if stat.S_ISDIR(mode):
            path.mkdir(parents=True, exist_ok=True)
        elif stat.S_ISREG(mode):
            require(not path.is_symlink(), 'unexpected staged symlink')
            path.write_bytes(raw)
            path.chmod(stat.S_IMODE(mode))
        elif stat.S_ISLNK(mode):
            require(raw == b'busybox' and name.startswith('bin/'), 'base rescue symlink')
            if not path.is_symlink():path.symlink_to(raw.decode())
        # Device nodes belong to the initramfs archive, never the host filesystem.
    for name in ('y2-platform-start', 'y2-usb-status', 'y2-fbtest', 'y2-abi-check'):
        shutil.copyfile(target/'sbin'/name, build/name)
        (build/name).chmod(0o755)
    shutil.copyfile(base/'metadata/buildroot.config', build/'buildroot/.config')
    (build/'userspace-source.json').write_text(json.dumps({
        'base_manifest_sha256': digest(base/'manifest.json'),
        'base_build_git_commit': m['build_git_commit'],
        'rootfs_version': m['rootfs_version'],
        'retained_images': {p['target_partition']: p['raw'] for p in m['payloads']
                            if p['target_partition'] != 'BOOTIMG'},
        'method': 'reuse verified production rescue binaries; no ext4 writes or rebuild',
    }, indent=2)+'\n')
    print('PASS reused exact production userspace binaries; Y2ROOT/Y2DATA untouched')


def package(build, base, out, fallback_package=None):
    from tools.production.validate import validate_manifest, validate_boot_update
    require(out.is_relative_to(PROJECT/'out') and not out.exists(), 'fresh package inside out')
    require(not subprocess.check_output(['git', 'status', '--porcelain'], cwd=PROJECT).strip(),
            'commit reviewed source before packaging')
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=PROJECT, text=True).strip()
    versions = json.loads((build/'versions.json').read_text())
    require(versions['build_git_commit'] == head == (build/'kernel-source-commit').read_text().strip(),
            'kernel build source identity')
    previous = validate_manifest(base)
    fallback_package = fallback_package or base
    fallback_manifest = validate_manifest(fallback_package)
    for field in ('rootfs_version','layout_version','data_schema_version','minimum_compatible_components'):
        require(fallback_manifest[field] == previous[field], 'fallback installed compatibility '+field)
    fallback_boot = next(p for p in fallback_manifest['payloads'] if p['target_partition']=='BOOTIMG')
    receipt = json.loads((build/'userspace-source.json').read_text())
    require(receipt['base_manifest_sha256'] == digest(base/'manifest.json'), 'userspace base changed')
    require(versions['rootfs_version'] == previous['rootfs_version'] and
            versions['rootfs_build_git_commit'] == previous['build_git_commit'], 'retained root version')
    out.mkdir();(out/'metadata').mkdir();(out/'fallback').mkdir()
    shutil.copyfile(build/'BOOTIMG.img', out/'BOOTIMG.img')
    shutil.copyfile(fallback_package/'BOOTIMG.img', out/'fallback/BOOTIMG-previous.img')
    shutil.copyfile(fallback_package/'manifest.json', out/'metadata/fallback-manifest.json')
    for name in ('layout.json', 'rescue-manifest.json', 'versions.json', 'kernel-source-commit', 'userspace-source.json'):
        shutil.copyfile(build/name, out/'metadata'/name)
    for name in ('partitions.json', 'buildroot.config', 'kernel-inputs.lock.json',
                 'buildroot-inputs.lock.json', 'kernel-COPYING', 'Buildroot-COPYING'):
        shutil.copyfile(base/'metadata'/name, out/'metadata'/name)
    shutil.copyfile(build/'kernel/.config', out/'metadata/kernel.config')
    shutil.copyfile(base/'manifest.json', out/'metadata/base-manifest.json')
    shutil.copyfile(PROJECT/'docs/knowledge/storage06-addressing-correction.md', out/'metadata/mmc-correction.md')
    (out/'metadata/storage-addressing.json').write_text(json.dumps(addressing_contract(), indent=2)+'\n')
    classification = json.loads((out/'metadata/partitions.json').read_text())
    (out/'metadata/readback-plan.json').write_text(json.dumps(
        make_readback_plan(classification, PROJECT/'tests/fixtures/production'), indent=2)+'\n')
    stock = (PROJECT/'tests/fixtures/production/MT6582_Android_scatter.txt').read_text()
    scatter = 'MT6582_BOOTIMG_only_scatter.txt'
    (out/scatter).write_text(make_boot_scatter(stock))
    m = copy.deepcopy(previous)
    m.update(versions)
    m['storage_addressing'] = addressing_contract()
    m['hardware_compatibility']['accepted_linux_user_sector_counts'] = [CAPACITY//512]
    boot = next(p for p in m['payloads'] if p['target_partition'] == 'BOOTIMG')
    m['installed_components'] = [p for p in m['payloads'] if p['target_partition'] != 'BOOTIMG']
    m['payloads'] = [boot]
    boot['version'] = versions['kernel_version']
    boot['modules']['release'] = versions['kernel_version']
    boot['required_in_profiles'] = ['boot-only']
    boot['raw'] = {'file': 'BOOTIMG.img', 'size_bytes': (out/'BOOTIMG.img').stat().st_size,
                   'sha256': digest(out/'BOOTIMG.img')}
    boot['spft'] = {'format': 'raw-android-mtk-bootimg', **boot['raw']}
    m['installation_profile'] = 'boot-only'
    m['status'] = 'M4 integrated candidate; physical power/suspend qualification pending'
    m['base_manifest_sha256'] = digest(base/'manifest.json')
    m['installed_components_policy'] = 'reference identities only; preserve ANDROID/USRDATA; no root/data payload packaged'
    m['profiles'] = {scatter: {'sha256': digest(out/scatter), 'selected_partitions': ['BOOTIMG']}}
    m['fallback'] = {'file': 'fallback/BOOTIMG-previous.img',
                     'sha256': fallback_boot['raw']['sha256'],
                     'size_bytes': fallback_boot['raw']['size_bytes'],
                     'manifest_sha256': digest(out/'metadata/fallback-manifest.json'),
                     'kernel_version': fallback_manifest['kernel_version'],
                     'expected_behavior': 'restore the supplied previous BOOTIMG; root/data remain untouched'}
    (out/'manifest.json').write_text(json.dumps(m, indent=2)+'\n')
    (out/'install.md').write_text(
        '# Production BOOTIMG-only update\n\n'
        'Use Download Only and MT6582_BOOTIMG_only_scatter.txt. Select BOOTIMG → BOOTIMG.img only. '
        'Every other row, including PRELOADER, MBR, EBR1, EBR2, ANDROID and USRDATA, stays unchecked. '
        'Do not use Format or Firmware Upgrade. Kernel writes remain limited to the two existing root/data spans.\n\n'
        'Boot once without an SD card. Expected: stock-layout=1, offset=23552, disk=15203328, '
        'sector-zero signature55aa, mmcblk0p5/p7, internal ext4 root/data, '
        'switch_root to Buildroot and normal services. M4 power acceptance is pending. '
        'Existing rootfs, data and SSH authorization are preserved.\n\n'
        'Fallback: same scatter and BOOTIMG-only selection, choosing fallback/BOOTIMG-previous.img. '
        'Its exact previous kernel version and identity are recorded in manifest.json. '
        'See the canonical M4 qualification procedure before suspend/reboot/poweroff tests.\n')
    (out/'SHA256SUMS').write_text(''.join(digest(p)+'  '+str(p.relative_to(out))+'\n'
        for p in sorted(out.rglob('*')) if p.is_file() and p.name != 'SHA256SUMS'))
    validate_boot_update(out, base)
    print('PASS production BOOTIMG-only package:', out)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--stage-userspace', nargs=2, type=Path, metavar=('BASE', 'BUILD'))
    p.add_argument('--build', type=Path)
    p.add_argument('--base', type=Path)
    p.add_argument('--output', type=Path)
    p.add_argument('--fallback-package',type=Path,help='verified prior package containing the accepted BOOTIMG; defaults to BASE')
    a = p.parse_args()
    if a.stage_userspace:
        stage_userspace(*a.stage_userspace)
    else:
        if not all((a.build, a.base, a.output)):p.error('--build, --base and --output required')
        package(a.build.resolve(), a.base.resolve(), a.output.resolve(),
                a.fallback_package.resolve() if a.fallback_package else None)


if __name__ == '__main__':main()
