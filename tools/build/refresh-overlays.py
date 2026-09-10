#!/usr/bin/env python3
"""Regenerate only changed patch overlays from an explicitly supplied work tree."""
import argparse, difflib, hashlib, json
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('work',type=Path);args=p.parse_args()
root=Path(__file__).resolve().parents[2]; manifest=root/'kernel/patches/manifest.json'
m=json.loads(manifest.read_text())
for spec in m['overlays']:
    new=args.work/spec['path']
    if not new.exists(): continue
    raw=new.read_bytes(); digest=hashlib.sha256(raw).hexdigest()
    if digest==spec['result_sha256']: continue
    base=(root/'.cache/sources/linux-6.18'/spec['path']).read_bytes()
    if hashlib.sha256(base).hexdigest()!=spec['base_sha256']: raise ValueError('Base mismatch')
    patch=''.join(difflib.unified_diff(base.decode().splitlines(True),raw.decode().splitlines(True),fromfile='a/'+spec['path'],tofile='b/'+spec['path']))
    (root/'kernel/patches'/spec['patch']).write_text(patch)
    spec['result_sha256']=digest
    print(spec['path'])
manifest.write_text(json.dumps(m,indent=2)+'\n')
