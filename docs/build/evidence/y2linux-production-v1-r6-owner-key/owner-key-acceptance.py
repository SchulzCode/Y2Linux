"""Host-only acceptance of the owner-key initialization package; no devices."""
import contextlib
import hashlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile

PROJECT = Path.cwd()
sys.path.insert(0, str(PROJECT))
from tools.production.layout import TARGETS, digest
from tools.production.owner_data import fs_read, public_key, validate_package

base = PROJECT/'out/y2linux-production-v1-r4'
system = PROJECT/'out/y2linux-production-v1-r6'
candidate = PROJECT/'out/y2linux-production-v1-r6-owner-key'
owner_public = Path.home()/'.ssh/y2linux_ed25519.pub'
manifest = validate_package(candidate, owner_public)
key = public_key(owner_public)
assert hashlib.sha256(key).hexdigest() == 'bf9cc937fb5dcee8569514b674fb9b3d5ba25396162efac7e8854f88bfe98c25'
print('PASS exact existing owner public file, fingerprint SHA256:IS9HhrmQKRtlEEbrcJjZa0XQHVwnRXRUzQQ61/7XAb8')
expected_data_sha = manifest['payloads'][0]['raw']['sha256']

root = base/'Y2ROOT.img'
for path, target in [('root/.ssh', '/data/ssh/authorized_keys.d'),
                     ('etc/dropbear', '/data/ssh/host-keys')]:
    assert 'Fast link dest: "'+target+'"' in fs_read(root, 'stat /'+path).decode()
    print('PASS retained root symlink /'+path+' -> '+target)
for name in ('etc/init.d/S50dropbear', 'etc/default/dropbear'):
    expected = (PROJECT/'buildroot/board/y2/overlay'/name).read_bytes()
    assert fs_read(root, 'cat /'+name) == expected
assert fs_read(root, 'cat /etc/init.d/S02y2-data') == (PROJECT/'buildroot/board/y2/production-overlay/etc/init.d/S02y2-data').read_bytes()
print('PASS installed root contains unchanged USB-address wait, key-only Dropbear and persistent data setup')
for image, expected in [(system/'BOOTIMG.img', '3ba809e44bd9c945c994209d1d93f41fe0da9259a303487dd6ecf49724bf49a8'),
                        (root, '814a5b2543931e01cee2eb6f641c3b6e02317bd6308d2d663aea78f618bd554f'),
                        (base/'Y2DATA.img', '01bcf65da1716081707202af074969e21fc1196540cb1de6ca76ef9098d44079')]:
    assert digest(image) == expected
print('PASS original BOOTIMG, Y2ROOT and unused Y2DATA seed remain byte-identical')

def edit_json(path, change):
    value = json.loads(path.read_text())
    change(value)
    path.write_text(json.dumps(value, indent=2)+'\n')

def manifest_change(change):
    return lambda out: edit_json(out/'manifest.json', change)

def refresh_image_identity(out):
    def change(m):
        for transport in ('raw', 'spft'):
            m['payloads'][0][transport]['sha256'] = digest(out/'Y2DATA.img')
    edit_json(out/'manifest.json', change)

def protected_selection(out):
    scatter = out/'MT6582_Y2DATA_only_scatter.txt'
    text = scatter.read_text()
    old = 'partition_name: NVRAM\n  file_name: NONE\n  is_download: false'
    assert old in text
    scatter.write_text(text.replace(old, 'partition_name: NVRAM\n  file_name: NVRAM.img\n  is_download: true'))
    edit_json(out/'manifest.json', lambda m: m['profiles'][scatter.name].update(sha256=digest(scatter)))

def change_permissions(out):
    subprocess.run(['debugfs', '-w', '-R', 'set_inode_field /ssh/authorized_keys.d/authorized_keys mode 0100666', str(out/'Y2DATA.img')],
                   check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    refresh_image_identity(out)

def unexpected_host_state(out):
    # A harmless synthetic marker, never a real or generated private key.
    marker = out/'metadata/synthetic-host-state.txt'
    marker.write_text('synthetic state that must not be packaged\n')
    subprocess.run(['debugfs', '-w', '-R', 'write '+str(marker)+' /ssh/host-keys/unexpected', str(out/'Y2DATA.img')],
                   check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    refresh_image_identity(out)

def unrelated_public(out):
    # Reuse a retained public-only fixture, not a private identity or a new key.
    old_key = fs_read(base/'Y2DATA.img', 'cat /ssh/authorized_keys.d/authorized_keys')
    (out/'metadata/owner-authorized.pub').write_bytes(old_key)
    edit_json(out/'manifest.json', lambda m: m['debug_access'].update(authorized_public_key_sha256=hashlib.sha256(old_key).hexdigest()))

cases = {
    'different owner public key despite updated public hash': unrelated_public,
    'shifted USRDATA address': manifest_change(lambda m: m['payloads'][0].update(absolute_start_bytes=TARGETS['USRDATA']['start']+512)),
    'wrong Y2DATA UUID': manifest_change(lambda m: m['payloads'][0]['filesystem'].update(uuid='wrong')),
    'NVRAM selection despite matching scatter hash': protected_selection,
    'USRDATA added to preserving system updates': manifest_change(lambda m: m['normal_update_allowlist'].append('USRDATA')),
    'data initialization falsely claims preservation': manifest_change(lambda m: m.update(preserves_existing_data=True)),
    'extra BOOTIMG payload file': lambda out: (out/'BOOTIMG.img').write_bytes(b'unexpected payload'),
    'wrong native disk offset': manifest_change(lambda m: m['storage_addressing'].update(logical_to_native_user_offset_sectors=0)),
    'wrong filesystem bytes hash': manifest_change(lambda m: m['payloads'][0]['raw'].update(sha256='0'*64)),
    'writable authorization despite updated image hash': change_permissions,
    'host state in seed despite updated image hash': unexpected_host_state,
}
for label, change in cases.items():
    with tempfile.TemporaryDirectory(prefix='y2-owner-data-acceptance-', dir='/tmp') as temporary:
        out = Path(temporary)/'candidate'
        subprocess.run(['cp', '-a', '--sparse=always', str(candidate), str(out)], check=True)
        change(out)
        # Refresh inventory to exercise semantic guards rather than stale hashes.
        (out/'SHA256SUMS').write_text(''.join(digest(p)+'  '+str(p.relative_to(out))+'\n'
            for p in sorted(out.rglob('*')) if p.is_file() and p.name != 'SHA256SUMS'))
        try:
            with contextlib.redirect_stdout(io.StringIO()):
                validate_package(out, owner_public)
        except ValueError as error:
            print('PASS rejected '+label+': '+str(error), flush=True)
        else:
            raise AssertionError('accepted '+label)
assert digest(candidate/'Y2DATA.img') == expected_data_sha
print('PASS all 11 isolated host-only mutations refused; final candidate preserved')
print('Y2DATA SHA256 '+expected_data_sha)
