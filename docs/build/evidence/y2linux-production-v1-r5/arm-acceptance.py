"""Run the retained ARM shell and ABI probe, without device access."""
from pathlib import Path
import subprocess

root = Path('/build/buildroot/target')
qemu = ['qemu-arm', '-cpu', 'cortex-a7', '-L', str(root)]
shell = qemu + [str(root/'bin/busybox'), 'sh']
old = subprocess.run(shell+['-c', 'set -i; echo UNREACHABLE'], text=True, capture_output=True)
assert old.returncode == 2 and 'illegal option -i' in old.stderr and not old.stdout
print('PASS reproduced Storage04 handover blocker: exit=2; '+old.stderr.strip())
scripts = [Path('/project/initramfs/production/init'), Path('/project/initramfs/production/storage.sh')]
overlay = Path('/project/buildroot/board/y2/production-overlay')
scripts += sorted((overlay/'etc/init.d').glob('*')) + sorted((overlay/'usr/sbin').glob('*'))
for script in scripts:
    subprocess.run(shell+['-n', str(script)], check=True)
print('PASS actual ARM BusyBox syntax for production rescue, resolver and '+str(len(scripts)-2)+' root scripts')
result = subprocess.run(qemu+['/build/y2-abi-check'], text=True, capture_output=True, check=True)
print(result.stdout.strip())
print('PASS retained ARM ABI probe')
