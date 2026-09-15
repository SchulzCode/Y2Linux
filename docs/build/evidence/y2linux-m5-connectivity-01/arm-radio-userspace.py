#!/usr/bin/env python3
"""Run final ARM programs in a read-only, network/device-isolated target root."""
import hashlib
from pathlib import Path
import subprocess
import sys

P=next(p for p in Path(__file__).resolve().parents if (p/'tools/production').is_dir() and (p/'AGENTS.md').is_file())
B=Path(sys.argv[1]).resolve()
lock=hashlib.sha256((P/'tools/build/inputs.lock.json').read_bytes()+
                    (P/'tools/build/connectivity-host.lock.json').read_bytes()).hexdigest()
qemu=P/'.cache'/('environment-m5-'+lock[:12])/'usr/bin/qemu-arm'
prefix=['bwrap','--unshare-all','--uid','0','--gid','0','--die-with-parent',
        '--new-session','--clearenv','--ro-bind',str(B/'buildroot/target'),'/',
        '--proc','/proc','--dev','/dev','--tmpfs','/tmp','--ro-bind',str(qemu),'/tmp/qemu-arm',
        '--setenv','PATH','/usr/bin:/bin:/usr/sbin:/sbin','--chdir','/',
        '--','/tmp/qemu-arm','-cpu','cortex-a7']
def run(args):
    r=subprocess.run(prefix+args,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,timeout=20)
    print(r.stdout,end='')
    return r

for args,version in [(['/usr/sbin/wpa_supplicant','-v'],'v2.12'),
                     (['/usr/libexec/bluetooth/bluetoothd','-v'],'5.79'),
                     (['/usr/bin/bluealsa','--version'],'v4.3.1'),
                     (['/usr/sbin/iw','--version'],'6.9'),
                     (['/usr/bin/dbus-daemon','--version'],'1.14.10')]:
    r=run(args);assert r.returncode==0 and version in r.stdout,args
    print('PASS actual ARM version:',args[0])
r=run(['/usr/bin/bluealsa','--help'])
assert r.returncode==0 and 'a2dp-source:\tSBC\n' in r.stdout
print('PASS A2DP Source codec selection: SBC')
r=run(['/usr/bin/aplay','-L'])
assert r.returncode==0 and 'ALSA lib' not in r.stdout and 'null\n' in r.stdout
print('PASS ALSA configuration loads inside the isolated target root')
r=run(['/usr/bin/aplay','-D','bluealsa','-t','raw','-f','S16_LE','-r','44100','-c','2','-d','1','/dev/zero'])
assert r.returncode==1 and 'bluealsa-pcm.c:' in r.stdout and "Couldn't initialize D-Bus context" in r.stdout
print('PASS BlueALSA PCM plugin loads; expected unavailable isolated D-Bus, no radio/audio operation claimed')
print('PASS 8 isolated ARM userspace checks; no physical devices or network accessed')
