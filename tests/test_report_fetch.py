"""Error reports R3: tools/reports/fetch.py signs like AWS documents and only writes report files."""
import datetime
import hashlib
import importlib.util
import io
import json
import re
import tempfile
import unittest
import urllib.parse
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
_spec = importlib.util.spec_from_file_location('report_fetch', ROOT / 'tools/reports/fetch.py')
fetch = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(fetch)

EXAMPLE_KEY, EXAMPLE_SECRET = 'AKIAIOSFODNN7EXAMPLE', 'wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY'


def signature(header):
    return re.search(r'Signature=([0-9a-f]{64})$', header).group(1)


class SignatureV4(unittest.TestCase):
    """Published examples: the AWS SigV4 test suite (get-vanilla) and the S3 API reference."""

    def test_get_vanilla(self):
        header = fetch.authorization(
            'GET', 'https://example.amazonaws.com/',
            {'host': 'example.amazonaws.com', 'x-amz-date': '20150830T123600Z'}, fetch.EMPTY_SHA256,
            'AKIDEXAMPLE', 'wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY', 'us-east-1', 'service', '20150830T123600Z')
        self.assertEqual(header, 'AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, '
                                 'SignedHeaders=host;x-amz-date, '
                                 'Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31')

    def test_s3_get_object(self):
        header = fetch.authorization(
            'GET', 'https://examplebucket.s3.amazonaws.com/test.txt',
            {'host': 'examplebucket.s3.amazonaws.com', 'range': 'bytes=0-9',
             'x-amz-content-sha256': fetch.EMPTY_SHA256, 'x-amz-date': '20130524T000000Z'},
            fetch.EMPTY_SHA256, EXAMPLE_KEY, EXAMPLE_SECRET, 'us-east-1', 's3', '20130524T000000Z')
        self.assertIn('SignedHeaders=host;range;x-amz-content-sha256;x-amz-date', header)
        self.assertEqual(signature(header), 'f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41')

    def test_s3_list_objects(self):
        header = fetch.authorization(
            'GET', 'https://examplebucket.s3.amazonaws.com/?max-keys=2&prefix=J',
            {'host': 'examplebucket.s3.amazonaws.com',
             'x-amz-content-sha256': fetch.EMPTY_SHA256, 'x-amz-date': '20130524T000000Z'},
            fetch.EMPTY_SHA256, EXAMPLE_KEY, EXAMPLE_SECRET, 'us-east-1', 's3', '20130524T000000Z')
        self.assertEqual(signature(header), '34b48302e7b5fa45bde8084f4b7868a86f0a534bc59db6670ed5711ef69dc6f7')


class Response(io.BytesIO):
    def __init__(self, data, headers=None):
        super().__init__(data)
        self.headers = headers or {}


class FakeR2:
    """Answers ListObjectsV2 (two pages) and GetObject from a dict; records requests."""

    def __init__(self, objects):
        self.objects = objects
        self.requests = []

    def open(self, request, timeout):
        self.requests.append(request)
        url = urllib.parse.urlsplit(request.full_url)
        query = dict(urllib.parse.parse_qsl(url.query))
        if query.get('list-type') == '2':
            keys = sorted(k for k in self.objects if k.startswith(query['prefix']))
            start = int(query.get('continuation-token', '0'))
            page = keys[start:start + 2]
            more = start + 2 < len(keys)
            items = ''.join('<Contents><Key>%s</Key><Size>%d</Size></Contents>' % (k, len(self.objects[k][0]))
                            for k in page)
            xml = ('<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">%s'
                   '<IsTruncated>%s</IsTruncated>%s</ListBucketResult>' % (
                       items, 'true' if more else 'false',
                       '<NextContinuationToken>%d</NextContinuationToken>' % (start + 2) if more else ''))
            return Response(xml.encode())
        key = urllib.parse.unquote(url.path.split('/', 2)[2])
        data, digest = self.objects[key]
        return Response(data, {'x-amz-meta-sha256': digest})


CONFIG = {'account_id': 'acc123', 'access_key_id': 'AK', 'secret_access_key': 'SK', 'bucket': 'y2linux-reports'}
NOW = lambda: datetime.datetime(2026, 10, 19, 12, 0, tzinfo=datetime.timezone.utc)


def stored(data):
    return data, hashlib.sha256(data).hexdigest()


