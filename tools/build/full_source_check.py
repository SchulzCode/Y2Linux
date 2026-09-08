"""Compare every upstream file/link with the checksum-locked release archive."""
import hashlib
import json
import posixpath
from pathlib import Path
import tarfile


def main():
    project = Path(__file__).resolve().parents[2]
    lock = json.loads((project/'tools/build/inputs.lock.json').read_text())
    item = next(x for x in lock['base_archives'] if x['name']=='linux-6.18.tar.xz')
    archive = project/'.cache/downloads'/item['name']
    with archive.open('rb') as f:
        if hashlib.file_digest(f,'sha256').hexdigest()!=item['sha256']: raise ValueError('source archive hash')
    root = project/'.cache/sources/linux-6.18'
    entries = {}
    with tarfile.open(archive) as tar:
        for member in tar:
            path = Path(member.name)
            if path.parts[0]!='linux-6.18' or '..' in path.parts: raise ValueError('archive path')
            name = path.relative_to('linux-6.18').as_posix()
            dest = root/name
            if member.isfile():
                with tar.extractfile(member) as f: expected = hashlib.file_digest(f,'sha256').hexdigest()
                with dest.open('rb') as f: actual = hashlib.file_digest(f,'sha256').hexdigest()
                if actual!=expected or dest.is_symlink(): raise ValueError('source differs: '+name)
                entries[name] = expected
            elif member.issym():
                if not dest.is_symlink() or str(dest.readlink())!=posixpath.normpath(member.linkname): raise ValueError('source link differs: '+name)
                entries[name] = 'symlink:'+posixpath.normpath(member.linkname)
            elif not member.isdir(): raise ValueError('unexpected archive type')
    actual = {p.relative_to(root).as_posix() for p in root.rglob('*') if p.is_file() or p.is_symlink()}
    if actual!=set(entries): raise ValueError('added/missing upstream source paths')
    manifest = ''.join(f'{value}  {name}\n' for name,value in sorted(entries.items()))
    print(json.dumps({'archive_sha256':item['sha256'],'upstream_commit':lock['linux_commit'],
        'verified_files_and_links':len(entries),'tree_manifest_sha256':hashlib.sha256(manifest.encode()).hexdigest(),
        'result':'PASS pristine source cache; only declared Kconfig overlay at build time'},indent=2))

if __name__=='__main__': main()
