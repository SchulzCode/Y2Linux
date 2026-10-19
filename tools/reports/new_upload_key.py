#!/usr/bin/env python3
"""Make the error report upload key, once (tools/reports/README.md).

The same key goes into two places: the Worker's UPLOAD_KEY secret and every
release image (tools/production/build.py reads it from KEY_FILE). It is public
in the firmware and only keeps random traffic away from the receiver.

  python3 tools/reports/new_upload_key.py              # make it (refuses to replace one)
  python3 tools/reports/new_upload_key.py --print | npx wrangler secret put UPLOAD_KEY
"""
# SPDX-License-Identifier: GPL-2.0-only
import argparse
import json
import os
import secrets
import sys
from pathlib import Path

KEY_FILE = Path.home() / '.local/share/y2linux/report-upload.json'


def make(path, replace=False):
    path = Path(path)
    if path.exists() and not replace:
        raise SystemExit('%s exists; --replace makes a new key (then set the Worker secret again '
                         'and build a release, or players lose uploads until they update)' % path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name('.' + path.name + '.part')
    temporary.unlink(missing_ok=True)
    fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'w') as stream:
        json.dump({'upload_key': secrets.token_urlsafe(32)}, stream)
    os.replace(temporary, path)
    return path


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--file', type=Path, default=KEY_FILE)
    parser.add_argument('--print', action='store_true', help='write the existing key to stdout (for wrangler)')
    parser.add_argument('--replace', action='store_true')
    args = parser.parse_args(argv)
    if args.print:
        sys.stdout.write(json.loads(args.file.read_text())['upload_key'])
        return 0
    print('made %s (mode 600)' % make(args.file, args.replace), file=sys.stderr)
    print('next, in tools/reports/worker: python3 ../new_upload_key.py --print | npx wrangler secret put UPLOAD_KEY',
          file=sys.stderr)
    return 0


if __name__ == '__main__':
    sys.exit(main())
