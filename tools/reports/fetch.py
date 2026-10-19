#!/usr/bin/env python3
"""Download new error reports from the private R2 bucket (ERROR-REPORTS-PLAN, R3).

The Worker only stores; this is the one way reports are read. It lists
`reports/` with the S3 API (AWS Signature Version 4, standard library only, so
it runs on the PC or the home server without packages) and downloads every
report that is not in the target folder yet, checked against the SHA-256 the
Worker recorded. Layout: <into>/<YYYY-MM-DD>/<install-id>/<report-id>.tar.gz.

Credentials: an R2 API token with Object Read on the bucket, in
~/.config/y2linux/r2-reports.json (mode 600):
  {"account_id": "...", "access_key_id": "...", "secret_access_key": "...", "bucket": "y2linux-reports"}
See tools/reports/README.md.
"""
# SPDX-License-Identifier: GPL-2.0-only
import argparse
import datetime
import hashlib
import hmac
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
from pathlib import Path

CONFIG = Path.home() / '.config/y2linux/r2-reports.json'
PREFIX = 'reports/'
S3 = '{http://s3.amazonaws.com/doc/2006-03-01/}'
EMPTY_SHA256 = hashlib.sha256(b'').hexdigest()


def _quote(text, safe='-_.~'):
    return urllib.parse.quote(text, safe=safe)


def _hmac(key, text):
    return hmac.new(key, text.encode(), hashlib.sha256).digest()


def authorization(method, url, headers, payload_sha256, access_key, secret_key, region, service, amz_date):
    """The SigV4 Authorization header for a request whose headers are all signed.

    `headers` must hold host and x-amz-date (amz_date, YYYYMMDDTHHMMSSZ)."""
    parts = urllib.parse.urlsplit(url)
    path = _quote(parts.path or '/', safe='-_.~/')
    query = '&'.join('%s=%s' % (_quote(k), _quote(v))
                     for k, v in sorted(urllib.parse.parse_qsl(parts.query, keep_blank_values=True)))
    canonical = {name.lower(): ' '.join(str(value).split()) for name, value in headers.items()}
    signed = ';'.join(sorted(canonical))
    request = '\n'.join([method, path, query,
                         ''.join('%s:%s\n' % (name, canonical[name]) for name in sorted(canonical)),
                         signed, payload_sha256])
    day = amz_date[:8]
    scope = '%s/%s/%s/aws4_request' % (day, region, service)
    to_sign = '\n'.join(['AWS4-HMAC-SHA256', amz_date, scope, hashlib.sha256(request.encode()).hexdigest()])
    key = _hmac(_hmac(_hmac(_hmac(('AWS4' + secret_key).encode(), day), region), service), 'aws4_request')
    signature = hmac.new(key, to_sign.encode(), hashlib.sha256).hexdigest()
    return 'AWS4-HMAC-SHA256 Credential=%s/%s, SignedHeaders=%s, Signature=%s' % (access_key, scope, signed, signature)


class Bucket:
    """GET-only S3 client for one R2 bucket."""

    def __init__(self, config, opener=None, now=None):
        self.account = config['account_id']
        self.access_key = config['access_key_id']
        self.secret_key = config['secret_access_key']
        self.bucket = config.get('bucket') or 'y2linux-reports'
        self.host = '%s.r2.cloudflarestorage.com' % self.account
        self.opener = opener or urllib.request.build_opener(urllib.request.ProxyHandler({}))
        self.now = now or (lambda: datetime.datetime.now(datetime.timezone.utc))

    def get(self, key='', query=None):
        url = 'https://%s/%s/%s' % (self.host, _quote(self.bucket), _quote(key, safe='-_.~/'))
        if query:
            url += '?' + urllib.parse.urlencode(sorted(query.items()), quote_via=urllib.parse.quote)
        amz_date = self.now().strftime('%Y%m%dT%H%M%SZ')
        headers = {'host': self.host, 'x-amz-content-sha256': EMPTY_SHA256, 'x-amz-date': amz_date}
        headers['Authorization'] = authorization('GET', url, headers, EMPTY_SHA256, self.access_key,
                                                 self.secret_key, 'auto', 's3', amz_date)
        request = urllib.request.Request(url, headers={k: v for k, v in headers.items() if k != 'host'})
        return self.opener.open(request, timeout=60)

    def keys(self, prefix=PREFIX):
        """(key, size) of every object below prefix."""
        token = None
        while True:
            query = {'list-type': '2', 'prefix': prefix}
            if token:
                query['continuation-token'] = token
            with self.get(query=query) as response:
                root = ET.fromstring(response.read())
            for item in root.iter(S3 + 'Contents'):
                yield item.findtext(S3 + 'Key'), int(item.findtext(S3 + 'Size') or 0)
            if root.findtext(S3 + 'IsTruncated') != 'true':
                return
            token = root.findtext(S3 + 'NextContinuationToken')


def target_for(into, key):
    """reports/<day>/<install>/<report>.tar.gz below into, refusing anything else."""
    parts = key.split('/')
    if (len(parts) != 4 or parts[0] != 'reports' or not parts[3].endswith('.tar.gz')
            or any(p in ('', '.', '..') or p.startswith('.') for p in parts)):
        return None
    return Path(into, *parts[1:])


def download(bucket, key, target):
    """Write one report atomically; its SHA-256 must match the Worker's record."""
    with bucket.get(key) as response:
        data = response.read()
        expected = response.headers.get('x-amz-meta-sha256')
    if expected and hashlib.sha256(data).hexdigest() != expected:
        raise ValueError('sha256_mismatch:' + key)
    target.parent.mkdir(parents=True, exist_ok=True)
    temporary = target.with_name('.' + target.name + '.part')
    with open(temporary, 'wb') as stream:
        stream.write(data)
    os.replace(temporary, target)
    return len(data)


def load_config(path):
    path = Path(path)
    if path.stat().st_mode & 0o077:
        raise SystemExit('%s must be readable by its owner only (chmod 600)' % path)
    value = json.loads(path.read_text())
    missing = {'account_id', 'access_key_id', 'secret_access_key'} - set(value)
    if missing:
        raise SystemExit('%s lacks %s' % (path, ', '.join(sorted(missing))))
    return value


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--into', type=Path, default=Path(__file__).resolve().parents[2] / 'out/reports')
    parser.add_argument('--config', type=Path, default=CONFIG)
    parser.add_argument('--list', action='store_true', help='only list what is in the bucket')
    args = parser.parse_args(argv)
    bucket = Bucket(load_config(args.config))
    new = failed = 0
    try:
        for key, size in bucket.keys():
            target = target_for(args.into, key)
            if args.list:
                print('%9d  %s%s' % (size, key, '' if target and target.exists() else '  (new)'))
                continue
            if target is None:
                print('skipped (unexpected name): %s' % key, file=sys.stderr)
                continue
            if target.exists():
                continue
            try:
                download(bucket, key, target)
                new += 1
                print(target)
            except (ValueError, urllib.error.URLError) as error:
                failed += 1
                print('failed: %s (%s)' % (key, error), file=sys.stderr)
    except urllib.error.HTTPError as error:
        raise SystemExit('R2 answered %d: check the token and the bucket name' % error.code)
    if not args.list:
        print('%d new report(s) in %s' % (new, args.into), file=sys.stderr)
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
