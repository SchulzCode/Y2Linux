#!/usr/bin/env python3
"""Verify operator-supplied SPFT files only; never connects to a device."""
import argparse, hashlib, json, sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[2]))
from tools.production.layout import require, digest

def prefix(path,length):
    require(path.is_file() and not path.is_symlink(),'regular readback file required')
    h=hashlib.sha256()
    with path.open('rb') as f:
        while length:
            b=f.read(min(length,1048576));require(b,'short readback');h.update(b);length-=len(b)
    return h.hexdigest()

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--package',type=Path,required=True);p.add_argument('--before',type=Path,required=True);p.add_argument('--after',type=Path);p.add_argument('--before-only',action='store_true');a=p.parse_args()
    m=json.loads((a.package/'manifest.json').read_text());plan=json.loads((a.package/'metadata/readback-plan.json').read_text())
    for item in plan['preservation_ranges']:
        before=a.before/item['file']
        require(before.stat().st_size==item['size_bytes'],'before readback length '+item['file'])
        if not a.before_only:
            require(a.after is not None,'--after required unless --before-only')
            after=a.after/item['file']
            require(after.stat().st_size==item['size_bytes'],'after readback length '+item['file'])
            require(digest(before)==digest(after),'preservation failure '+item['file'])
        if 'stock_prefix_sha256' in item:require(prefix(before,item['stock_prefix_size_bytes'])==item['stock_prefix_sha256'],'current table differs from stock '+item['file'])
        print('BEFORE RETAINED' if a.before_only else 'UNCHANGED',item['file'])
    if a.before_only:
        print('PASS before lengths and original table identity; retain files independently before manual write')
        return
    for item in m['payloads']:
        path=a.after/(item['target_partition']+'.bin');raw=item['raw']
        require(path.stat().st_size==item['maximum_size_bytes'],'full partition readback length')
        require(prefix(path,raw['size_bytes'])==raw['sha256'],'raw image prefix mismatch '+path.name)
        print('IMAGE VERIFIED',path.name,raw['size_bytes'],'bytes at physical',hex(item['absolute_start_bytes']))
    print('PASS supplied readback bytes; operator DA/address identity and physical boot still required')
if __name__=='__main__':main()
