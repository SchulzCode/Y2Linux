"""F31: the reason of a failed Wi-Fi join is kept, and the last failure survives the next connect."""
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

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
                         ('handshake_failed', 'WRONG_KEY'))

    def test_a_wrong_password_stays_reported_until_a_connect(self):
        # Y2Reborn #20: one wrong letter, 15 tries, the player said "Connecting…"
        # and "Saved", never why. On the owner's player (8.27) each try ended in
        # WRONG_KEY, then "not found" while the network was disabled for a while.
        from y2_platform.observe import wifi_state
        # F52: one failed handshake is no verdict; the second without a connect is.
        self.monitor.event('<3>CTRL-EVENT-SSID-TEMP-DISABLED id=7 ssid="home-net" auth_failures=1 duration=10 '
                           'reason=WRONG_KEY')
        self.assertEqual(self.record()['error'], 'handshake_failed')
        self.assertNotIn('credential', self.record())
        self.monitor.event('<3>CTRL-EVENT-NETWORK-NOT-FOUND')
        self.monitor.event('<3>Authentication with aa:bb:cc:dd:ee:01 timed out.')
        self.monitor.event('<3>CTRL-EVENT-DISCONNECTED bssid=aa:bb:cc:dd:ee:01 reason=3 locally_generated=1')
        self.monitor.event('<3>CTRL-EVENT-SSID-TEMP-DISABLED id=7 ssid="home-net" auth_failures=2 duration=20 '
                           'reason=WRONG_KEY')
        self.monitor.event('<3>CTRL-EVENT-NETWORK-NOT-FOUND')
        record = self.record()
        self.assertEqual(record['error'], 'ap_unavailable')
        self.assertEqual((record['credential']['error'], record['credential']['network_id']),
                         ('wrong_credentials', 7))
        self.assertNotIn('home-net', json.dumps(record))
        for wpa_state in ('SCANNING', '4WAY_HANDSHAKE', 'DISCONNECTED'):
            wpa = {'wpa_state': wpa_state, 'credential_failure': record['credential']['error']}
            self.assertEqual(wifi_state(True, wpa, [], [], False), ('Failed', 'wrong_credentials'))
        self.assertEqual(wifi_state(True, {'wpa_state': 'COMPLETED', 'credential_failure': 'wrong_credentials'},
                                    ['192.0.2.2'], [{}], True), ('Online', None))
        self.monitor.event('<3>CTRL-EVENT-CONNECTED - Connection to aa:bb:cc:dd:ee:01 completed [id=7 id_str=]')
        self.assertNotIn('credential', self.record())
        # Another kind of failure is not a password.
        self.monitor.event('<3>CTRL-EVENT-NETWORK-NOT-FOUND')
        self.assertNotIn('credential', self.record())
        for event in ('<3>CTRL-EVENT-SSID-TEMP-DISABLED id=7 ssid="home-net" reason=CONN_FAILED',
                      '<3>CTRL-EVENT-AUTH-REJECT aa:bb:cc:dd:ee:01 auth_type=0 status_code=1'):
            self.monitor.event(event)
            self.monitor.event(event)
            self.assertNotIn('credential', self.record())

    def test_a_handshake_cut_short_here_is_no_password_verdict(self):
        # F52 (aa96e00a): wpa_supplicant says WRONG_KEY for any handshake that
        # ends early. 950a93ac: the sleep's radio off cut one at 246 s and the
        # player said "wrong password" until the owner joined by hand.
        local = '<3>CTRL-EVENT-DISCONNECTED bssid=aa:bb:cc:dd:ee:01 reason=3 locally_generated=1'
        wrong = '<3>CTRL-EVENT-SSID-TEMP-DISABLED id=4 ssid="home-net" auth_failures=1 duration=10 reason=WRONG_KEY'
        for _ in range(3):
            self.monitor.event(local)
            self.monitor.event(wrong)
        self.assertIsNone(self.record())
        self.assertEqual(self.monitor.interrupted_handshakes, 3)
        # A blocked radio (a missed disconnect event) is ours too.
        rfkill = self.ctx.path('/sys/class/rfkill/rfkill1')
        rfkill.mkdir(parents=True)
        (rfkill / 'type').write_text('wlan\n')
        (rfkill / 'soft').write_text('1\n')
        self.monitor.event(wrong)
        self.assertIsNone(self.record())
        (rfkill / 'soft').write_text('0\n')
        # The access point's disconnect counts; a connect starts the count again.
        self.monitor.event('<3>CTRL-EVENT-DISCONNECTED bssid=aa:bb:cc:dd:ee:01 reason=15')
        self.monitor.event(wrong)
        self.assertEqual((self.record()['error'], self.record()['wrong_key']['count']), ('handshake_failed', 1))
        self.monitor.event('<3>CTRL-EVENT-CONNECTED - Connection to aa:bb:cc:dd:ee:01 completed [id=4 id_str=]')
        self.assertNotIn('wrong_key', self.record())
        self.monitor.event(wrong)
        self.assertEqual(self.record()['error'], 'handshake_failed')
        # Another network's failure starts its own count.
        self.monitor.event(wrong.replace('id=4', 'id=5'))
        self.assertEqual(self.record()['error'], 'handshake_failed')
        self.monitor.event(wrong.replace('id=4', 'id=5'))
        self.assertEqual((self.record()['error'], self.record()['credential']['network_id']), ('wrong_credentials', 5))
        summary = self.monitor.summary()
        self.assertEqual((summary['counts']['DISCONNECTED'], summary['counts']['SSID-TEMP-DISABLED']), (4, 8))
        self.assertEqual(summary['last_disconnect'], {'reason': 15, 'local': False, 'timeout': False})
        self.assertNotIn('aa:bb', json.dumps(summary))


