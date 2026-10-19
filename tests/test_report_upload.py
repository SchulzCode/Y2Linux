"""Error reports R4: the uploader sends queued reports to the receiver only when it should.

A local HTTPS server with a throwaway certificate stands in for the Worker
(tools/reports/worker answers the same way; its own tests run under node)."""
import datetime
import gzip
import hashlib
import http.server
import json
import os
import re
import shutil
import ssl
import subprocess
import sys
import tempfile
import threading
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools/platform'))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from y2_platform import report, service
from y2_platform.common import Context

from test_error_reports import player, runner, write

ROOT = Path(__file__).resolve().parents[1]
KEY = 'k' * 43
NOW = datetime.datetime(2026, 10, 19, 12, tzinfo=datetime.timezone.utc)


class Receiver(http.server.BaseHTTPRequestHandler):
    """Records every request and answers from the server's script."""

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get('Content-Length', 0)))
        self.server.received.append((self.path, {k.lower(): v for k, v in self.headers.items()}, body))
        status, answer, extra = (self.server.script.pop(0) if self.server.script
                                 else (201, {'status': 'stored'}, {}))
        if status in (200, 201):
            answer = dict(answer, report_id=self.headers.get('X-Y2-Report-Id'))
        data = json.dumps(answer).encode()
        self.send_response(status)
        for name, value in extra.items():
            self.send_header(name, value)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *args):
        pass


