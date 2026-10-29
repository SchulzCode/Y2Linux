#!/usr/bin/env python3
"""Exercise the actual SFTP subsystem over stdio in fresh scratch files, no network/device."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import select
import struct
import subprocess
import tempfile
import time


def blob(value):
    return struct.pack('>I', len(value)) + value


def receive(process):
    def exact(size):
        result = bytearray()
        deadline = time.monotonic() + 5
        while len(result) < size:
            wait = deadline - time.monotonic()
            if wait <= 0 or not select.select([process.stdout], [], [], wait)[0]:
                raise ValueError('SFTP response timeout')
            value = os.read(process.stdout.fileno(), size-len(result))
            if not value:
                raise ValueError('SFTP premature EOF')
            result.extend(value)
        return bytes(result)
    size = struct.unpack('>I', exact(4))[0]
    if not 1 <= size <= 1024**2:
        raise ValueError('SFTP response bound')
    return exact(size)


def send(process, packet):
    process.stdin.write(blob(packet)); process.stdin.flush()
    return receive(process)


def check(server, cc, qemu_root=None):
    with tempfile.TemporaryDirectory(prefix='y2-sftp-test-') as directory:
        root = Path(directory)
        source = root/'low-space.c'
        source.write_text('''#define _GNU_SOURCE
#include <sys/statvfs.h>
#include <string.h>
#define LOW(s) do { memset(s,0,sizeof(*(s))); (s)->f_bsize=(s)->f_frsize=4096; (s)->f_bavail=1; (s)->f_files=(s)->f_favail=10000; } while(0)
int fstatvfs(int fd,struct statvfs *s) { (void)fd; LOW(s); return 0; }
#if defined(__GLIBC__)
int fstatvfs64(int fd,struct statvfs64 *s) { (void)fd; LOW(s); return 0; }
#endif
''')
        shim = root/'low-space.so'
        subprocess.run([cc, '-shared', '-fPIC', str(source), '-o', str(shim)], check=True, timeout=60)
        for case in ('success', 'space_reserve', 'interrupted'):
            environment = os.environ.copy()
            command = [str(server), '-e', '-P', 'symlink,hardlink,copy-data']
            if qemu_root:
                command = ['qemu-arm', '-cpu', 'cortex-a7', '-L', str(qemu_root),
                           *(['-E', 'LD_PRELOAD='+str(shim)] if case == 'space_reserve' else []), *command]
            elif case == 'space_reserve':
                environment['LD_PRELOAD'] = str(shim)
            process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                       stderr=subprocess.PIPE, env=environment)
            path = root/(case+'.part')
            try:
                response = send(process, b'\x01'+struct.pack('>I', 3))
                assert response[:5] == b'\x02\0\0\0\x03', 'SFTP v3 negotiation'
                response = send(process, b'\x03'+struct.pack('>I', 1)+blob(os.fsencode(path))+struct.pack('>II', 42, 0))
                assert response[0] == 102, 'SFTP handle'
                length = struct.unpack('>I', response[5:9])[0]
                handle = response[9:9+length]
                payload = b'fresh dedicated SFTP scratch payload\0'*500
                response = send(process, b'\x06'+struct.pack('>I', 2)+blob(handle)+struct.pack('>Q', 0)+blob(payload))
                assert response[0] == 101
                status = struct.unpack('>I', response[5:9])[0]
                if case == 'space_reserve':
                    assert status != 0 and b'Y2 storage reserve' in response, 'actual write reserve rejected'
                    assert path.read_bytes() == b'', 'no data written through reserve'
                else:
                    assert status == 0 and path.read_bytes() == payload, 'exact protocol write'
                if case == 'interrupted':
                    process.kill(); process.wait(timeout=5)
                    assert path.exists(), 'partial remains available for explicit cleanup'
                else:
                    response = send(process, b'\x04'+struct.pack('>I', 3)+blob(handle))
                    assert struct.unpack('>I', response[5:9])[0] == 0
                    process.stdin.close(); process.stdin = None
                    process.communicate(timeout=5)
                    assert process.returncode == 0
            finally:
                if process.poll() is None: process.kill()
                process.communicate(timeout=5)
    return {'passed': True, 'checks': ['protocol_write_close_readback', 'injected_space_reserve', 'interrupted_session'],
            'server_sha256': hashlib.sha256(server.read_bytes()).hexdigest(),
            'evidence_level': 'ARM_BUILT' if qemu_root else 'HOST_TESTED', 'hardware_validation': False}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--server', type=Path, required=True)
    parser.add_argument('--cc', required=True)
    parser.add_argument('--qemu-root', type=Path)
    args = parser.parse_args()
    print(json.dumps(check(args.server.resolve(), args.cc, args.qemu_root), sort_keys=True))
