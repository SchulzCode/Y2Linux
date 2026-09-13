#!/usr/bin/env python3
"""Prepare locked host build inputs. This is not device firmware downloading."""
import hashlib,json,shlex,shutil,subprocess,tarfile,urllib.request
from pathlib import Path

def buildroot_source(project):
    lock=json.loads((project/'buildroot/inputs.lock.json').read_text())
    archive=project/'.cache/downloads'/('buildroot-'+lock['version']+'.tar.xz')
    if not archive.exists():
        archive.parent.mkdir(parents=True,exist_ok=True)
        temporary=archive.with_suffix('.partial')
        with urllib.request.urlopen(lock['url'],timeout=120) as src,temporary.open('wb') as dst:
            shutil.copyfileobj(src,dst)
        with temporary.open('rb') as f:
            if hashlib.file_digest(f,'sha256').hexdigest()!=lock['sha256']:
                raise ValueError('Buildroot download hash mismatch')
        temporary.replace(archive)
    with archive.open('rb') as f:
        if hashlib.file_digest(f,'sha256').hexdigest()!=lock['sha256']:
            raise ValueError('Buildroot archive hash mismatch')
    source=project/'.cache/sources'/('buildroot-'+lock['version'])
    if not source.exists():
        source.parent.mkdir(parents=True,exist_ok=True)
        with tarfile.open(archive) as t:t.extractall(source.parent,filter='data')
    if not (source/'Makefile').is_file():raise ValueError('Buildroot source missing Makefile')
    return source,lock

def environment(project):
    if not (project/'.cache/environment/.y2-build-lock').exists():
        subprocess.run(['python3','tools/build/prepare.py'],cwd=project,check=True)
    if not shutil.which('bc'):
        directory=project/'.cache/host-tools';directory.mkdir(exist_ok=True)
        root=project/'.cache/environment'
        argv=[str(root/'lib/ld-musl-x86_64.so.1'),'--library-path',str(root/'usr/lib')+':'+str(root/'lib'),str(root/'usr/bin/bc')]
        wrapper=directory/'bc';wrapper.write_text('#!/bin/sh\nexec '+shlex.join(argv)+' "$@"\n');wrapper.chmod(0o755)

if __name__=='__main__':
    project=Path(__file__).resolve().parents[2]
    environment(project);print(buildroot_source(project)[0])