@unittest.skipUnless(shutil.which('openssl'), 'needs openssl for the throwaway certificate')
class Upload(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.certs = tempfile.TemporaryDirectory()
        cls.cert = Path(cls.certs.name) / 'cert.pem'
        key = Path(cls.certs.name) / 'key.pem'
        subprocess.run(['openssl', 'req', '-x509', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:prime256v1',
                        '-nodes', '-keyout', str(key), '-out', str(cls.cert), '-days', '1', '-subj', '/CN=127.0.0.1',
                        '-addext', 'subjectAltName=IP:127.0.0.1'], check=True, capture_output=True)
        cls.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Receiver)
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(cls.cert, key)
        cls.server.socket = context.wrap_socket(cls.server.socket, server_side=True)
        cls.server.received, cls.server.script = [], []
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.certs.cleanup()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = self.temp.name
        player(self.root)
        self.ctx = Context(self.root, runner=runner)
        write(self.root, report.UPLOAD_KEY, json.dumps({'upload_key': KEY}))
        self.online()
        self.battery(80)
        self.server.received.clear()
        self.server.script.clear()
        self.endpoint = mock.patch.object(report, 'ENDPOINT',
                                          'https://127.0.0.1:%d/v1/report' % self.server.server_address[1])
        self.endpoint.start()
        self.clock = mock.patch('y2_platform.timekeeping.status', return_value={'tls_ready': True})
        self.clock.start()
        trusted = ssl.create_default_context(cafile=str(self.cert))
        self.opener = report._opener(trusted)

    def tearDown(self):
        self.clock.stop()
        self.endpoint.stop()
        self.temp.cleanup()

    def online(self, state='Online', boot='boot-1'):
        write(self.root, '/run/y2/network.json', json.dumps({'state': state, 'boot_id': boot}))

    def battery(self, percent, charging=False):
        write(self.root, '/run/y2/power.json', json.dumps({'boot_id': 'boot-1', 'battery': {
            'percent': percent, 'charging': charging, 'status': 'Charging' if charging else 'Discharging'}}))

    def queued(self, trigger='manual', when=NOW):
        result = report.queue(self.ctx, trigger, now=when)
        self.assertTrue(result['queued'], result)
        return result

    def send(self, now):
        return report.upload(self.ctx, now=now.timestamp(), opener=self.opener)

    def test_a_queued_report_is_sent_with_its_identity_and_leaves_the_queue(self):
        install = report.set_enabled(self.ctx, True)['install_id']
        item = self.queued()
        body = (Path(self.root) / report.QUEUE.lstrip('/') / item['name']).read_bytes()
        result = self.send(NOW)
        self.assertEqual(result['uploaded'], [{'name': item['name'], 'outcome': 'sent', 'report_id': item['report_id']}])
        [(path, headers, received)] = self.server.received
        self.assertEqual(path, '/v1/report')
        self.assertEqual(received, body)
        self.assertEqual(received[:2], b'\x1f\x8b')
        self.assertEqual(headers['x-y2-upload-key'], KEY)
        self.assertEqual(headers['x-y2-schema'], '1')
        self.assertEqual(headers['x-y2-install-id'], install)
        self.assertEqual(headers['x-y2-report-id'], item['report_id'])
        self.assertEqual(headers['x-y2-sha256'], hashlib.sha256(body).hexdigest())
        self.assertEqual(headers['x-y2-trigger'], 'manual')
        self.assertEqual(headers['content-type'], 'application/gzip')
        self.assertEqual(int(headers['content-length']), len(body))
        status = report.status(self.ctx)
        self.assertEqual(status['queued'], [])
        self.assertEqual(status['last']['state'], 'sent')
        self.assertTrue(status['upload'])
        self.assertIsNone(report.next_upload(self.ctx))

    def test_without_an_upload_key_nothing_is_sent_and_reborn_offers_only_save(self):
        report.set_enabled(self.ctx, True)
        self.queued()
        (Path(self.root) / report.UPLOAD_KEY.lstrip('/')).write_text(json.dumps({'upload_key': 'short'}))
        self.assertFalse(report.status(self.ctx)['upload'])
        self.assertEqual(self.send(NOW)['reason'], 'no_upload_key')
        (Path(self.root) / report.UPLOAD_KEY.lstrip('/')).unlink()
        self.assertFalse(report.status(self.ctx)['upload'])
        self.assertEqual(self.send(NOW)['reason'], 'no_upload_key')
        self.assertEqual(self.server.received, [])
        self.assertEqual(len(report.status(self.ctx)['queued']), 1)

    def test_reports_wait_for_opt_in_wifi_clock_battery_and_no_update(self):
        self.assertEqual(self.send(NOW)['reason'], 'not_enabled')
        report.set_enabled(self.ctx, True)
        self.queued()
        cases = [
            (lambda: self.online('Connecting'), 'offline'),
            (lambda: self.online('Online', boot='boot-0'), 'offline'),           # an earlier boot's record
            (lambda: self.battery(14), 'battery_low'),
            (lambda: write(self.root, '/run/y2/power.json', '{}'), 'battery_low'),   # unknown: wait
            (lambda: write(self.root, '/data/updates/state.json', json.dumps({'state': 'Queued'})), 'update_in_progress'),
            (lambda: write(self.root, '/data/system/platform/maintenance-pending', ''), 'update_in_progress'),
        ]
        def ready():
            self.online()
            self.battery(80)
            write(self.root, '/data/updates/state.json', json.dumps({'state': 'Acknowledged'}))
            (Path(self.root) / 'data/system/platform/maintenance-pending').unlink(missing_ok=True)
        for change, reason in cases:
            with self.subTest(reason=reason):
                ready()
                change()
                self.assertEqual(self.send(NOW)['reason'], reason)
        ready()
        with mock.patch('y2_platform.timekeeping.status', return_value={'tls_ready': False}):
            self.assertEqual(self.send(NOW)['reason'], 'clock_not_ready')
        self.assertEqual(self.server.received, [])
        # Charging overrides a low battery.
        self.battery(5, charging=True)
        self.assertEqual(self.send(NOW)['uploaded'][0]['outcome'], 'sent')

    def test_server_errors_back_off_and_stop_the_run_then_succeed(self):
        report.set_enabled(self.ctx, True)
        first = self.queued('manual', NOW)
        self.queued('manual', NOW + datetime.timedelta(minutes=1))
        self.server.script[:] = [(503, {'error': 'busy'}, {})]
        result = self.send(NOW)
        self.assertEqual([r['outcome'] for r in result['uploaded']], ['retry'])   # the second was not tried
        self.assertEqual(len(self.server.received), 1)
        self.assertEqual(report.status(self.ctx)['last']['state'], 'queued')       # the last report is the second
        due = report.next_upload(self.ctx)
        self.assertEqual(due, 0)                                                    # the second never failed
        retry = report.state(self.ctx)['uploads'][first['name']]
        self.assertEqual((retry['attempts'], retry['next'], retry['error']),
                         (1, NOW.timestamp() + report.RETRY_S[0], 'HTTP_503'))
        # A later run sends the untried one; the failed one waits for its time.
        result = self.send(NOW + datetime.timedelta(seconds=10))
        self.assertEqual([r['outcome'] for r in result['uploaded']], ['sent'])
        self.assertEqual(report.next_upload(self.ctx), NOW.timestamp() + report.RETRY_S[0])
        result = self.send(NOW + datetime.timedelta(seconds=report.RETRY_S[0]))
        self.assertEqual([r['outcome'] for r in result['uploaded']], ['sent'])
        self.assertEqual(report.status(self.ctx)['queued'], [])
        self.assertEqual(report.state(self.ctx)['uploads'], {})

    def test_the_last_reports_failure_shows_and_backoff_grows(self):
        report.set_enabled(self.ctx, True)
        item = self.queued()
        self.server.script[:] = [(500, {}, {}), (502, {}, {})]
        self.send(NOW)
        last = report.status(self.ctx)['last']
        self.assertEqual((last['state'], last['reason'], last['report_id']), ('failed', 'HTTP_500', item['report_id']))
        self.send(NOW + datetime.timedelta(seconds=report.RETRY_S[0]))
        retry = report.state(self.ctx)['uploads'][item['name']]
        self.assertEqual(retry['attempts'], 2)
        self.assertEqual(retry['next'], NOW.timestamp() + report.RETRY_S[0] + report.RETRY_S[1])

    def test_a_full_day_waits_for_the_next_utc_day(self):
        report.set_enabled(self.ctx, True)
        item = self.queued()
        self.server.script[:] = [(429, {'error': 'install_daily_limit'}, {})]
        self.send(NOW)
        tomorrow = datetime.datetime(2026, 10, 20, 0, 5, tzinfo=datetime.timezone.utc).timestamp()
        self.assertEqual(report.state(self.ctx)['uploads'][item['name']]['next'], tomorrow)

    def test_a_refused_report_is_dropped_and_says_why(self):
        report.set_enabled(self.ctx, True)
        self.queued()
        self.server.script[:] = [(400, {'error': 'sha256_mismatch'}, {})]
        result = self.send(NOW)
        self.assertEqual(result['uploaded'][0]['reason'], 'refused_400_sha256_mismatch')
        self.assertEqual(report.status(self.ctx)['queued'], [])
        last = report.status(self.ctx)['last']
        self.assertEqual((last['state'], last['reason']), ('dropped', 'refused_400_sha256_mismatch'))

    def test_a_report_unsent_for_three_days_is_dropped_without_sending(self):
        report.set_enabled(self.ctx, True)
        self.queued()
        result = self.send(NOW + datetime.timedelta(seconds=report.GIVE_UP_S + 1))
        self.assertEqual(result['uploaded'][0]['reason'], 'expired')
        self.assertEqual(self.server.received, [])
        self.assertEqual(report.status(self.ctx)['last']['state'], 'dropped')

    def test_a_redirect_is_never_followed(self):
        report.set_enabled(self.ctx, True)
        item = self.queued()
        self.server.script[:] = [(307, {}, {'Location': 'https://127.0.0.1:%d/elsewhere' % self.server.server_address[1]})]
        result = self.send(NOW)
        self.assertEqual(result['uploaded'][0]['outcome'], 'retry')
        self.assertEqual([path for path, _, _ in self.server.received], ['/v1/report'])
        self.assertEqual(len(report.status(self.ctx)['queued']), 1)
        self.assertEqual(report.state(self.ctx)['uploads'][item['name']]['error'], 'HTTP_307')

    def test_an_untrusted_certificate_sends_nothing(self):
        report.set_enabled(self.ctx, True)
        self.queued()
        result = report.upload(self.ctx, now=NOW.timestamp(), opener=report._opener(ssl.create_default_context()))
        self.assertEqual(result['uploaded'][0]['outcome'], 'retry')
        self.assertEqual(result['uploaded'][0]['reason'], 'SSLCertVerificationError')
        self.assertEqual(self.server.received, [])

    def test_a_damaged_queue_file_is_dropped(self):
        report.set_enabled(self.ctx, True)
        write(self.root, report.QUEUE + '/20261019T120000Z-x.tar.gz', gzip.compress(b'not a tar'), 'wb')
        result = self.send(NOW)
        self.assertEqual(result['uploaded'], [{'name': '20261019T120000Z-x.tar.gz', 'outcome': 'dropped',
                                               'reason': 'unreadable'}])
        self.assertEqual(self.server.received, [])

    def test_only_one_upload_runs_at_a_time(self):
        import fcntl
        report.set_enabled(self.ctx, True)
        self.queued()
        lock = os.open(Path(self.root) / report.QUEUE.lstrip('/') / '.upload.lock', os.O_RDWR | os.O_CREAT, 0o600)
        try:
            fcntl.flock(lock, fcntl.LOCK_EX)
            self.assertEqual(self.send(NOW)['reason'], 'busy')
        finally:
            os.close(lock)
        self.assertEqual(self.server.received, [])

    def test_a_sleep_in_progress_leaves_the_report_queued(self):
        report.set_enabled(self.ctx, True)
        self.queued()
        with mock.patch.object(report, 'upload', side_effect=ValueError('sleep_operation_in_progress')):
            self.assertEqual(report.upload_now(self.ctx)['reason'], 'sleep_operation_in_progress')
        self.assertEqual(len(report.status(self.ctx)['queued']), 1)


