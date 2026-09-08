#!/usr/bin/env python3
"""Read/hash the owner-proven recovery files. Never contacts or writes a device."""
import argparse
import csv
import hashlib
import json
from pathlib import Path

PROJECT = Path(__file__).resolve().parents[2]
TOOL_FILES = {
    'flash_tool': (11392491, 'd618e7d08ba5a4020038921a95336a3805a4b2cf17a374fd308cc5299ea7d9d8'),
    'MTK_AllInOne_DA.bin': (16107176, '46cd175d7556e6e80b13f6a70827c6931a5dfa25a09c3cc50e75ba7ff9327618'),
}

def verify(path, size, expected):
    with path.open('rb') as stream:
        actual = hashlib.file_digest(stream, 'sha256').hexdigest()
    if path.stat().st_size != size or actual != expected:
        raise ValueError('Recovery input mismatch: ' + str(path))
    return {'path': str(path.resolve()), 'bytes': size, 'sha256': actual}

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--rom',type=Path,default=Path('/home/luca/Dokumente/Code/Y2Player/y2_v3.2.0_FM-20260813'))
    parser.add_argument('--tool',type=Path,default=Path('/home/luca/Downloads/SP_Flash_Tool_v5.2032_Linux'))
    args=parser.parse_args()
    rows=[]
    with (PROJECT/'docs/knowledge/handoff-artifacts.tsv').open() as stream:
        for row in csv.DictReader(stream,delimiter='\t'):
            prefix='Y2Player/y2_v3.2.0_FM-20260813/'
            if row['source_locator'].startswith(prefix):
                name=row['source_locator'][len(prefix):]
                if Path(name).name != name: raise ValueError('Unexpected ROM manifest path')
                rows.append(verify(args.rom/name,int(row['bytes']),row['sha256']))
    if len(rows)!=24: raise ValueError('Expected complete 24-file FM ROM manifest')
    for name,(size,digest) in TOOL_FILES.items(): rows.append(verify(args.tool/name,size,digest))
    print(json.dumps({'status':'PASS local recovery bytes; no device operation',
        'bootimg_region':'EMMC_USER [0x01d80000,0x02d80000)',
        'fallback':'FM boot.img, not an installed-device backup',
        'spft_version':'5.2032.00', 'files':rows},indent=2))

if __name__=='__main__': main()
