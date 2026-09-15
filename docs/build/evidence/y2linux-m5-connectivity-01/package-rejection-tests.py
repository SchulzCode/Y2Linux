#!/usr/bin/env python3
"""Exercise preserving-package rejection without touching original images/devices."""
import copy
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile

PROJECT = next(p for p in Path(__file__).resolve().parents
               if (p/'tools/production').is_dir() and (p/'AGENTS.md').is_file())
sys.path.insert(0, str(PROJECT))
from tools.production.validate import validate_manifest

package = Path(sys.argv[1]).resolve()
original = json.loads((package / 'manifest.json').read_text())
def digest(path):
    with path.open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()

images = {str(p.relative_to(package)): digest(p) for p in package.rglob('*.img')}

def payload(m, name):
    return next(p for p in m['payloads'] if p['target_partition'] == name)

def set_scatter(p, m, old, new, fallback=False):
    name = ('fallback/' if fallback else '') + 'MT6582_preserve_data_scatter.txt'
    path = p / name
    text = path.read_text()
    assert old in text
    path.write_text(text.replace(old, new, 1))
    if not fallback:
        m['profiles'][name]['sha256'] = digest(path)

def corrupt_fallback(p, m):
    path = p / 'fallback/BOOTIMG.img'
    path.unlink()
    shutil.copyfile(package / 'fallback/BOOTIMG.img', path)
    with path.open('r+b') as f:
        f.write(b'INVALID!')

cases = [
    ('additional data payload', lambda p,m: m['payloads'].append(copy.deepcopy(m['installed_components'][1]))),
    ('unexpected data image', lambda p,m: (p/'Y2DATA.img').write_bytes(b'not a permitted image')),
    ('changed retained data identity', lambda p,m: m['installed_components'][1]['raw'].__setitem__('sha256', '0'*64)),
    ('data schema reset', lambda p,m: m.__setitem__('data_schema_version', 2)),
    ('protected runtime write', lambda p,m: m['runtime_kernel_write_allowlist'].append('NVRAM')),
    ('wrong root coordinates', lambda p,m: payload(m,'ANDROID').__setitem__('absolute_start_bytes', 0)),
    ('root image identity lie', lambda p,m: payload(m,'ANDROID')['raw'].__setitem__('sha256', '0'*64)),
    ('module ABI mismatch', lambda p,m: payload(m,'BOOTIMG')['modules'].__setitem__('release','wrong')),
    ('unlicensed public firmware claim', lambda p,m: m['owner_firmware'].__setitem__('redistribution_permission_established', True)),
    ('firmware receipt identity lie', lambda p,m: m['owner_firmware']['files']['WMT_SOC.cfg'].__setitem__('sha256', '0'*64)),
    ('root version lie', lambda p,m: m.__setitem__('rootfs_version', 'wrong')),
    ('fallback metadata identity lie', lambda p,m: m['fallback']['images'][0].__setitem__('sha256', '0'*64)),
    ('fallback image corruption', corrupt_fallback),
    ('data scatter selection', lambda p,m: set_scatter(p,m,'partition_name: USRDATA\n  file_name: NONE\n  is_download: false','partition_name: USRDATA\n  file_name: Y2DATA.img\n  is_download: true')),
    ('unchecked protected image mapping', lambda p,m: set_scatter(p,m,'partition_name: NVRAM\n  file_name: NONE','partition_name: NVRAM\n  file_name: Y2ROOT.img')),
    ('fallback data selection', lambda p,m: set_scatter(p,m,'partition_name: USRDATA\n  file_name: NONE\n  is_download: false','partition_name: USRDATA\n  file_name: Y2DATA.img\n  is_download: true', True)),
]

validate_manifest(package)
accepted=[]
with tempfile.TemporaryDirectory(prefix='y2-m5-package-', dir=PROJECT/'out') as tmp:
    for number, (label, mutate) in enumerate(cases):
        clone=Path(tmp)/str(number)
        for source in package.rglob('*'):
            target=clone/source.relative_to(package)
            target.parent.mkdir(parents=True, exist_ok=True)
            if source.is_dir(): target.mkdir(parents=True, exist_ok=True)
            elif source.suffix == '.img': os.link(source, target)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source,target)
        m=copy.deepcopy(original)
        mutate(clone,m)
        (clone/'manifest.json').write_text(json.dumps(m, indent=2)+'\n')
        sums=[]
        for path in sorted(clone.rglob('*')):
            if path.is_file() and path.name != 'SHA256SUMS':
                relative=str(path.relative_to(clone))
                sha=images[relative] if relative in images and path.samefile(package/relative) else digest(path)
                sums.append(sha+'  '+relative+'\n')
        (clone/'SHA256SUMS').write_text(''.join(sums))
        try:
            validate_manifest(clone)
        except (ValueError, KeyError, StopIteration, OSError) as e:
            print('PASS rejected',label,':',str(e),flush=True)
        else:
            print('FAIL accepted',label,flush=True)
            accepted.append(label)
        shutil.rmtree(clone)
assert all(digest(package/name)==sha for name,sha in images.items()), 'original images changed'
if accepted:
    raise SystemExit('Unexpectedly accepted: '+', '.join(accepted))
print(f'PASS {len(cases)} isolated rejection cases; original candidate/fallback images unchanged')
