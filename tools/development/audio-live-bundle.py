#!/usr/bin/env python3
"""Bundle already-built M3 ALSA tools for isolated /tmp diagnostics over SSH.

No rebuild, rootfs replacement, automatic playback or physical-register writes.
"""
import argparse, hashlib, io, json, tarfile
from pathlib import Path

def build(root, output):
    target=root/'buildroot/target'
    files={}
    for name in ('aplay','amixer'):
        files['bin/'+name]=((target/'usr/bin'/name).read_bytes(),0o755)
        files[name]=(('#!/bin/sh\nset -eu\nd=$(CDPATH= cd -- "$(dirname "$0")" && pwd)\n'
            'export ALSA_CONFIG_PATH="$d/asound.conf"\n'
            'exec "$d/lib/ld-linux-armhf.so.3" --library-path "$d/lib" "$d/bin/'+name+'" "$@"\n').encode(),0o755)
    for name in ('libasound.so.2','libm.so.6','libc.so.6','ld-linux-armhf.so.3'):
        src=next(p for p in (target/'lib'/name,target/'usr/lib'/name) if p.is_file())
        files['lib/'+name]=(src.read_bytes(),0o755)
    for rate in (44100,48000):
        name=f'headphone-{rate}.wav'
        files['audio/'+name]=((root/'audio'/name).read_bytes(),0o644)
    # Only hardware plugins; no dependency on the installed rootfs ALSA data.
    files['asound.conf']=(b'''pcm.hw {
    @args [ CARD DEV SUBDEV ]
    @args.CARD { type string default Y2Audio }
    @args.DEV { type integer default 0 }
    @args.SUBDEV { type integer default -1 }
    type hw
    card $CARD
    device $DEV
    subdevice $SUBDEV
}
ctl.sysdefault {
    @args [ CARD ]
    @args.CARD { type string default Y2Audio }
    type hw
    card $CARD
}
ctl.hw {
    @args [ CARD ]
    @args.CARD { type string default Y2Audio }
    type hw
    card $CARD
}
''',0o644)
    manifest={n:hashlib.sha256(raw).hexdigest() for n,(raw,_) in files.items()}
    files['SHA256SUMS']=(''.join(h+'  '+n+'\n' for n,h in sorted(manifest.items())).encode(),0o644)
    with tarfile.open(output,'w') as tar:
        for name,(raw,mode) in sorted(files.items()):
            info=tarfile.TarInfo(name);info.mode=mode;info.size=len(raw)
            tar.addfile(info,io.BytesIO(raw))
    print(json.dumps({'archive':str(output),'sha256':hashlib.sha256(output.read_bytes()).hexdigest(),'files':manifest},indent=2))

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('root',type=Path);p.add_argument('output',type=Path)
    a=p.parse_args();build(a.root,a.output)
