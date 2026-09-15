import copy, hashlib, json, shutil, tempfile, sys
from pathlib import Path
from tools.production.validate import validate_boot_update
from tools.production.layout import digest

original = Path(sys.argv[1]).resolve()
manifest = validate_boot_update(original, Path(sys.argv[2]).resolve())

def write_json(p, obj): p.write_text(json.dumps(obj, indent=2)+'\n')
def refresh(p):
    (p/'SHA256SUMS').write_text(''.join(digest(f)+'  '+str(f.relative_to(p))+'\n'
        for f in sorted(p.rglob('*')) if f.is_file() and f.name!='SHA256SUMS'))
def change_manifest(p, fn):
    m=json.loads((p/'manifest.json').read_text());fn(m);write_json(p/'manifest.json',m)
def fallback_version(p):
    fm=json.loads((p/'metadata/fallback-manifest.json').read_text())
    fm['rootfs_version']='unreviewed-root'
    write_json(p/'metadata/fallback-manifest.json',fm)
    change_manifest(p,lambda m:m['fallback'].update(manifest_sha256=digest(p/'metadata/fallback-manifest.json')))
def corrupt_fallback(p):
    file=p/'fallback/BOOTIMG-previous.img';data=bytearray(file.read_bytes());data[2048]^=1;file.write_bytes(data)

cases={
 'rootfs version mismatch':lambda p:change_manifest(p,lambda m:m.update(rootfs_version='new-root')),
 'data identity changed':lambda p:change_manifest(p,lambda m:m['installed_components'][1]['filesystem'].update(uuid='changed')),
 'protected runtime write':lambda p:change_manifest(p,lambda m:m['runtime_kernel_write_allowlist'].append('PRELOADER')),
 'wrong BOOTIMG coordinates':lambda p:change_manifest(p,lambda m:m['payloads'][0].update(absolute_start_bytes=0)),
 'module version mismatch':lambda p:change_manifest(p,lambda m:m['payloads'][0]['modules'].update(release='6.18.0-y2linux-storage06')),
 'second payload':lambda p:change_manifest(p,lambda m:m['payloads'].append(copy.deepcopy(m['installed_components'][0]))),
 'rootfs image added':lambda p:(p/'Y2ROOT.img').write_bytes(b''),
 'data image added':lambda p:(p/'Y2DATA.img').write_bytes(b''),
 'retained data hash changed':lambda p:change_manifest(p,lambda m:m['installed_components'][1]['raw'].update(sha256='0'*64)),
 'non-BOOTIMG scatter selection':lambda p:(p/'MT6582_BOOTIMG_only_scatter.txt').write_text((p/'MT6582_BOOTIMG_only_scatter.txt').read_text().replace('is_download: false','is_download: true',1)),
 'fallback incompatible root':fallback_version,
 'fallback corruption':corrupt_fallback,
 'fallback version lie':lambda p:change_manifest(p,lambda m:m['fallback'].update(kernel_version='wrong-kernel')),
 'boot identity lie':lambda p:change_manifest(p,lambda m:m['payloads'][0]['raw'].update(sha256='0'*64)),
}
for name, mutate in cases.items():
    with tempfile.TemporaryDirectory(prefix='m5-boot-package-') as directory:
        candidate=Path(directory)/'package';shutil.copytree(original,candidate)
        mutate(candidate);refresh(candidate)
        try: validate_boot_update(candidate)
        except (ValueError, KeyError) as error: print('PASS rejected',name,':',error)
        else: raise AssertionError('accepted '+name)
assert digest(original/'BOOTIMG.img')==manifest['payloads'][0]['raw']['sha256']
assert digest(original/manifest['fallback']['file'])==manifest['fallback']['sha256']
print(f'PASS {len(cases)} isolated package rejection cases; original images unchanged; system-update base needs no data image')
