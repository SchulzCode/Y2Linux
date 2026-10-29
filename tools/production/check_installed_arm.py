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

# The Rust platform under qemu on an empty fixture root (Y2_ROOT): no device command runs there
# (fixture answers), unknown hardware is never reported healthy.
with tempfile.TemporaryDirectory(prefix='y2-arm-status-fixture-') as root:
    fixture = dict(os.environ, Y2_ROOT=root)
    status = json.loads(run('usr/sbin/y2-platform', ['status'], env=fixture).stdout)
    answer = subprocess.run(prefix+[str(target/'usr/sbin/y2-platform'), 'health', '--full'], text=True,
                            capture_output=True, timeout=180, env=fixture)
    health = json.loads(answer.stdout)
    assert status['schema'] == 'org.y2linux.status/v1' and status['record']['y2linux_commit'] is None
    assert health['schema'] == 'org.y2linux.health/v1' and health['state'] != 'OK'
    assert answer.returncode == (1 if health['state'] == 'FAILED' else 0)
platform = dict(fixture_status_schema=status['schema'], fixture_health=health['state'],
                unavailable_hardware_not_reported_healthy=True)
audio_command=['bwrap','--unshare-all','--uid','0','--gid','0','--ro-bind',str(target),'/',
               '--dev','/dev','--proc','/proc','--tmpfs','/tmp','--tmpfs','/run',
               '--ro-bind',str(qemu),'/run/qemu','--setenv','ALSA_CONFIG_PATH','/usr/share/alsa/alsa.conf',
               '--','/run/qemu','-cpu','cortex-a7','/usr/sbin/y2-audio-contract','--device','null']
audio=json.loads(subprocess.check_output(audio_command,text=True,timeout=15))
assert len(audio['combinations']) == 16 and all(v['accepted_constraints'] for v in audio['combinations'])
assert not audio['physical_qualification'] and not audio['pcm_started']
defaults = json.loads(run('usr/bin/reborn', ['--default-settings']).stdout)
assert defaults['session_schema'] == 3 and 'volume' in defaults['settings']

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
# Phase 6 of the platform port: no Python interpreter in the image.
assert not list((target/'usr/bin').glob('python3*')) and not list((target/'usr/lib').glob('python3*')), 'Python installed'
for name in ('usr/sbin/y2-platform','usr/sbin/y2-audio-contract','usr/bin/reborn','usr/bin/reborn-bench'):
    with (target/name).open('rb') as f: binaries[name]=hashlib.file_digest(f,'sha256').hexdigest()
help_text=run('usr/bin/bluealsad', ['--help']).stdout
codec_inventory=json.loads((target/'etc/y2linux/bluetooth-codecs.json').read_text())
# RELEASE-49: a public image compiles SBC, AAC and LDAC; aptX/aptX HD only private.
expected={'SBC','AAC','LDAC'} if codec_inventory['profile']=='public' else {'SBC','AAC','aptX','aptX-HD','LDAC'}
for codec in expected:
    assert codec in help_text, codec
(build/'arm-bluealsa-help.txt').write_text(help_text)
assert {n for n,v in codec_inventory['codecs'].items() if v['compiled_locally']}==expected
assert all(not v['platform_qualified'] for v in codec_inventory['codecs'].values())
receipt=dict(versions=versions,hardware_validation=False,passed=True,binaries=binaries,
             platform=platform,audio_null_constraints=audio,reborn_defaults=defaults,codec_software_contract='arm-codec-software-contract.txt',
             library_benchmark='arm-library-1000.json',evidence_level='ARM_BUILT')
(build/'installed-arm-tools.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(dict(passed=True,hardware_validation=False,platform_health=platform['fixture_health'])))
