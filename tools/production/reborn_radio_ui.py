#!/usr/bin/env python3
"""Package the scan UI correction over the installed splash system. No device access."""
import argparse
import json
from pathlib import Path
import re
import shutil
import subprocess

from tools.production.reborn_splash import inventory, sha
from tools.production.layout import scatter_rows

PROJECT = Path(__file__).resolve().parents[2]
ROOT_SHA = 'd40e9337e3a1149d12fdd240a59918b0d2c48e4e6a9974ab5b470685250d900c'
BOOT_SHA = '8671de2900fd05cc80a9eb7a3541d5bbdab7e5dd96c18f73f44ca1b13beb19a5'
CHANGES = {'usr/bin/reborn', 'usr/bin/rebornctl', 'etc/y2linux/build-id',
           'etc/y2linux/versions.json', 'usr/lib/os-release'}


def root_only(scatter):
    chunks = scatter.split('- partition_index:')
    for i in range(1, len(chunks)):
        name = re.search(r'partition_name: (\S+)', chunks[i]).group(1)
        chunks[i] = re.sub(r'is_download: \S+', 'is_download: ' + ('true' if name == 'ANDROID' else 'false'), chunks[i])
        chunks[i] = re.sub(r'file_name: \S+', 'file_name: ' + ('Y2ROOT.img' if name == 'ANDROID' else 'NONE'), chunks[i])
    return '- partition_index:'.join(chunks)


def package(build, output):
    baseline = PROJECT / 'out/REBORN-SPLASH-01'
    baseline_build = PROJECT / 'out/reborn-splash-01-build'
    assert not output.exists() and output.is_relative_to(PROJECT / 'out'), 'fresh output under out required'
    for repo in [PROJECT, PROJECT.parent / 'Y2Reborn']:
        assert not subprocess.check_output(['git', '-C', str(repo), 'status', '--porcelain']).strip(), 'commit reviewed sources'
    assert sha(baseline / 'Y2ROOT.img') == ROOT_SHA
    assert sha(baseline / 'BOOTIMG.img') == sha(build / 'BOOTIMG.img') == BOOT_SHA
    before = inventory(baseline_build / 'buildroot/images/rootfs.tar')
    after = inventory(build / 'buildroot/images/rootfs.tar')
    changes = {p for p in before.keys() | after.keys() if before.get(p) != after.get(p)}
    assert changes == CHANGES, changes
    image = build / 'buildroot/images/rootfs.ext4'
    assert image.stat().st_size == 536870912
    check = subprocess.run(['e2fsck', '-fn', str(image)], capture_output=True, check=True)
    (build / 'radio-e2fsck.log').write_bytes(check.stdout + check.stderr)

    def raw(name):
        return subprocess.check_output(['debugfs', '-R', 'cat /' + name, str(image)], stderr=subprocess.DEVNULL)

    import hashlib
    for name in CHANGES:
        assert hashlib.sha256(raw(name)).hexdigest() == after[name][3], name
    assert raw('etc/y2linux/build-id').strip() == b'Y2LINUX-REBORN-RADIO-UI-01'
    versions = json.loads(raw('etc/y2linux/versions.json'))
    for field, repo in [('build_git_commit', PROJECT), ('reborn_source_commit', PROJECT.parent / 'Y2Reborn')]:
        assert versions[field] == subprocess.check_output(['git', '-C', str(repo), 'rev-parse', 'HEAD'], text=True).strip()

    def usage(path):
        header = subprocess.check_output(['dumpe2fs', '-h', str(path)], stderr=subprocess.DEVNULL).decode()
        assert re.search(r'Filesystem volume name:\s+Y2ROOT', header)
        assert '79324c69-6e75-4801-8000-000000000101' in header
        fields = dict(re.findall(r'^([^:\n]+):\s+(.*)$', header, re.M))
        return (int(fields['Block count']) - int(fields['Free blocks'])) * int(fields['Block size'])

    scatter = root_only((baseline / 'MT6582_preserve_data_scatter.txt').read_text())
    rows = scatter_rows(scatter)
    assert {r['partition_name'] for r in rows if r['is_download'] == 'true'} == {'ANDROID'}
    assert all(r['file_name'] == 'NONE' for r in rows if r['partition_name'] != 'ANDROID')
    original = scatter_rows((baseline / 'MT6582_preserve_data_scatter.txt').read_text())
    for old, new in zip(original, rows, strict=True):
        assert {k: v for k, v in old.items() if k not in ('file_name', 'is_download')} == {k: v for k, v in new.items() if k not in ('file_name', 'is_download')}
    output.mkdir(); (output / 'fallback').mkdir(); (output / 'metadata').mkdir()
    for source, target in [(image, output / 'Y2ROOT.img'), (baseline / 'Y2ROOT.img', output / 'fallback/Y2ROOT.img')]:
        subprocess.run(['cp', '--reflink=auto', '--sparse=always', str(source), str(target)], check=True)
    for directory in [output, output / 'fallback']:
        (directory / 'MT6582_reborn_root_only_scatter.txt').write_text(scatter)
    (output / 'metadata/versions.json').write_text(json.dumps(versions, indent=2) + '\n')
    shutil.copyfile(build / 'radio-e2fsck.log', output / 'metadata/e2fsck.log')
    (output / 'metadata/changed-paths.json').write_text(json.dumps(sorted(changes), indent=2) + '\n')
    manifest = {
        'schema': 'org.reborn.radio-ui-01/v1', 'status': 'HOST_VALIDATED_PHYSICAL_PENDING',
        'reborn_commit': versions['reborn_source_commit'], 'integration_commit': versions['build_git_commit'],
        'root': {'sha256': sha(output / 'Y2ROOT.img'), 'bytes': image.stat().st_size, 'used_bytes': usage(image),
                 'used_bytes_delta': usage(image) - usage(baseline / 'Y2ROOT.img')},
        'fallback': {'sha256': ROOT_SHA, 'file': 'fallback/Y2ROOT.img'},
        'bootimg_changed': False, 'required_installed_bootimg_sha256': BOOT_SHA,
        'data_policy': 'preserve Y2DATA; no formatting or data payload',
        'binary_sizes': {n: (build / 'buildroot/target/usr/bin' / n).stat().st_size for n in ['reborn', 'rebornctl']},
        'changed_paths': sorted(changes),
    }
    (output / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    (output / 'SHA256SUMS').write_text(''.join(sha(p) + '  ' + str(p.relative_to(output)) + '\n' for p in sorted(output.rglob('*')) if p.is_file()))
    print(json.dumps(manifest, indent=2))


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--build', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    args = p.parse_args()
    package(args.build.resolve(), args.output.resolve())
