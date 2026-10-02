#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

import argparse
parser = argparse.ArgumentParser(description='Verify fresh installed ARM platform/application/codec contracts without devices')
parser.add_argument('--build', type=Path, required=True)
args = parser.parse_args()
project = Path(__file__).resolve().parents[2]
build = args.build.resolve()
target = build/'buildroot/target'
qemu = project/'.cache/environment/usr/bin/qemu-arm'
prefix = [str(qemu), '-cpu', 'cortex-a7', '-L', str(target)]
versions = json.loads((build/'versions.json').read_text())

def run(relative, args, **kwargs):
    return subprocess.run(prefix+[str(target/relative), *args], text=True,
                          capture_output=True, check=True, timeout=180, **kwargs)

script = '''import importlib,json,pkgutil,sqlite3,ssl,tempfile
from pathlib import Path
import y2_platform
from y2_platform.common import Context
from y2_platform.observe import snapshot
from y2_platform.health import check
modules=sorted(m.name for m in pkgutil.iter_modules(y2_platform.__path__))
for name in modules: importlib.import_module('y2_platform.'+name)
with tempfile.TemporaryDirectory(prefix='y2-arm-status-fixture-') as root:
 ctx=Context(root)
 assert ctx.command(['never-execute-a-host-command'])['reason']=='fixture'
 status=snapshot(ctx)
 health=check(ctx,full=True)
 assert status['schema']=='org.y2linux.status/v1'
 assert status['record']['y2linux_commit'] is None
 assert health['state']!='OK'
 db=sqlite3.connect(str(Path(root)/'test.db'))
 db.execute('PRAGMA journal_mode=WAL')
 db.execute('CREATE TABLE test(value INTEGER)')
 db.execute('INSERT INTO test VALUES(42)');db.commit()
 assert db.execute('SELECT value FROM test').fetchone()==(42,)
 db.execute('PRAGMA wal_checkpoint(TRUNCATE)');db.close()
 print(json.dumps(dict(modules=modules,sqlite=sqlite3.sqlite_version,openssl=ssl.OPENSSL_VERSION,
                      fixture_status_schema=status['schema'],fixture_health=health['state'],
                      unavailable_hardware_not_reported_healthy=True)))
'''
env = os.environ.copy()
env.update(PYTHONHOME=str(target/'usr'), PYTHONPATH=str(target/'usr/lib/y2-platform'), PYTHONDONTWRITEBYTECODE='1')
python = json.loads(run('usr/bin/python3', ['-c', script], env=env).stdout)
audio_command=['bwrap','--unshare-all','--uid','0','--gid','0','--ro-bind',str(target),'/',
               '--dev','/dev','--proc','/proc','--tmpfs','/tmp','--tmpfs','/run',
               '--ro-bind',str(qemu),'/run/qemu','--setenv','ALSA_CONFIG_PATH','/usr/share/alsa/alsa.conf',
               '--','/run/qemu','-cpu','cortex-a7','/usr/sbin/y2-audio-contract','--device','null']
audio=json.loads(subprocess.check_output(audio_command,text=True,timeout=15))
assert len(audio['combinations']) == 12 and all(v['accepted_constraints'] for v in audio['combinations'])
assert not audio['physical_qualification'] and not audio['pcm_started']
defaults = json.loads(run('usr/bin/reborn', ['--default-settings']).stdout)
assert defaults['session_schema'] == 2 and 'volume' in defaults['settings']

with tempfile.TemporaryDirectory(prefix='y2-cpu-final-arm-bench-') as directory:
    fd=os.open(directory,os.O_RDONLY|os.O_DIRECTORY)
    try:
        result=run('usr/bin/reborn-bench',['--scratch-fd',str(fd),'--tracks','1000','--scan'],pass_fds=(fd,))
    finally: os.close(fd)
    benchmark=json.loads(result.stdout)
    assert 'database' in benchmark and benchmark['scan'] is not None
    (build/'arm-library-1000.json').write_text(json.dumps(dict(versions=versions,hardware_validation=False,result=benchmark),indent=2)+'\n')
    (build/'arm-library-1000.stderr').write_text(result.stderr)

codec_contract = run('usr/sbin/y2-codec-software-contract', []).stdout
(build/'arm-codec-software-contract.txt').write_text(codec_contract)
binaries={}
for name in ('usr/bin/python3.12','usr/sbin/y2-audio-contract','usr/bin/reborn','usr/bin/reborn-bench'):
    with (target/name).open('rb') as f: binaries[name]=hashlib.file_digest(f,'sha256').hexdigest()
help_text=run('usr/bin/bluealsad', ['--help']).stdout
for codec in ('SBC', 'AAC', 'aptX', 'aptX-HD', 'LDAC'):
    assert codec in help_text, codec
(build/'arm-bluealsa-help.txt').write_text(help_text)
codec_inventory=json.loads((target/'etc/y2linux/bluetooth-codecs.json').read_text())
assert all(v['compiled_locally'] for v in codec_inventory['codecs'].values())
assert all(not v['platform_qualified'] for v in codec_inventory['codecs'].values())
receipt=dict(versions=versions,hardware_validation=False,passed=True,binaries=binaries,
             python=python,audio_null_constraints=audio,reborn_defaults=defaults,codec_software_contract='arm-codec-software-contract.txt',
             library_benchmark='arm-library-1000.json',evidence_level='ARM_BUILT')
(build/'installed-arm-tools.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(dict(passed=True,hardware_validation=False,modules=len(python['modules']),sqlite=python['sqlite'],openssl=python['openssl'])))
