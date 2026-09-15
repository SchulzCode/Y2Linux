#!/usr/bin/env python3
"""Stage explicitly supplied owner firmware; never fetch it or open a device.

The firmware manifest is the completed M5 inventory. Optional stock default
records are extracted from the exact same product's libcustom_nvram.so. They
are generic board parameters, not a replacement for device factory records.
No default address is allowed into the resulting files.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import struct

PROJECT = Path(__file__).resolve().parents[2]
INVENTORY = PROJECT / 'docs/knowledge/evidence/m5-firmware-inventory.json'
CUSTOM_SHA = 'c6460e9649f539b56449dc19f8d20771402f539be9e73340a74a9ccf45287397'
CUSTOM_SIZE = 51224
DEFAULTS = {
    'WIFI.defaults': (512, 'eb5e87224e3a493e825179f547fc13924f564a42730ea821b4359b5b36f4defe'),
    'BT.defaults': (64, 'f224ff4abfb45a612ef03249e87e4ecfa44b7c1f9f19ee3cc78ae354764d9472'),
}


def verify_provision(directory):
    """Accept only the reviewed seven owner files, never a calibration tree."""
    inventory = json.loads(INVENTORY.read_text())
    files = {x['filename']: (x['bytes'], x['sha256']) for x in inventory['files']}
    files.update(DEFAULTS)
    for name, (size, sha) in files.items():
        checked(directory/name, size, sha)
    return files


def install_provision(directory, target):
    files = verify_provision(directory)
    destination = target/'lib/firmware/mediatek/mt6582'
    destination.mkdir(parents=True, exist_ok=True)
    for name, (size, sha) in files.items():
        data = checked(directory/name, size, sha)
        path = destination/name
        if path.is_symlink():
            raise ValueError('firmware destination is a symlink')
        path.write_bytes(data)
        path.chmod(0o600)
    return files


def regular(path, size):
    fd = os.open(path, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode) or st.st_size != size:
            raise ValueError('wrong input type/size: ' + Path(path).name)
        chunks = []
        remaining = size
        while remaining:
            part = os.read(fd, remaining)
            if not part:
                raise ValueError('short owner input')
            chunks.append(part)
            remaining -= len(part)
        if os.read(fd, 1):
            raise ValueError('owner input grew during read')
        return b''.join(chunks)
    finally:
        os.close(fd)


def checked(path, size, digest):
    data = regular(path, size)
    if hashlib.sha256(data).hexdigest() != digest:
        raise ValueError('owner input hash mismatch: ' + Path(path).name)
    return data


def defaults(path):
    data = checked(path, CUSTOM_SIZE, CUSTOM_SHA)
    # Exact stock ELF32 LE PT_LOAD translation: VA >= 0x2d4c is file +0x1000.
    # g_akCFG_File_Custom[4]: WIFI, version 000, record size 512, count 1.
    # nvram_bt_default_value(0x6582): 0x132e PC-add -> VA 0x4a18, 64 bytes.
    if data[:7] != b'\x7fELF\x01\x01\x01':
        raise ValueError('stock reader ELF format')
    wifi = bytearray(data[0x5a8e:0x5c8e])
    bt = bytearray(data[0x3a18:0x3a58])
    if struct.unpack_from('<HH', wifi) != (0x104, 0) or wifi[196:198] != b'\x01\x00':
        raise ValueError('stock Wi-Fi descriptor/default disagreement')
    wifi[4:10] = bytes(6)
    bt[:6] = bytes(6)
    return {'WIFI.defaults': bytes(wifi), 'BT.defaults': bytes(bt)}


def provision(source, destination, custom=None):
    inventory = json.loads(INVENTORY.read_text())
    staged = {}
    for entry in inventory['files']:
        name = entry['filename']
        staged[name] = checked(source / name, entry['bytes'], entry['sha256'])
    # Validate the actual patch headers, not filename sorting.
    patches = []
    for name, data in staged.items():
        if not name.endswith('_hdr.bin'):
            continue
        count, sequence = data[24] >> 4, data[24] & 15
        if count != 2 or sequence not in (1, 2) or data[18:20] != b'PS':
            raise ValueError('patch header/order mismatch')
        patches.append((sequence, name))
    if sorted(seq for seq, _ in patches) != [1, 2]:
        raise ValueError('incomplete patch set')
    if custom:
        staged.update(defaults(custom))
    if destination.exists():
        raise ValueError('fresh owner staging directory required')
    destination.mkdir(parents=True, mode=0o700)
    receipt = {'schema': 'org.schulzcode.y2linux.owner-radio-firmware/v1',
               'redistribution_permission_established': False,
               'owner_supplied': True, 'contains_device_calibration': False,
               'contains_radio_addresses': False,
               'patch_order': [name for _, name in sorted(patches)],
               'default_record_source_sha256': CUSTOM_SHA if custom else None,
               'files': []}
    for name, data in staged.items():
        path = destination / name
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        with os.fdopen(fd, 'wb') as output:
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
        receipt['files'].append({'filename': name, 'bytes': len(data),
                                 'sha256': hashlib.sha256(data).hexdigest()})
    path = destination / 'owner-provisioning.json'
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'w') as output:
        json.dump(receipt, output, indent=2)
        output.write('\n')
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--owner-firmware', type=Path, required=True)
    parser.add_argument('--stock-custom-nvram', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    receipt = provision(args.owner_firmware, args.output, args.stock_custom_nvram)
    print('Verified owner-local firmware:', len(receipt['files']), 'files')
    print('Redistribution permission is not established; this is an owner-local input.')


if __name__ == '__main__':
    main()
