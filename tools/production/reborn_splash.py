#!/usr/bin/env python3
"""Validate/package the targeted splash update; no device access or flashing."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import stat
import subprocess
import tarfile

PROJECT = Path(__file__).resolve().parents[2]
ROOT_HASH = 'c353152dea44a554486e024a4d8a8105fbbdfd1310db5749b28d7e5b8e5c8012'
BOOT_HASH = '2e5f7e785e80dfc646e57d0ccfadff683d54a2c5303671c819ea4e42863089a9'
ROOT_CHANGES = {
    'etc/default/syslogd', 'etc/init.d/S25y2-display', 'etc/inittab',
    'etc/y2linux/build-id', 'etc/y2linux/versions.json', 'usr/bin/reborn',
    'usr/bin/rebornctl', 'usr/lib/os-release', 'usr/libexec/reborn-boot-services',
    'usr/libexec/reborn-splash',
}


def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as f:
        for b in iter(lambda: f.read(1048576), b''): h.update(b)
    return h.hexdigest()


def check_boot():
    # Invoked inside the existing network/device-isolated build environment.
    from tools.validation.dev_artifacts import rescue_entries
    from tools.validation.formats import gunzip, fdt
    from tools.validation.bootimg import check
    build = Path('/build')
    base = PROJECT / 'out/reborn-baseline-01-build'
    old = rescue_entries(gunzip((base / 'initramfs.cpio.gz').read_bytes(), 0x810000))
    new = rescue_entries(gunzip((build / 'initramfs.cpio.gz').read_bytes(), 0x810000))
    assert not (old.keys() - new.keys())
    changed = {n for n in old if old[n] != new[n]}
    added = new.keys() - old.keys()
    assert changed == {'init'}, changed
    assert added <= {'sbin/reborn-splash', 'usr/lib/libdrm.so.2', 'usr', 'usr/lib'}, added
    assert new['init'][1] == (PROJECT / 'initramfs/production/init').read_bytes()
    assert new['sbin/reborn-splash'][1] == (build / 'buildroot/target/usr/libexec/reborn-splash').read_bytes()
    for name in ['display.ko', 'sbin/y2-offline-charge', 'sbin/y2-platform-start', 'sbin/y2-storage']:
        assert old[name] == new[name], name
    z = 'kernel/arch/arm/boot/zImage'
    assert (base / z).read_bytes() == (build / z).read_bytes()
    before, reserve_before = fdt((base / 'y2.dtb').read_bytes())
    after, reserve_after = fdt((build / 'y2.dtb').read_bytes())
    after['/chosen']['linux,initrd-end'] = before['/chosen']['linux,initrd-end']
    assert after == before and reserve_after == reserve_before, 'unexpected DT/platform change'
    layout = json.loads((build / 'layout.json').read_text())
    check((build / 'BOOTIMG.img').read_bytes(), (build / 'zImage-dtb').read_bytes(),
          (build / 'initramfs.cpio.gz').read_bytes(), layout)
    root = build / 'splash-initramfs-runtime'
    root.mkdir(exist_ok=True)
    for name, (mode, raw) in new.items():
        path = root / name
        if stat.S_ISDIR(mode): path.mkdir(parents=True, exist_ok=True)
        elif stat.S_ISREG(mode):
            path.parent.mkdir(parents=True, exist_ok=True); path.write_bytes(raw); path.chmod(stat.S_IMODE(mode))
    q = ['qemu-arm', '-cpu', 'cortex-a7', '-L', str(root), str(root / 'sbin/reborn-splash')]
    assert subprocess.run(q + ['--quiet-if-normal'], timeout=10).returncode == 0
    assert subprocess.run(q + ['--test'], timeout=10).returncode == 2
    target = build / 'buildroot/target'
    for script in ['usr/libexec/reborn-boot-services', 'etc/init.d/S25y2-display']:
        subprocess.run(['qemu-arm', '-L', str(target), str(target / 'bin/busybox'),
                        'sh', '-n', str(target / script)], check=True)
    subprocess.run(['qemu-arm', '-L', str(target), str(target / 'bin/busybox'),
                    'sh', '-n', str(root / 'init')], check=True)
    receipt = {
        'passed': True, 'physical_test': False, 'kernel_unchanged_sha256': sha(build / z),
        'display_module_unchanged_sha256': hashlib.sha256(new['display.ko'][1]).hexdigest(),
        'offline_charger_unchanged_sha256': hashlib.sha256(new['sbin/y2-offline-charge'][1]).hexdigest(),
        'initramfs_changed_entries': sorted(changed), 'initramfs_added_entries': sorted(added),
        'unchanged_entry_count': len(old) - len(changed),
        'dt_changed_fields': ['/chosen/linux,initrd-end'],
        'bootimg': layout['bootimg'], 'initramfs_inputs': layout['inputs'],
        'arm_splash_and_shell_checks': 5,
    }
    (build / 'splash-boot-preservation.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(receipt, indent=2))


def inventory(path):
    with tarfile.open(path) as t:
        return {m.name.removeprefix('./'): (m.type.decode(), m.mode, m.linkname,
                hashlib.sha256(t.extractfile(m).read()).hexdigest() if m.isfile() else '') for m in t}


def package(build, output):
    base = PROJECT / 'out/REBORN-BASELINE-01'
    base_build = PROJECT / 'out/reborn-baseline-01-build'
    oldboot = PROJECT / 'out/y2linux-gpu-02/BOOTIMG.img'
    assert sha(base / 'Y2ROOT.img') == ROOT_HASH and sha(oldboot) == BOOT_HASH
    assert not output.exists(), 'fresh output directory required'
    boot = json.loads((build / 'splash-boot-preservation.json').read_text())
    assert boot['passed'] and boot['bootimg']['sha256'] == sha(build / 'BOOTIMG.img')
    before = inventory(base_build / 'buildroot/images/rootfs.tar')
    after = inventory(build / 'buildroot/images/rootfs.tar')
    changed = {n for n in before.keys() | after.keys() if before.get(n) != after.get(n)}
    assert changed == ROOT_CHANGES, changed
    assert not any(n.startswith(('data/reborn/', 'data/network/', 'data/bluetooth/',
                                 'root/.ssh/', 'etc/dropbear/')) for n in after)
    image = build / 'buildroot/images/rootfs.ext4'
    assert image.stat().st_size == 536870912
    fscheck = subprocess.run(['e2fsck', '-fn', str(image)], check=True, capture_output=True)
    (build / 'splash-e2fsck.txt').write_bytes(fscheck.stdout + fscheck.stderr)
    def raw(name):
        return subprocess.check_output(['debugfs', '-R', 'cat /' + name, str(image)], stderr=subprocess.DEVNULL)
    for name in ROOT_CHANGES:
        assert hashlib.sha256(raw(name)).hexdigest() == after[name][3], name
    assert raw('etc/y2linux/build-id').strip() == b'Y2LINUX-REBORN-SPLASH-01'
    def usage(path):
        h = subprocess.check_output(['dumpe2fs', '-h', str(path)], stderr=subprocess.DEVNULL).decode()
        assert '79324c69-6e75-4801-8000-000000000101' in h
        assert re.search(r'Filesystem volume name:\s+Y2ROOT', h)
        fields = dict(re.findall(r'^([^:\n]+):\s+(.*)$', h, re.M))
        return (int(fields['Block count']) - int(fields['Free blocks'])) * int(fields['Block size'])
    output.mkdir(parents=True); (output / 'fallback').mkdir(); (output / 'metadata').mkdir()
    for src, name in [(image, 'Y2ROOT.img'), (build / 'BOOTIMG.img', 'BOOTIMG.img'),
                      (base / 'Y2ROOT.img', 'fallback/Y2ROOT.img'), (oldboot, 'fallback/BOOTIMG.img')]:
        subprocess.run(['cp', '--reflink=auto', '--sparse=always', str(src), str(output / name)], check=True)
    scatter = (PROJECT / 'out/y2linux-gpu-02/MT6582_preserve_data_scatter.txt').read_text()
    from tools.production.layout import scatter_rows
    rows = scatter_rows(scatter)
    assert {r['partition_name'] for r in rows if r['is_download'] == 'true'} == {'BOOTIMG', 'ANDROID'}
    for r in rows:
        if r['partition_name'] not in ('BOOTIMG', 'ANDROID'): assert r['file_name'] == 'NONE'
    for directory in [output, output / 'fallback']:
        (directory / 'MT6582_preserve_data_scatter.txt').write_text(scatter)
    for name in ['versions.json', 'splash-boot-preservation.json', 'splash-root-changes.json']:
        shutil.copyfile(build / name, output / 'metadata' / name)
    versions = json.loads(raw('etc/y2linux/versions.json'))
    manifest = {
        'schema': 'org.reborn.splash-01/v1', 'status': 'HOST_VALIDATED_PHYSICAL_PENDING',
        'reborn_commit': versions['reborn_source_commit'], 'integration_commit': versions['build_git_commit'],
        'kernel_source_commit': versions['kernel_source_commit'], 'bootimg_changed': True,
        'kernel_unchanged': True, 'offline_charging_binary_unchanged': True,
        'rootfs_changed_paths': sorted(changed), 'data_policy': 'preserve Y2DATA; no format or payload',
        'payloads': {n: {'bytes': (output / n).stat().st_size, 'sha256': sha(output / n)}
                     for n in ['BOOTIMG.img', 'Y2ROOT.img', 'fallback/BOOTIMG.img', 'fallback/Y2ROOT.img']},
        'root_used_bytes': usage(image), 'root_used_bytes_delta': usage(image) - usage(base / 'Y2ROOT.img'),
        'splash_binary_bytes': (build / 'buildroot/target/usr/libexec/reborn-splash').stat().st_size,
    }
    (output / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    (output / 'SHA256SUMS').write_text(''.join(sha(f) + '  ' + str(f.relative_to(output)) + '\n'
                                           for f in sorted(output.rglob('*')) if f.is_file()))
    print(json.dumps(manifest, indent=2))


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--check-boot', action='store_true')
    p.add_argument('--build', type=Path); p.add_argument('--output', type=Path)
    a = p.parse_args()
    if a.check_boot: check_boot()
    else:
        if not a.build or not a.output: p.error('--build and --output required')
        package(a.build.resolve(), a.output.resolve())


if __name__ == '__main__': main()
