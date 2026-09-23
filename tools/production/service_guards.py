"""Apply the maintenance hold to the pinned BlueZ init service."""
from pathlib import Path
import re
import sys


def bluetooth_maintenance_guard(source):
    # Anchor the declaration: the suffix of restart() is also 'start()'.
    starts = list(re.finditer(r'^start\(\) \{\n', source, re.MULTILINE))
    if len(starts) != 1:
        raise ValueError('unknown bluetoothd service structure')
    guard = '\t[ ! -e /data/system/platform/maintenance-pending ] || return 1\n'
    offset = starts[0].end()
    if source[offset:].startswith(guard):
        return source
    if guard in source:
        raise ValueError('misplaced bluetoothd maintenance guard')
    return source[:offset] + guard + source[offset:]


if __name__ == '__main__':
    path = Path(sys.argv[1])
    path.write_text(bluetooth_maintenance_guard(path.read_text()))
