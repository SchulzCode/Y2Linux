import copy, hashlib, json, shutil, tempfile
from pathlib import Path
from tools.production.validate import validate_boot_update
from tools.production.layout import digest

original = Path('out/y2linux-m4-01').resolve()
manifest = validate_boot_update(original)
assert manifest['build_git_commit'] == 'f2d2ac641ee67cd94cd2c57c9f5201b6e78841fc'
assert manifest['fallback']['sha256'] == '3ba809e44bd9c945c994209d1d93f41fe0da9259a303487dd6ecf49724bf49a8'
assert manifest['kernel_version'] == '6.18.0-y2linux-m4-01'

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
 'non-BOOTIMG scatter selection':lambda p:(p/'MT6582_BOOTIMG_only_scatter.txt').write_text((p/'MT6582_BOOTIMG_only_scatter.txt').read_text().replace('is_download: false','is_download: true',1)),
 'fallback incompatible root':fallback_version,
 'fallback corruption':corrupt_fallback,
 'fallback version lie':lambda p:change_manifest(p,lambda m:m['fallback'].update(kernel_version='wrong-kernel')),
 'boot identity lie':lambda p:change_manifest(p,lambda m:m['payloads'][0]['raw'].update(sha256='0'*64)),
}
for name, mutate in cases.items():
    with tempfile.TemporaryDirectory(prefix='m4-package-') as directory:
        candidate=Path(directory)/'package';shutil.copytree(original,candidate)
        mutate(candidate);refresh(candidate)
        try: validate_boot_update(candidate)
        except (ValueError, KeyError) as error: print('PASS rejected',name,':',error)
        else: raise AssertionError('accepted '+name)
print('PASS 12 isolated package rejection cases; original BOOTIMG and Storage06 fallback unchanged')
