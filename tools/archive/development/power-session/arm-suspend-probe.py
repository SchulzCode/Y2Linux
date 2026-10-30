#!/usr/bin/env python3
"""Host side: arm the read-only suspend probe on the Y2 (needs the host device lease).

Verifies the installed identity, boot ID and taint, copies suspend-probe.py to
the tmpfs /run/y2 (nothing persistent except the probe's own capture directory
below /data/logs), starts it detached and writes a host receipt. Run only after
the owner has approved. It does not touch product state, PMIC, PM or the guard.
"""
import argparse
import datetime
import fcntl
import hashlib
import json
import shlex
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
SSH = ['ssh', '-T', '-oBatchMode=yes', '-oStrictHostKeyChecking=yes', '-oConnectTimeout=5',
       '-i', str(Path.home() / '.ssh/y2linux_ed25519'), 'root@10.42.0.1']
# Reviewed images the read-only probe may be armed on (exact commits, no wildcards).
ACCEPTED_COMMITS = ('5e267571d584da7fbe356d3161801ba15f87fa57',   # DEEPSLEEP-FINAL-04
                    '837b6146ee4d0f551b69cfaddd686a69ce56a060',   # POWER-FINAL-05
                    'c1ef37a6f6338b3abe7fcb4f2aa505b6fb861075')   # POWER-FINAL-06 bring-up
ACCEPTED_REBORN = '97ff8c357bab12319087740d7c97b5051e6588f5'


def ssh(command, stdin=None, timeout=30):
    return subprocess.run(SSH + [command], input=stdin, capture_output=True, timeout=timeout)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('receipt', type=Path, help='new host receipt directory')
    parser.add_argument('--max-seconds', type=int, default=3600)
    args = parser.parse_args()
    args.receipt.mkdir(parents=True, exist_ok=False)
    probe = (HERE / 'suspend-probe.py').read_bytes()
    with Path('/tmp/y2-platform-v1-device.lock').open('a') as lease:
        fcntl.flock(lease, fcntl.LOCK_EX | fcntl.LOCK_NB)
        check = ssh('cat /etc/y2linux/versions.json /proc/sys/kernel/random/boot_id /proc/sys/kernel/tainted; '
                    'ls /run/y2/suspend-probe.py 2>&1 | head -1')
        text = check.stdout.decode()
        versions = json.JSONDecoder().raw_decode(text)[0]
        rest = text[json.JSONDecoder().raw_decode(text)[1]:].split()
        boot_id, taint = rest[0], rest[1]
        if (versions.get('build_git_commit') not in ACCEPTED_COMMITS
                or versions.get('reborn_source_commit') != ACCEPTED_REBORN or taint != '0'):
            raise SystemExit('unexpected image or tainted kernel: refusing')
        if 'suspend-probe.py' in ' '.join(rest[2:]) and 'No such file' not in ' '.join(rest[2:]):
            raise SystemExit('a probe file already exists on the device: refusing')
        out = '/data/logs/suspend-probe-%s-%d' % (boot_id[:8], int(datetime.datetime.now().timestamp()))
        install = ssh('umask 077; cat > /run/y2/suspend-probe.py', stdin=probe)
        if install.returncode:
            raise SystemExit('install failed')
        start = ssh('cd /run/y2 && setsid nohup python3 -B /run/y2/suspend-probe.py --boot-id %s --out %s '
                    '--max-seconds %d >/run/y2/suspend-probe.log 2>&1 < /dev/null & echo started' % (
                        shlex.quote(boot_id), shlex.quote(out), args.max_seconds))
        receipt = {'host_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'boot_id': boot_id,
                   'device_output_dir': out, 'probe_sha256': hashlib.sha256(probe).hexdigest(),
                   'max_seconds': args.max_seconds, 'start_stdout': start.stdout.decode().strip(),
                   'device_mutations': ['/run/y2/suspend-probe.py (tmpfs)', out + ' (captures)', '/run/y2/suspend-probe.log (tmpfs)'],
                   'reads_only': True}
        (args.receipt / 'armed.json').write_text(json.dumps(receipt, indent=2) + '\n')
        print(json.dumps(receipt, indent=2))


if __name__ == '__main__':
    main()
