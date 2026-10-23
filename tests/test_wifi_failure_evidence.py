"""F31: the reason of a failed Wi-Fi join is kept, and the last failure survives the next connect."""
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools/platform'))
from y2_platform.common import Context
from y2_platform.network import EventMonitor


class WifiFailureEvidence(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        outputs = {'scan_results': 'bssid / frequency / signal level / flags / ssid\n'
                                   'aa:bb:cc:dd:ee:01\t2462\t-71\t[WPA2-PSK-CCMP][ESS]\thome-net\n',
                   'list_networks': 'network id / ssid / bssid / flags\n0\thome-net\tany\t[CURRENT]\n',
                   'get': 'DE\n', 'bss': 'id=3\nbssid=aa:bb:cc:dd:ee:01\nage=142\nssid=home-net\n',
                   'get_network': 'WPA-PSK\n'}
        self.ctx = Context(self.tmp.name, lambda argv, **kw: {'ok': True, 'reason': None,
                                                              'output': outputs.get(argv[3])})
        for name, value in (('/proc/sys/kernel/random/boot_id', 'boot-one'), ('/run/y2/wifi-epoch', '3')):
            path = self.ctx.path(name)
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(value)
        self.monitor = EventMonitor(self.ctx)

    def record(self):
        return self.ctx.json('/run/y2/wifi-failure.json')

    def test_reason_is_kept_without_names(self):
        self.monitor.event('<3>CTRL-EVENT-SSID-TEMP-DISABLED id=0 ssid="home-net" auth_failures=1 duration=10 '
                           'reason=CONN_FAILED')
        record = self.record()
        self.assertEqual((record['error'], record['reason']), ('authentication_failed', 'CONN_FAILED'))
        self.monitor.event('<3>CTRL-EVENT-AUTH-REJECT aa:bb:cc:dd:ee:01 auth_type=0 auth_transaction=2 '
                           'status_code=15')
        self.assertEqual(self.record()['reason'], '15')
        self.assertNotIn('home-net', json.dumps(self.record()))
        self.assertNotIn('aa:bb', json.dumps(self.record()))

    def test_last_failure_survives_connects(self):
        self.monitor.event('<3>CTRL-EVENT-NETWORK-NOT-FOUND')
        self.monitor.event('<3>CTRL-EVENT-CONNECTED - Connection to aa:bb:cc:dd:ee:01 completed [id=0 id_str=]')
        record = self.record()
        self.assertIsNone(record['error'])
        last = record['last_failure']
        self.assertEqual((last['error'], last['failures'], last['boot_id']), ('ap_unavailable', 1, 'boot-one'))
        self.assertTrue(last['scan']['saved_seen'])
        # F13 (cb904ffb): seen at a good level and still "not found": entry age
        # and security of the hit vs the saved network, no name or address.
        self.assertEqual((last['scan']['saved_age_s'], last['scan']['saved_flags'], last['scan']['saved_key_mgmt']),
                         (142, '[WPA2-PSK-CCMP][ESS]', ['WPA-PSK']))
        self.assertNotIn('home-net', json.dumps(record))
        self.assertNotIn('aa:bb', json.dumps(record))
        # A second connect keeps it; a new failure replaces it at the next connect.
        self.monitor.event('<3>CTRL-EVENT-CONNECTED - Connection to aa:bb:cc:dd:ee:01 completed [id=0 id_str=]')
        self.assertEqual(self.record()['last_failure']['error'], 'ap_unavailable')
        self.monitor.event('<3>CTRL-EVENT-SSID-TEMP-DISABLED id=0 ssid="home-net" reason=WRONG_KEY')
        self.assertEqual(self.record()['last_failure']['error'], 'ap_unavailable')
        self.monitor.event('<3>CTRL-EVENT-CONNECTED - Connection to aa:bb:cc:dd:ee:01 completed [id=0 id_str=]')
        self.assertEqual((self.record()['last_failure']['error'], self.record()['last_failure']['reason']),
                         ('wrong_credentials', 'WRONG_KEY'))


if __name__ == '__main__':
    unittest.main()