class Service(unittest.TestCase):
    """The platform service starts `report work` in its own process, and only when due."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = self.temp.name
        player(self.root)
        self.ctx = Context(self.root, runner=runner)
        self.started = []
        process = mock.Mock()
        process.poll.return_value = 0
        self.process = process
        self.popen = mock.patch('subprocess.Popen', side_effect=lambda argv, **kw: self.started.append(argv) or process)
        self.popen.start()

    def tearDown(self):
        self.popen.stop()
        self.temp.cleanup()

    def tick(self, run, now, network='Online', wall=None):
        return service.error_reports(self.ctx, {'state': network}, run, now, wall=wall)

    def test_nothing_runs_without_an_upload_key_or_opt_in(self):
        report.set_enabled(self.ctx, True)
        run = self.tick(None, 0)
        self.assertEqual(self.tick(run, 10_000), run)
        self.assertEqual(self.started, [])
        write(self.root, report.UPLOAD_KEY, json.dumps({'upload_key': KEY}))
        report.set_enabled(self.ctx, False)
        self.tick(run, 10_000)
        self.assertEqual(self.started, [])

    def test_checks_run_after_start_then_every_period_and_never_twice_at_once(self):
        write(self.root, report.UPLOAD_KEY, json.dumps({'upload_key': KEY}))
        report.set_enabled(self.ctx, True)
        run = self.tick(None, 1000)
        self.assertEqual(self.started, [])                                   # not right at start
        run = self.tick(run, 1000 + service.REPORT_FIRST_S)
        self.assertEqual(self.started, [['/usr/sbin/y2-platform', 'report', 'work']])
        self.process.poll.return_value = None                                # still running
        run = self.tick(run, 1000 + service.REPORT_FIRST_S + service.REPORT_LIMIT_S - 1)
        self.assertEqual(len(self.started), 1)
        self.process.poll.return_value = 0
        run = self.tick(run, 1000 + service.REPORT_FIRST_S + service.REPORT_CHECK_S)
        self.assertEqual(len(self.started), 2)

    def test_a_due_report_is_sent_before_the_next_check_but_not_every_pass(self):
        write(self.root, report.UPLOAD_KEY, json.dumps({'upload_key': KEY}))
        report.set_enabled(self.ctx, True)
        run = self.tick(None, 1000)
        report.queue(self.ctx, 'manual', now=NOW)
        run = self.tick(run, 1030, network='Connecting')
        self.assertEqual(self.started, [])                                   # waits for Wi-Fi
        run = self.tick(run, 1060)
        self.assertEqual(len(self.started), 1)
        run = self.tick(run, 1090)
        self.assertEqual(len(self.started), 1)                               # blocked upload: not every pass
        run = self.tick(run, 1060 + service.REPORT_SEND_GAP_S)
        self.assertEqual(len(self.started), 2)
        # A report waiting for its retry time does not start a run.
        state = report.state(self.ctx)
        name = report.status(self.ctx)['queued'][0]
        state['uploads'] = {name: {'attempts': 1, 'next': NOW.timestamp() + 3600}}
        report.save(self.ctx, state)
        self.tick(run, 1060 + 2 * service.REPORT_SEND_GAP_S, wall=NOW.timestamp())
        self.assertEqual(len(self.started), 2)

    def test_a_broken_reports_record_never_stops_the_service_loop(self):
        write(self.root, report.UPLOAD_KEY, json.dumps({'upload_key': KEY}))
        write(self.root, report.STATE, json.dumps({'enabled': True, 'uploads': ['not', 'a', 'dict']}))
        write(self.root, report.QUEUE + '/a.tar.gz', 'x')
        run = self.tick(None, 0)
        self.tick(run, service.REPORT_FIRST_S)            # no exception

    def test_a_stuck_run_is_stopped(self):
        write(self.root, report.UPLOAD_KEY, json.dumps({'upload_key': KEY}))
        report.set_enabled(self.ctx, True)
        run = self.tick(None, 0)
        run = self.tick(run, service.REPORT_FIRST_S)
        self.process.poll.return_value = None
        self.process.pid = 4242
        with mock.patch.object(service, 'kill_tree') as kill:
            self.tick(run, service.REPORT_FIRST_S + service.REPORT_LIMIT_S)
        kill.assert_called_once_with(4242)


class BuildAndReceiver(unittest.TestCase):
    def test_the_release_build_carries_a_checked_upload_key_only_when_there_is_one(self):
        from tools.production.build import report_upload_key
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            source, out = tmp / 'report-upload.json', tmp / 'out'
            out.mkdir()
            self.assertFalse(report_upload_key(out, source))
            source.write_text(json.dumps({'upload_key': KEY}))
            source.chmod(0o644)
            with self.assertRaises(SystemExit):
                report_upload_key(out, source)
            source.chmod(0o600)
            self.assertTrue(report_upload_key(out, source))
            self.assertEqual(oct((out / 'report-upload.json').stat().st_mode & 0o777), '0o600')
            self.assertEqual(json.loads((out / 'report-upload.json').read_text()), {'upload_key': KEY})
            for bad in ({'upload_key': 'short'}, {'upload_key': KEY, 'extra': 1}, {'key': KEY}):
                source.write_text(json.dumps(bad))
                with self.subTest(bad=bad), self.assertRaises(SystemExit):
                    report_upload_key(out, source)
                self.assertFalse((out / 'report-upload.json').exists())
        post = (ROOT / 'buildroot/board/y2/post-build-production.sh').read_text()
        self.assertIn('install -D -m 600 "$Y2_ARTIFACT_DIR/report-upload.json" '
                      '"$target/usr/share/y2linux/report-upload.json"', post)
        self.assertIn('rm -f "$target/usr/share/y2linux/report-upload.json"', post)

    def test_the_key_maker_writes_a_private_key_and_never_replaces_one_by_accident(self):
        import importlib.util
        spec = importlib.util.spec_from_file_location('new_upload_key', ROOT / 'tools/reports/new_upload_key.py')
        maker = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(maker)
        with tempfile.TemporaryDirectory() as tmp:
            path = maker.make(Path(tmp) / 'k.json')
            self.assertEqual(oct(path.stat().st_mode & 0o777), '0o600')
            key = json.loads(path.read_text())['upload_key']
            self.assertRegex(key, r'^[A-Za-z0-9_-]{43}$')
            with self.assertRaises(SystemExit):
                maker.make(path)
            maker.make(path, replace=True)
            self.assertNotEqual(json.loads(path.read_text())['upload_key'], key)

    def test_the_receiver_accepts_exactly_the_players_triggers_and_schema(self):
        source = (ROOT / 'tools/reports/worker/src/index.js').read_text()
        triggers = set(re.findall(r"'([a-z_]+)'", re.search(r'TRIGGERS = new Set\(\[(.*?)\]\)', source, re.S).group(1)))
        self.assertEqual(triggers, set(report.TRIGGERS))
        self.assertIn("const SCHEMA = '%d';" % report.SCHEMA, source)
        self.assertIn('MAX_BYTES = %d * 1024 * 1024' % (report.BUDGET // (1024 * 1024)), source)
        self.assertIn("'%s'" % urlpath(report.ENDPOINT), source)


def urlpath(url):
    return '/' + url.split('/', 3)[3]


if __name__ == '__main__':
    unittest.main()