class WifiRecoveryTests(unittest.TestCase):
    """F52: DISCONNECTED for minutes with the saved network in range; a Join connected at once."""
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.calls = []
        self.outputs = {'list_networks': 'network id / ssid / bssid / flags\n0\thome-net\tany\t\n'
                                         '1\tcafe\tany\t[DISABLED]\n2\thome-net\tany\t[TEMP-DISABLED]\n',
                        'scan_results': 'bssid / frequency / signal level / flags / ssid\n'
                                        'aa:bb:cc:dd:ee:01\t2462\t-61\t[WPA2-PSK-CCMP][ESS]\thome-net\n'
                                        'aa:bb:cc:dd:ee:02\t2412\t-50\t[WPA2-PSK-CCMP][ESS]\tcafe\n',
                        'select_network': 'OK\n', 'enable_network': 'OK\n', 'scan': 'OK\n'}
        def runner(argv, **kw):
            self.calls.append(argv[3:])
            return {'ok': True, 'reason': None, 'output': self.outputs.get(argv[3])}
        self.ctx = Context(self.tmp.name, runner)
        path = self.ctx.path('/proc/sys/kernel/random/boot_id')
        path.parent.mkdir(parents=True)
        path.write_text('boot-one')
        from y2_platform.network import WifiRecovery
        self.recovery = WifiRecovery(self.ctx)
        self.monitor = EventMonitor(self.ctx)
        patcher = patch('y2_platform.sleep_diagnostics.data_ready', return_value=True)
        patcher.start()
        self.addCleanup(patcher.stop)

    def journal(self):
        path = self.ctx.path('/data/system/platform/wifi-journal.jsonl')
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def test_the_saved_network_in_range_is_selected_like_a_join(self):
        value = {'association': 'DISCONNECTED', 'radio_present': True, 'state': 'Failed',
                 'reason': 'not_authenticated'}
        self.assertIsNone(self.recovery.observe(value, 100, self.monitor))
        self.assertIsNone(self.recovery.observe(value, 129, self.monitor))
        self.assertEqual(self.calls, [])
        self.monitor.connected_network = 2
        self.assertEqual(self.recovery.observe(value, 130, self.monitor), 'wifi_reselect')
        # The last network that connected, then the others enabled again.
        self.assertEqual(self.calls[-2:], [['select_network', '2'], ['enable_network', 'all']])
        row = self.journal()[-1]
        self.assertEqual((row['event'], row['network_id'], row['saved'], row['in_range'], row['temp_disabled'],
                          row['ok']), ('wifi_reselect', 2, 2, 2, 1, [True, True]))
        self.assertIn('counts', row['events'])
        self.assertNotIn('home-net', json.dumps(row))
        # Backed off: 30 s, then 60 s.
        self.calls.clear()
        self.recovery.observe(value, 159, self.monitor)
        self.assertEqual(self.calls, [])
        self.recovery.observe(value, 160, self.monitor)
        self.recovery.observe(value, 219, self.monitor)
        self.assertEqual(len(self.calls), 4)
        # A trying supplicant resets the wait, not the back-off.
        self.recovery.observe(dict(value, association='SCANNING'), 230, self.monitor)
        self.recovery.observe(value, 231, self.monitor)
        self.recovery.observe(value, 260, self.monitor)
        self.assertEqual(len(self.calls), 4)
        self.recovery.observe(value, 261, self.monitor)
        self.assertEqual(len(self.calls), 8)
        self.recovery.observe(dict(value, association='COMPLETED'), 270, self.monitor)
        self.assertEqual((self.journal()[-1]['event'], self.journal()[-1]['tries']), ('wifi_recovered', 3))

    def test_the_owners_disconnect_is_not_undone(self):
        value = {'association': 'DISCONNECTED', 'radio_present': True}
        self.monitor.event('<3>CTRL-EVENT-DISCONNECTED bssid=aa:bb:cc:dd:ee:01 reason=3 locally_generated=1')
        for now in (0, 30, 100, 400):
            self.assertIsNone(self.recovery.observe(value, now, self.monitor))
        self.assertEqual(self.calls, [])
        # A join after it (any try) ends the hold.
        self.monitor.event("<3>Trying to associate with aa:bb:cc:dd:ee:01 (SSID='home-net' freq=2462 MHz)")
        self.recovery.observe(value, 500, self.monitor)
        self.assertEqual(self.recovery.observe(value, 530, self.monitor), 'wifi_reselect')

    def test_not_in_range_scans_and_a_password_verdict_is_left_alone(self):
        self.outputs['scan_results'] = 'bssid / frequency / signal level / flags / ssid\n'
        value = {'association': 'INACTIVE', 'radio_present': True}
        self.recovery.observe(value, 0)
        self.assertEqual(self.recovery.observe(value, 30), 'wifi_rescan')
        self.assertEqual(self.calls[-1], ['scan'])
        self.assertEqual(self.journal()[-1]['in_range'], 0)
        self.calls.clear()
        wrong = dict(value, association='DISCONNECTED', state='Failed', reason='wrong_credentials')
        for now in (100, 200, 400):
            self.assertIsNone(self.recovery.observe(wrong, now))
        # Off, sleeping, or no radio: nothing.
        self.recovery.observe(dict(value, radio_present=False), 500)
        rfkill = self.ctx.path('/sys/class/rfkill/rfkill1')
        rfkill.mkdir(parents=True)
        (rfkill / 'type').write_text('wlan\n')
        (rfkill / 'soft').write_text('1\n')
        for now in (600, 700, 800):
            self.recovery.observe(value, now)
        self.assertEqual(self.calls, [])


if __name__ == '__main__':
    unittest.main()
