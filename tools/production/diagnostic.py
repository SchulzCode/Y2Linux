#!/usr/bin/env python3
"""Build a BOOTIMG-only observation package from the exact Storage02 kernel."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

PROJECT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(PROJECT))
from tools.production.layout import digest, require, make_scatter, scatter_rows, TARGETS

BASE = 'y2linux-production-build-v1-r2'
BASE_PACKAGE = 'y2linux-production-v1-r2'
BUILD = 'y2linux-storage-diag-01-build'
OUTPUT = 'y2linux-storage-diag-01'
PINNED_MANIFEST = '7b6526c30b3d84940510b60e1472306c0c29115a95887f7569de03d95f26ea61'


def inside(commit):
    from tools.build.dev_initramfs import encode
    from tools.validation.dev_artifacts import check, rescue_entries
    from tools.validation.formats import gunzip, fdt
    from tools.validation.dev_memory import check_layout
    from tools.validation.d08 import Inputs
    from tools.validation.bootimg import check as check_bootimg
    from tools.build.package import pack
    import gzip
    import stat

    base = PROJECT/'out'/BASE
    original = PROJECT/'out'/BASE_PACKAGE
    require(digest(original/'manifest.json') == PINNED_MANIFEST, 'exact Storage02 manifest')
    manifest = json.loads((original/'manifest.json').read_text())
    boot = next(p for p in manifest['payloads'] if p['target_partition'] == 'BOOTIMG')
    require(digest(original/'BOOTIMG.img') == boot['raw']['sha256'], 'base BOOTIMG identity')
    layout, payload, rd = check(base, PROJECT, production=True)
    check_bootimg((original/'BOOTIMG.img').read_bytes(), payload, rd, layout)
    build = Path('/build')
    out = build/'package'
    out.mkdir(exist_ok=True)
    (out/'metadata').mkdir(exist_ok=True)
    entries = {}
    for name, (mode, data) in rescue_entries(gunzip(rd, 0x810000)).items():
        major, minor = (5, 1) if name == 'dev/console' else (1, 3) if name == 'dev/null' else (0, 0)
        entries[name] = (mode, data, major, minor)
    additions = {
        'init': PROJECT/'initramfs/production/diagnostic-init',
        'sbin/y2-diagnostic-pages': PROJECT/'initramfs/production/diagnostic-pages.sh',
        'sbin/y2-usb-status': build/'y2-usb-status',
    }
    for name, source in additions.items():
        entries[name] = (stat.S_IFREG | 0o755, source.read_bytes(), 0, 0)
    # Keep the exact base observer, blkid, libc, module, and production resolver.
    for name in ('sbin/y2-observer', 'sbin/blkid', 'display.ko', 'sbin/y2-storage'):
        require(entries[name][1] == rescue_entries(gunzip(rd, 0x810000))[name][1], 'base rescue identity '+name)
    (build/'diagnostic.cpio').write_bytes(encode(entries))
    ramdisk = gzip.compress(encode(entries), compresslevel=9, mtime=0)
    (build/'diagnostic.cpio.gz').write_bytes(ramdisk)
    decoded = rescue_entries(gunzip(ramdisk, 0x810000))
    require({n: (v[0], v[1]) for n, v in entries.items()} == decoded, 'newc roundtrip')
    regular = sum(len(v[1]) for v in entries.values() if stat.S_ISREG(v[0]))

    dtc = str(base/'kernel/scripts/dtc/dtc')
    subprocess.run([dtc, '-I', 'dtb', '-O', 'dts', '-o', str(build/'diagnostic.dts'), str(base/'y2.dtb')], check=True)
    source = (build/'diagnostic.dts').read_text()
    old_nodes, old_reserved = fdt((base/'y2.dtb').read_bytes())
    old_args = old_nodes['/chosen']['bootargs'].decode().rstrip('\0')
    args = old_args.replace('loglevel=8 ignore_loglevel', 'loglevel=3')
    require(args != old_args and 'ignore_loglevel' not in args, 'quiet console only; kernel log retained')
    require(source.count('bootargs = "'+old_args+'";') == 1, 'unique DT command line')
    source = source.replace('bootargs = "'+old_args+'";', 'bootargs = "'+args+'";')
    source, count = re.subn(r'linux,initrd-end = <0x[0-9a-f]+>;',
        f'linux,initrd-end = <0x{0x84000000+len(ramdisk):x}>;', source)
    require(count == 1, 'unique initrd end')
    (build/'diagnostic.dts').write_text(source)
    subprocess.run([dtc, '-I', 'dts', '-O', 'dtb', '-o', str(build/'diagnostic.dtb'), str(build/'diagnostic.dts')], check=True)
    tree = (build/'diagnostic.dtb').read_bytes()
    nodes, reserved = fdt(tree)
    import struct
    require(nodes['/chosen']['linux,initrd-end'] == struct.pack('>I', 0x84000000+len(ramdisk)), 'new initrd extent')
    require(nodes['/chosen']['bootargs'] == args.encode()+b'\0', 'new command line')
    for field in ('bootargs', 'linux,initrd-end'):
        nodes['/chosen'][field] = old_nodes['/chosen'][field]
    require(nodes == old_nodes and reserved == old_reserved, 'DT changed beyond logging/initrd extent')
    inputs = dict(layout['inputs'], dtb=len(tree), initramfs=len(ramdisk), regular_file_bytes=regular)
    result = check_layout(Inputs(**inputs))
    zimage = (base/'kernel/arch/arm/boot/zImage').read_bytes()
    require(hashlib.sha256(zimage).hexdigest() == layout['artifacts']['zImage']['sha256'], 'unchanged kernel')
    appended = zimage + tree + bytes(-len(tree) % 8)
    image = pack(appended, ramdisk)
    result['bootimg'] = check_bootimg(image, appended, ramdisk, result)
    (out/'BOOTIMG.img').write_bytes(image)
    (out/'metadata/layout.json').write_text(json.dumps(result, indent=2)+'\n')
    (out/'metadata/rescue-manifest.json').write_text(json.dumps({n: {'mode':v[0], 'size':len(v[1]),
        'sha256':hashlib.sha256(v[1]).hexdigest()} for n,v in entries.items()}, indent=2)+'\n')
    stock = (PROJECT/'tests/fixtures/production/MT6582_Android_scatter.txt').read_text()
    chunks = re.split(r'(?m)(?=^- partition_index: )', make_scatter(stock, False))
    for i in range(1, len(chunks)):
        if 'partition_name: BOOTIMG\n' in chunks[i]: continue
        chunks[i] = re.sub(r'(?m)^  file_name: .*$', '  file_name: NONE', chunks[i])
        chunks[i] = re.sub(r'(?m)^  is_download: .*$', '  is_download: false', chunks[i])
    scatter = ''.join(chunks)
    original_rows = scatter_rows(stock)
    rows = scatter_rows(scatter)
    for before, after in zip(original_rows, rows):
        require({k:v for k,v in before.items() if k not in ('file_name','is_download')} ==
                {k:v for k,v in after.items() if k not in ('file_name','is_download')}, 'stock scatter geometry')
        selected = after['partition_name'] == 'BOOTIMG'
        require(after['file_name'] == ('BOOTIMG.img' if selected else 'NONE') and
                after['is_download'] == str(selected).lower(), 'BOOTIMG-only mapping')
    (out/'MT6582_Android_scatter.txt').write_text(scatter)
    target = TARGETS['BOOTIMG']
    report = {'schema':'org.schulzcode.y2linux.rescue-diagnostic/v1', 'diagnostic_version':'storage02-diag01',
        'build_git_commit':commit, 'kernel_build_git_commit':manifest['build_git_commit'],
        'kernel_version':manifest['kernel_version'], 'layout_version':1,
        'hardware_compatibility':manifest['hardware_compatibility'],
        'base_manifest_sha256':PINNED_MANIFEST, 'restore_bootimg':boot['raw'],
        'unchanged_kernel_sha256':hashlib.sha256(zimage).hexdigest(),
        'unchanged_display_module_sha256':hashlib.sha256(entries['display.ko'][1]).hexdigest(),
        'status':'offline validated diagnostic; physical test pending',
        'runtime':'rescue only; no block mounts, fsck, switch_root or data writes',
        'preserved_partitions':'all except owner-selected BOOTIMG; stock table unchanged',
        'payload':{'file':'BOOTIMG.img','target_partition':'BOOTIMG','region':'EMMC_USER',
            'absolute_start_bytes':target['start'],'scatter_linear_start_bytes':target['linear'],
            'partition_relative_offset_bytes':0,'maximum_size_bytes':target['size'],
            'size_bytes':len(image),'sha256':digest(out/'BOOTIMG.img')},
        'scatter_sha256':digest(out/'MT6582_Android_scatter.txt')}
    (out/'manifest.json').write_text(json.dumps(report, indent=2)+'\n')
    shutil.copyfile(PROJECT/'docs/build/storage-diagnostic-install.md', out/'install.md')
    (out/'SHA256SUMS').write_text(''.join(digest(p)+'  '+str(p.relative_to(out))+'\n'
        for p in sorted(out.rglob('*')) if p.is_file() and p.name != 'SHA256SUMS'))
    print('PASS exact base kernel/module; changed rescue + two DT fields only')
    print('PASS newc, DT equality, D08 memory, BOOTIMG bounds/hash and BOOTIMG-only scatter')
    print(json.dumps(report, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--inside', action='store_true')
    parser.add_argument('--source-commit')
    args = parser.parse_args()
    if args.inside:
        require(bool(re.fullmatch('[0-9a-f]{40}', args.source_commit or '')), 'source commit')
        inside(args.source_commit)
        return
    require(not subprocess.check_output(['git','status','--porcelain'], cwd=PROJECT).strip(), 'commit reviewed source first')
    commit = subprocess.check_output(['git','rev-parse','HEAD'], cwd=PROJECT, text=True).strip()
    output = PROJECT/'out'/OUTPUT
    require(not output.exists(), 'fresh final package required')
    build = PROJECT/'out'/BUILD
    build.mkdir(exist_ok=True)
    stamp = build/'source-commit'
    require(not stamp.exists() or stamp.read_text().strip() == commit, 'staged source commit changed')
    stamp.write_text(commit+'\n')
    cc = PROJECT/'out'/BASE/'buildroot/host/bin/arm-linux-gcc'
    subprocess.run([str(cc), '-Os','-Wall','-Wextra','-Werror', str(PROJECT/'tools/production/usb-status.c'),
        '-o',str(build/'y2-usb-status')], check=True)
    runner = ['python3','tools/build/run.py','--output','out/'+BUILD,'--']
    with (build/'validation.log').open('w') as log:
        subprocess.run(runner+['python3','/project/tools/production/diagnostic.py','--inside',
            '--source-commit',commit], cwd=PROJECT, stdout=log, stderr=subprocess.STDOUT, check=True)
    with (build/'tests.log').open('w') as log:
        subprocess.run(runner+['sh','/project/tools/production/diagnostic-tests.sh'], cwd=PROJECT,
            stdout=log, stderr=subprocess.STDOUT, check=True)
    shutil.move(str(build/'package'), output)
    print('Validated diagnostic package:',output)


if __name__ == '__main__': main()