class Fetch(unittest.TestCase):

    def test_new_reports_are_downloaded_once_into_day_install_report_folders(self):
        objects = {
            'reports/2026-10-19/i1/r1.tar.gz': stored(b'\x1f\x8bone'),
            'reports/2026-10-19/i1/r2.tar.gz': stored(b'\x1f\x8btwo'),
            'reports/2026-10-20/i2/r3.tar.gz': stored(b'\x1f\x8bthree'),
        }
        r2 = FakeR2(objects)
        bucket = fetch.Bucket(CONFIG, opener=r2, now=NOW)
        with tempfile.TemporaryDirectory() as tmp:
            keys = list(bucket.keys())
            self.assertEqual([k for k, _ in keys], sorted(objects))      # both list pages
            for key, _ in keys:
                fetch.download(bucket, key, fetch.target_for(tmp, key))
            self.assertEqual((Path(tmp) / '2026-10-20/i2/r3.tar.gz').read_bytes(), b'\x1f\x8bthree')
            self.assertEqual(oct((Path(tmp) / '2026-10-20/i2/r3.tar.gz').stat().st_mode & 0o777), '0o600')
            self.assertEqual(sorted(p.name for p in Path(tmp).rglob('*') if p.is_file()),
                             ['r1.tar.gz', 'r2.tar.gz', 'r3.tar.gz'])           # no .part left
        first = r2.requests[0]
        self.assertEqual(urllib.parse.urlsplit(first.full_url).netloc, 'acc123.r2.cloudflarestorage.com')
        self.assertTrue(first.full_url.startswith('https://acc123.r2.cloudflarestorage.com/y2linux-reports/'))
        self.assertIn('Credential=AK/20261019/auto/s3/aws4_request', first.get_header('Authorization'))
        self.assertEqual(first.get_header('X-amz-date'), '20261019T120000Z')
        self.assertTrue(all(r.get_method() == 'GET' for r in r2.requests))   # read-only client

    def test_a_report_whose_hash_differs_from_the_workers_record_is_not_written(self):
        r2 = FakeR2({'reports/2026-10-19/i1/r1.tar.gz': (b'\x1f\x8bchanged', 'f' * 64)})
        bucket = fetch.Bucket(CONFIG, opener=r2, now=NOW)
        with tempfile.TemporaryDirectory() as tmp:
            target = fetch.target_for(tmp, 'reports/2026-10-19/i1/r1.tar.gz')
            with self.assertRaises(ValueError):
                fetch.download(bucket, 'reports/2026-10-19/i1/r1.tar.gz', target)
            self.assertFalse(any(Path(tmp).rglob('*.tar.gz')))

    def test_only_report_shaped_keys_map_to_a_file_inside_the_folder(self):
        self.assertEqual(fetch.target_for('/x', 'reports/2026-10-19/i/r.tar.gz'), Path('/x/2026-10-19/i/r.tar.gz'))
        for key in ('reports/../../etc/passwd.tar.gz', 'reports/2026-10-19/../r.tar.gz', 'other/a/b/c.tar.gz',
                    'reports/2026-10-19/i/r.txt', 'reports/2026-10-19/i/.hidden.tar.gz', 'reports/a/b/c/d.tar.gz',
                    'reports//i/r.tar.gz'):
            with self.subTest(key=key):
                self.assertIsNone(fetch.target_for('/x', key))

    def test_flat_names_match_the_r2_dashboard_download(self):
        self.assertEqual(fetch.target_for('/x', 'reports/2026-10-19/i/r.tar.gz', flat=True),
                         Path('/x/reports_2026-10-19_i_r.tar.gz'))
        self.assertIsNone(fetch.target_for('/x', 'reports/../i/r.tar.gz', flat=True))

    def test_pending_lists_reports_the_ledger_does_not_name(self):
        with tempfile.TemporaryDirectory() as tmp:
            names = ['reports_2026-10-20_i1_e37c8ebb-e8a6-4a2e-852f-473961c79d7d.tar.gz',
                     'reports_2026-10-20_i1_19a41489-0d2d-4dfd-b817-c2a9f4ed5eca.tar.gz',
                     'y2linux-report-20261019-153610.tar.gz']
            for name in names:
                (Path(tmp) / name).write_bytes(b'\x1f\x8b')
            (Path(tmp) / '2026-10-20/i2').mkdir(parents=True)
            (Path(tmp) / '2026-10-20/i2/81c5d87a-5bbe-4d46-944a-c4f832b4e931.tar.gz').write_bytes(b'\x1f\x8b')
            self.assertEqual(len(fetch.pending(tmp)), 4)                    # no ledger yet
            (Path(tmp) / 'REVIEWED.md').write_text('| e37c8ebb | ... |\n| y2linux-report-20261019-153610 |\n'
                                                   '| 81c5d87a |\n')
            self.assertEqual([p.name for p in fetch.pending(tmp)], [names[1]])

    def test_credentials_must_be_private_and_complete(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'r2.json'
            path.write_text(json.dumps(CONFIG))
            path.chmod(0o644)
            with self.assertRaises(SystemExit):
                fetch.load_config(path)
            path.chmod(0o600)
            self.assertEqual(fetch.load_config(path)['bucket'], 'y2linux-reports')
            path.write_text(json.dumps({'account_id': 'a'}))
            with self.assertRaises(SystemExit):
                fetch.load_config(path)


if __name__ == '__main__':
    unittest.main()
