#!/usr/bin/env python3
"""Upload one owner file over USB SFTP, then verify and publish without replacing music."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile


def upload(source, name, private_key, known_hosts):
    source = source.resolve(strict=True)
    if not source.is_file() or not private_key.is_file() or not known_hosts.is_file():
        raise ValueError('regular file, owner private key and verified known_hosts file required')
    with source.open('rb') as stream:
        sha256 = hashlib.file_digest(stream, 'sha256').hexdigest()
    options = ['-oBatchMode=yes', '-oStrictHostKeyChecking=yes', '-oConnectTimeout=5',
               '-oServerAliveInterval=5', '-oServerAliveCountMax=2',
               '-oUserKnownHostsFile=' + str(known_hosts.resolve()), '-i', str(private_key.resolve())]
    def remote(*args):
        result = subprocess.run(['ssh', *options, 'root@10.42.0.1',
                                 shlex.join(['y2-platform', 'transfer', *args])],
                                stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, check=True, timeout=660)
        if len(result.stdout) > 65536:
            raise ValueError('remote response too large')
        return json.loads(result.stdout)
    stage = remote('begin', '--name', name, '--bytes', str(source.stat().st_size), '--sha256', sha256)
    token = stage['id']
    print(json.dumps({'transfer_id': token, 'state': 'Staging'}), flush=True)
    try:
        with tempfile.TemporaryDirectory(prefix='y2-transfer-') as directory:
            root = Path(directory)
            # A fixed local operand prevents SFTP batch/glob metacharacters in
            # owner filenames from becoming commands or selecting other files.
            os.symlink(source, root/'payload')
            (root/'batch').write_text('put payload ' + stage['upload_path'] + '\n')
            subprocess.run(['sftp', *options, '-b', 'batch', 'root@10.42.0.1'],
                           cwd=root, stdin=subprocess.DEVNULL, check=True, timeout=3600)
        result = remote('commit', '--id', token)
        if result.get('state') != 'Committed' or result.get('sha256') != sha256:
            raise ValueError('remote commit receipt mismatch')
        print(json.dumps(result, sort_keys=True), flush=True)
        remote('discard', '--id', token)
    except Exception:
        # Preserve partial data/receipt for explicit inspection or abort. Never
        # transform disconnect into a claimed success or remove existing music.
        print(json.dumps({'state': 'Incomplete', 'transfer_id': token,
                          'next': 'inspect or discard with y2-platform transfer'}), flush=True)
        raise


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('file', type=Path)
    parser.add_argument('--name', help='relative destination under /data/music; no overwrite')
    parser.add_argument('--private-key', type=Path, required=True)
    parser.add_argument('--known-hosts', type=Path, required=True)
    args = parser.parse_args()
    upload(args.file, args.name or args.file.name, args.private_key, args.known_hosts)
