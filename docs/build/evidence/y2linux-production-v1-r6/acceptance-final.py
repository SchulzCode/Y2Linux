"""Final host checks: hardware invariance and BOOTIMG package rejection paths."""
import contextlib
import io
import json
from pathlib import Path
import shutil
import sys
import tempfile

PROJECT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(PROJECT))
from tools.build.check_config import parse
from tools.production.layout import digest
from tools.production.validate import validate_boot_update
from tools.validation.formats import fdt

base = PROJECT/'out/y2linux-production-v1-r4'
candidate = PROJECT/'out/y2linux-production-v1-r6'
build = Path(__file__).resolve().parent
old_layout = json.loads((base/'metadata/layout.json').read_text())
old_image = (base/'BOOTIMG.img').read_bytes()
dt_start = 2560 + old_layout['inputs']['z']
old_nodes, old_reserve = fdt(old_image[dt_start:dt_start+old_layout['inputs']['dtb']])
new_nodes, new_reserve = fdt((build/'y2.dtb').read_bytes())
assert old_nodes.keys() == new_nodes.keys() and old_reserve == new_reserve
differences = [(node, key) for node in old_nodes for key in old_nodes[node].keys() | new_nodes[node].keys()
               if old_nodes[node].get(key) != new_nodes[node].get(key)]
assert differences == [('/chosen', 'linux,initrd-end')], differences
print('PASS all 71 DT nodes and reserved regions identical except calculated initrd-end')
old_config = parse(base/'metadata/kernel.config')
new_config = parse(build/'kernel/.config')
changed = {k: (old_config.get(k), new_config.get(k)) for k in old_config.keys() | new_config.keys()
           if old_config.get(k) != new_config.get(k)}
assert set(changed) == {'CONFIG_LOCALVERSION'}, changed
print('PASS kernel configuration identical except LOCALVERSION storage04 -> storage06')

def update_json(path, change):
    document = json.loads(path.read_text())
    change(document)
    path.write_text(json.dumps(document, indent=2)+'\n')

def manifest_change(change):
    return lambda path: update_json(path/'manifest.json', change)

def protected_row(path):
    scatter = path/'MT6582_BOOTIMG_only_scatter.txt'
    text = scatter.read_text()
    assert 'partition_name: MBR\n  file_name: NONE\n  is_download: false' in text
    scatter.write_text(text.replace('partition_name: MBR\n  file_name: NONE\n  is_download: false',
                                  'partition_name: MBR\n  file_name: MBR.img\n  is_download: true'))
    update_json(path/'manifest.json', lambda m: m['profiles'][scatter.name].update(sha256=digest(scatter)))

def corrupt_boot(path):
    image = path/'BOOTIMG.img'
    data = bytearray(image.read_bytes())
    data[3000] ^= 1
    image.write_bytes(data)

cases = {
    'protected MBR selected with matching scatter hash': protected_row,
    'shifted ANDROID geometry': manifest_change(lambda m: m['installed_components'][0].update(absolute_start_bytes=85459456)),
    'wrong Y2DATA UUID': manifest_change(lambda m: m['installed_components'][1]['filesystem'].update(uuid='wrong')),
    'unexpected data payload': lambda path: (path/'Y2DATA.img').write_bytes(b'unrequested image'),
    'mismatched module release': manifest_change(lambda m: m['payloads'][0]['modules'].update(release='6.18.0-y2linux-storage04')),
    'corrupted BOOTIMG bytes': corrupt_boot,
    'changed runtime write allowlist': manifest_change(lambda m: m['runtime_kernel_write_allowlist'].append('NVRAM')),
    'wrong native offset': manifest_change(lambda m: m['storage_addressing'].update(logical_to_native_user_offset_sectors=40960)),
    'raw disk capacity accepted': manifest_change(lambda m: m['hardware_compatibility'].update(accepted_linux_user_sector_counts=[15203328,15269888])),
    'native tail exposed': manifest_change(lambda m: m['storage_addressing'].update(excluded_native_tail_sectors=0)),
    'missing stock window': manifest_change(lambda m: m.pop('storage_addressing')),
    'readback uses logical offset as physical': lambda path: update_json(path/'metadata/readback-plan.json',lambda p: p['preservation_ranges'][0].update(physical_start_bytes=0)),
    'changed root provenance': manifest_change(lambda m: m.update(rootfs_build_git_commit='0'*40)),
}
for label, change in cases.items():
    with tempfile.TemporaryDirectory(prefix='y2-package-reject-', dir='/tmp') as temporary:
        package = Path(temporary)/'candidate'
        shutil.copytree(candidate, package)
        change(package)
        # Refresh the generic inventory so semantic checks, not stale inventory hashes,
        # must detect each deliberate inconsistency.
        (package/'SHA256SUMS').write_text(''.join(digest(p)+'  '+str(p.relative_to(package))+'\n'
            for p in sorted(package.rglob('*')) if p.is_file() and p.name != 'SHA256SUMS'))
        try:
            with contextlib.redirect_stdout(io.StringIO()):
                validate_boot_update(package)
        except ValueError as error:
            print('PASS rejected '+label+': '+str(error))
        else:
            raise AssertionError('Accepted '+label)
print('PASS candidate preserved; mutations were isolated host-only copies')
