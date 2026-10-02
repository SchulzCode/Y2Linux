#!/usr/bin/env python3
"""Retrieve one redacted tester bundle through the owner's existing SSH connection."""
# SPDX-License-Identifier: GPL-2.0-only
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess


def retrieve(host, output, private_key=None, known_hosts=None):
    if not host or host.startswith('-') or not re.fullmatch(r'[A-Za-z0-9_.@:-]+', host):
        raise ValueError('invalid_ssh_host')
    if output.exists():
        raise ValueError('fresh_output_required')
    options = ['ssh', '-T', '-oBatchMode=yes', '-oStrictHostKeyChecking=yes', '-oConnectTimeout=5',
               '-oServerAliveInterval=5', '-oServerAliveCountMax=2']
    if private_key:
        options += ['-i', str(private_key.resolve())]
    if known_hosts:
        options += ['-oUserKnownHostsFile='+str(known_hosts.resolve())]
    receipt = subprocess.run([*options, host, 'y2-platform export-diagnostics'],
                             capture_output=True, timeout=90, check=True)
    if len(receipt.stdout) > 8192:
        raise ValueError('invalid_diagnostic_receipt')
    value = json.loads(receipt.stdout)
    if (value.get('state') != 'Complete' or value.get('redacted') is not True
            or not re.fullmatch(r'/data/exports/y2-diagnostics-[0-9]+-[0-9a-f]{32}\.tar\.gz', value.get('path', ''))
            or not re.fullmatch(r'[0-9a-f]{64}', value.get('sha256', ''))
            or type(value.get('bytes')) is not int or not 0 < value['bytes'] <= 2*1024**2):
        raise ValueError('invalid_diagnostic_receipt')
    output.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    try:
        with os.fdopen(fd, 'wb') as stream:
            result = subprocess.run([*options, host, shlex.join(['cat', value['path']])],
                                    stdout=stream, stderr=subprocess.PIPE, timeout=60)
            stream.flush(); os.fsync(stream.fileno())
        if result.returncode or output.stat().st_size != value['bytes']:
            raise ValueError('diagnostic_transfer_incomplete')
        with output.open('rb') as stream:
            if hashlib.file_digest(stream, 'sha256').hexdigest() != value['sha256']:
                raise ValueError('diagnostic_transfer_hash_mismatch')
    except BaseException:
        output.unlink(missing_ok=True)
        raise
    return {**value, 'local_path': str(output)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--host', default='y2')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--private-key', type=Path)
    parser.add_argument('--known-hosts', type=Path)
    args = parser.parse_args()
    print(json.dumps(retrieve(args.host, args.output, args.private_key, args.known_hosts), indent=2))


if __name__ == '__main__': main()
