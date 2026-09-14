"""Build current rescue programs with the production rootfs's pinned ABI.

BOOTIMG-only updates retain root/data and their libraries, but must not silently
reuse old rescue executables when their production sources change.
"""
import hashlib
import json
import tarfile

TOOLCHAIN = 'armv7-eabihf--glibc--stable-2024.05-1'
# Buildroot 2025.02.17 toolchain-external-bootlin.hash, same production toolchain.
TOOLCHAIN_SHA256 = '608263bc9dc3eadf0962ddb1165f1c2291001190f9927dee47d464e26374462c'


def rebuild(project, out, run):
    archive = project/'.cache/buildroot-dl/toolchain-external-bootlin'/(TOOLCHAIN+'.tar.xz')
    with archive.open('rb') as f:
        if hashlib.file_digest(f, 'sha256').hexdigest() != TOOLCHAIN_SHA256:
            raise ValueError('production rescue toolchain archive hash mismatch')
    directory = out/'rescue-toolchain'
    # Fresh extraction per candidate avoids trusting mutable historical outputs.
    directory.mkdir(exist_ok=True)
    with tarfile.open(archive) as t:
        t.extractall(directory, filter='data')
    compilers = list(directory.glob('*/bin/arm-*-linux-*-gcc'))
    if len(compilers) != 1:
        raise ValueError('unexpected Bootlin compiler layout')
    cc = str(compilers[0])
    sources = {
        'y2-platform-start': ('tools/production/platform-start.c', []),
        'y2-offline-charge': ('tools/production/offline-charge.c', []),
        'y2-usb-status': ('tools/production/usb-status.c', []),
        'y2-fbtest': ('tools/development/fbtest.c', []),
        'y2-abi-check': ('tools/development/abi-check.c', ['-pthread']),
    }
    built = {}
    for name, (source, extra) in sources.items():
        run([cc, '-Os', '-Wall', '-Wextra', '-Werror', '-mcpu=cortex-a7',
             '-mfpu=neon-vfpv4', '-mfloat-abi=hard', '-marm', *extra,
             str(project/source), '-o', str(out/name)], name+'-build.log')
        built[name] = {
            'source': source, 'source_sha256': hashlib.sha256((project/source).read_bytes()).hexdigest(),
            'binary_sha256': hashlib.sha256((out/name).read_bytes()).hexdigest(),
        }
    receipt = out/'userspace-source.json'
    data = json.loads(receipt.read_text())
    data['method'] = 'retain verified production libraries; rebuild current rescue programs; no ext4 writes or rebuild'
    data['rescue_toolchain_sha256'] = TOOLCHAIN_SHA256
    data['rebuilt_rescue_programs'] = built
    receipt.write_text(json.dumps(data, indent=2)+'\n')
