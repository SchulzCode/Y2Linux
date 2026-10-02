#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only
"""Validate installed ARM ELF class/ABI, interpreter and dependency closure."""
import json
import os
from pathlib import Path
import posixpath
import re
import subprocess
import sys
import struct

target=Path(sys.argv[1]).resolve()
def resolve(name):
    name='/' + name.lstrip('/')
    for _ in range(32):
        parts=name.lstrip('/').split('/')
        for i in range(len(parts)):
            path=target.joinpath(*parts[:i+1])
            if path.is_symlink():
                link=os.readlink(path)
                suffix='/'.join(parts[i+1:])
                name=posixpath.normpath(posixpath.join(link if link.startswith('/') else
                    posixpath.join('/',*parts[:i],link),suffix))
                break
        else:
            path=target/name.lstrip('/')
            assert path.is_file(), name
            return path
    raise AssertionError('symlink loop '+name)
count=0; dependencies=0
for path in sorted(target.rglob('*')):
    if path.is_symlink() or not path.is_file(): continue
    raw=path.read_bytes()
    if raw[:4]!=b'\x7fELF': continue
    assert raw[:6]==b'\x7fELF\x01\x01' and struct.unpack_from('<H',raw,18)[0]==40,path
    dynamic=subprocess.check_output(['readelf','-d',str(path)],text=True)
    headers=subprocess.check_output(['readelf','-l',str(path)],text=True)
    linked=bool(re.search(r'\(NEEDED\)',dynamic) or 'Requesting program interpreter' in headers)
    assert not linked or struct.unpack_from('<I',raw,36)[0]&0x400,path
    assert all(not value for value in re.findall(r'\((?:RPATH|RUNPATH)\).*?\[(.*?)\]',dynamic)),path
    for name in re.findall(r'\(NEEDED\).*?\[(.*?)\]',dynamic):
        assert '/' not in name,name
        matches=[]
        for directory in ('lib','usr/lib'):
            try: matches.append(resolve(directory+'/'+name))
            except AssertionError: pass
        assert matches,(path,name)
        dependencies+=1
    headers=subprocess.check_output(['readelf','-l',str(path)],text=True)
    for interpreter in re.findall(r'Requesting program interpreter: (.*?)\]',headers):
        resolve(interpreter)
    count+=1
print(json.dumps(dict(passed=True,hardware_validation=False,elf_files=count,dependency_edges=dependencies,
                     abi='ELF32 ARM little-endian; dynamic hard-float, freestanding static ABI isolated',build_rpaths=False)))
