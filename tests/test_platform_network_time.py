import json
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools/platform'))
from y2_platform.common import Context
from y2_platform.network import dhcp_event, lease_event, probe_dns, EventMonitor, DnsProbeSchedule
from y2_platform.timekeeping import status, bootstrap, ntp_event, set_manual, remember
from y2_platform.network_check import check as network_check


class NetworkTime(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.calls = []
        def runner(argv, **kw):
            self.calls.append(argv)
            return {'ok': True, 'output': '1', 'reason': None}
        self.ctx = Context(self.tmp.name, runner)
        self.put('/proc/sys/kernel/random/boot_id', 'boot-one')

    def put(self, name, value):
        p = self.ctx.path(name)
        p.parent.mkdir(exist_ok=True, parents=True)
        p.write_text(value)

    def test_dhcp_timeout_epoch_replacement_and_dns_lifecycle(self):
        event = lease_event(self.ctx, 'CONNECTED')
        env = {'interface': 'wlan0', 'Y2_DHCP_GENERATION': event['epoch']}
        self.assertEqual(dhcp_event(self.ctx, 'leasefail', env)['error'], 'dhcp_timeout')
        env.update(ip='192.0.2.2', subnet='255.255.255.0', router='192.0.2.1', dns='192.0.2.1', lease='300')
        result = dhcp_event(self.ctx, 'bound', env)
        self.assertEqual(result['state'], 'Authenticated')
        self.assertEqual(result['error'], None)
        self.assertIn('nameserver 192.0.2.1', self.ctx.read('/run/y2/resolv.conf'))
        self.assertTrue(all('usb0' not in cmd for cmd in self.calls))
        lease_event(self.ctx, 'DISCONNECTED')
        before = len(self.calls)
        with self.assertRaisesRegex(ValueError, 'stale_dhcp_epoch'):
            dhcp_event(self.ctx, 'renew', env)
        self.assertEqual(len(self.calls), before)
        self.assertNotIn('nameserver', self.ctx.read('/run/y2/resolv.conf'))

    def test_cleanup_disconnect_is_not_counted_as_another_link_loss(self):
        self.assertEqual(lease_event(self.ctx, 'DISCONNECTED')['state'], 'Unavailable')
        self.assertEqual(self.ctx.json('/run/y2/wifi-events.json')['real_disconnects'], 0)
        lease_event(self.ctx, 'CONNECTED')
        lease_event(self.ctx, 'DISCONNECTED')
        lease_event(self.ctx, 'DISCONNECTED')  # interface-removal cleanup duplicates the action
        events = self.ctx.json('/run/y2/wifi-events.json')
        self.assertEqual(events['disconnected'], 3)  # raw action API stays compatible
        self.assertEqual(events['real_disconnects'], 1)
        lease_event(self.ctx, 'CONNECTED')
        self.assertEqual(self.ctx.json('/run/y2/wifi-events.json')['reconnects'], 1)
        lease_event(self.ctx, 'CONNECTED')  # repeated notification is not another reconnect
        self.assertEqual(self.ctx.json('/run/y2/wifi-events.json')['reconnects'], 1)
        self.put('/proc/sys/kernel/random/boot_id', 'next-boot')
        lease_event(self.ctx, 'CONNECTED')
        events = self.ctx.json('/run/y2/wifi-events.json')
        self.assertEqual(events['connected'], 1)
        self.assertEqual(events['real_disconnects'], 0)
        self.assertEqual(events['reconnects'], 0)

    def test_sleep_radio_off_is_not_a_real_link_loss(self):
        self.put('/sys/class/rfkill/rfkill0/type', 'bluetooth')
        self.put('/sys/class/rfkill/rfkill0/soft', '1')
        self.put('/sys/class/rfkill/rfkill1/type', 'wlan')
        self.put('/sys/class/rfkill/rfkill1/soft', '1')
        lease_event(self.ctx, 'CONNECTED')
        lease_event(self.ctx, 'DISCONNECTED')
        events = self.ctx.json('/run/y2/wifi-events.json')
        self.assertEqual((events['real_disconnects'], events['radio_off_disconnects']), (0, 1))
        self.put('/sys/class/rfkill/rfkill1/soft', '0')
        lease_event(self.ctx, 'CONNECTED')
        lease_event(self.ctx, 'DISCONNECTED')
        events = self.ctx.json('/run/y2/wifi-events.json')
        self.assertEqual((events['real_disconnects'], events['radio_off_disconnects']), (1, 1))

    def test_classless_routes_and_untrusted_dhcp_syntax(self):
        epoch = lease_event(self.ctx, 'CONNECTED')['epoch']
        env = dict(interface='wlan0', Y2_DHCP_GENERATION=epoch, ip='192.0.2.2',
                   subnet='255.255.255.0', router='192.0.2.3', dns='192.0.2.1',
                   staticroutes='0.0.0.0/0 192.0.2.1 198.51.100.0/24 0.0.0.0')
        dhcp_event(self.ctx, 'bound', env)
        self.assertFalse(any('192.0.2.3' in cmd for cmd in self.calls))
        self.assertIn(['ip','-4','route','replace','198.51.100.0/24','dev','wlan0','metric','100'], self.calls)
        before = len(self.calls)
        env['dns'] = '192.0.2.1;touch /bad'
        with self.assertRaises(ValueError): dhcp_event(self.ctx, 'renew', env)
        self.assertEqual(len(self.calls), before)

    def test_probe_rejects_peer_disappearance_and_reports_auth_reason_without_ssid(self):
        lease_event(self.ctx, 'CONNECTED')
        before = dict(ip_addresses=['192.0.2.2'], default_route=[{}], association_id='peer-one')
        after = dict(before, association_id='peer-two')
        with patch('y2_platform.network.wifi', return_value=after):
            self.assertIsNone(probe_dns(self.ctx, before))
        self.assertFalse(self.ctx.path('/run/y2/dns.json').exists())
        with patch('y2_platform.network.wifi', return_value=dict(before, default_route=[])):
            self.assertIsNone(probe_dns(self.ctx, before))
        with patch('y2_platform.network.wifi', return_value=before):
            self.assertTrue(probe_dns(self.ctx, before)['ok'])
        monitor = EventMonitor(self.ctx)
        monitor.event('<3>CTRL-EVENT-SSID-TEMP-DISABLED id=0 ssid="private-name" reason=WRONG_KEY')
        record = self.ctx.json('/run/y2/wifi-failure.json')
        self.assertEqual(record['error'], 'wrong_credentials')
        self.assertNotIn('private-name', json.dumps(record))

    def test_network_not_found_records_what_the_scan_saw_without_names(self):
        # F13 (70cce0f1): after a wake the saved network was "not found" for minutes;
        # nothing said whether it was out of range, on a passive channel or not seen.
        outputs = {
            'scan_results': 'bssid / frequency / signal level / flags / ssid\n'
                            'aa:bb:cc:dd:ee:01\t2462\t-71\t[WPA2-PSK-CCMP][ESS]\thome-net\n'
                            'aa:bb:cc:dd:ee:02\t5180\t-80\t[WPA2-PSK-CCMP][ESS]\thome-net\n'
                            'aa:bb:cc:dd:ee:03\t2437\t-50\t[ESS]\tneighbour\n',
            'list_networks': 'network id / ssid / bssid / flags\n0\thome-net\tany\t[CURRENT]\n',
            'get': 'DE\n'}
        def runner(argv, **kw):
            self.calls.append(argv)
            return {'ok': True, 'reason': None, 'output': outputs.get(argv[3])}
        self.ctx.runner = runner
        monitor = EventMonitor(self.ctx)
        monitor.event('<3>CTRL-EVENT-NETWORK-NOT-FOUND')
        record = self.ctx.json('/run/y2/wifi-failure.json')
        self.assertEqual(record['error'], 'ap_unavailable')
        self.assertEqual(record['scan'], {'available': True, 'networks': 3, 'saved_networks': 1, 'band_2ghz': 2,
                                          'band_5ghz': 1, 'saved_seen': True, 'saved_signal_dbm': -71,
                                          'saved_frequency_mhz': 2462, 'saved_channel': 11,
                                          'saved_access_points': 2, 'country': 'DE'})
        text = json.dumps(record)
        for private in ('home-net', 'neighbour', 'aa:bb'):
            self.assertNotIn(private, text)
        # Reported after every scan: wpa_cli is asked at most every 30 s.
        asked = len(self.calls)
        monitor.event('<3>CTRL-EVENT-NETWORK-NOT-FOUND')
        self.assertEqual(len(self.calls), asked)
        self.assertEqual(self.ctx.json('/run/y2/wifi-failure.json')['failures'], 2)
        self.assertTrue(self.ctx.json('/run/y2/wifi-failure.json')['scan']['saved_seen'])
        outputs['scan_results'] = 'bssid / frequency / signal level / flags / ssid\n'
        with patch('time.monotonic', return_value=time.monotonic() + 31):
            monitor.event('<3>CTRL-EVENT-NETWORK-NOT-FOUND')
        scan = self.ctx.json('/run/y2/wifi-failure.json')['scan']
        self.assertEqual((scan['networks'], scan['saved_seen']), (0, False))
        self.assertNotIn('saved_channel', scan)

    def test_new_lease_dns_does_not_inherit_old_probe_deadline(self):
        schedule = DnsProbeSchedule()
        link = dict(association='COMPLETED', association_id='one',
                    ip_addresses=['192.0.2.2'], default_route=[{}])
        self.assertTrue(schedule.due(link, 'first', 100))
        self.assertFalse(schedule.due(link, 'first', 101))
        # Association without DHCP cannot consume the new probe opportunity.
        self.assertFalse(schedule.due(dict(link, ip_addresses=[]), 'second', 102))
        self.assertTrue(schedule.due(link, 'second', 103))
        self.assertFalse(schedule.due(link, 'second', 104))
        self.assertTrue(schedule.due(link, 'second', 133))
        self.assertTrue(schedule.due(dict(link, association_id='two'), 'second', 134))
        self.assertFalse(schedule.due(dict(link, association=None), 'second', 135))
        self.assertTrue(schedule.due(dict(link, ip_addresses=['192.0.2.3']), 'second', 136))
        self.assertFalse(schedule.due(dict(link, default_route=[]), 'third', 137))

    def test_confirmed_dns_is_rechecked_every_five_minutes_not_every_30_s(self):
        # Live Beta 1: the probe re-ran every 30 s forever after success.
        from y2_platform.observe import DNS_FRESH_S
        schedule = DnsProbeSchedule()
        link = dict(association='COMPLETED', association_id='one',
                    ip_addresses=['192.0.2.2'], default_route=[{}])
        self.assertTrue(schedule.due(link, 'lease', 100))
        schedule.confirmed(100)
        self.assertFalse(schedule.due(link, 'lease', 131))
        self.assertFalse(schedule.due(link, 'lease', 399))
        self.assertTrue(schedule.due(link, 'lease', 400))
        # A failed re-check retries after 30 s; freshness outlasts one miss.
        self.assertFalse(schedule.due(link, 'lease', 429))
        self.assertTrue(schedule.due(link, 'lease', 430))
        self.assertGreater(DNS_FRESH_S, schedule.CONFIRMED_S + schedule.RETRY_S)
        # A new lease is still probed at once.
        self.assertTrue(schedule.due(link, 'next', 431))

    def test_event_wait_wakes_for_real_supplicant_event(self):
        import socket
        import threading
        monitor = EventMonitor(self.ctx)
        monitor.sock, sender = socket.socketpair(socket.AF_UNIX, socket.SOCK_DGRAM)
        self.addCleanup(monitor.close)
        self.addCleanup(sender.close)
        timer = threading.Timer(.02, lambda: sender.send(b'event'))
        timer.start()
        started = time.monotonic()
        monitor.wait(2)
        timer.join()
        self.assertLess(time.monotonic() - started, 1)
        self.assertEqual(monitor.sock.recv(16), b'event')

    def test_dns_readiness_rejects_old_lease_same_address_and_peer(self):
        from y2_platform.observe import wifi
        self.put('/sys/class/net/wlan0/ifindex', '4')
        self.put('/run/y2/wifi-epoch', 'current')
        def runner(argv, **kw):
            output = ('wpa_state=COMPLETED\nbssid=one' if argv[-1] == 'status' else
                      '[{"addr_info":[{"local":"192.0.2.2","scope":"global"}]}]' if 'addr' in argv else
                      '[{"dst":"default"}]' if 'route' in argv else 'RSSI=-50')
            return dict(ok=True, output=output, reason=None)
        self.ctx.runner = runner
        value = dict(ok=True, boot_id='boot-one', monotonic_s=time.monotonic(),
                     address='192.0.2.2', bssid='one', epoch='old')
        self.put('/run/y2/dns.json', json.dumps(value))
        self.assertEqual(wifi(self.ctx)['state'], 'Authenticated')
        value['epoch'] = 'current'
        self.put('/run/y2/dns.json', json.dumps(value))
        self.assertEqual(wifi(self.ctx)['state'], 'Online')

    def test_rssi_cache_is_bounded_and_invalidates_on_peer_or_boot_change(self):
        from y2_platform.observe import wifi
        self.put('/sys/class/net/wlan0/ifindex', '4')
        self.put('/run/y2/wifi-epoch', 'current')
        peer = ['one']
        calls = []
        def runner(argv, **kw):
            calls.append((argv, kw))
            output = (f'wpa_state=COMPLETED\nbssid={peer[0]}' if argv[-1] == 'status' else
                      '[{"addr_info":[{"local":"192.0.2.2","scope":"global"}]}]' if 'addr' in argv else
                      '[{"dst":"default"}]' if 'route' in argv else 'RSSI=-50')
            return dict(ok=True, output=output, reason=None)
        self.ctx.runner = runner
        first = wifi(self.ctx)
        self.assertEqual(first['rssi_dbm'], -50)
        calls.clear()
        self.assertEqual(wifi(self.ctx, signal_cache=first)['rssi_dbm'], -50)
        self.assertFalse(any(c[-1] == 'signal_poll' for c, kw in calls))
        self.put('/run/y2/network.json', json.dumps(first))
        calls.clear()
        self.assertEqual(wifi(self.ctx)['rssi_dbm'], -50)
        self.assertFalse(any(c[-1] == 'signal_poll' for c, kw in calls))
        self.assertEqual(next(kw['timeout'] for c, kw in calls if c[-1] == 'status'), 3)
        first['signal_observation']['monotonic_s'] -= 31
        self.put('/run/y2/network.json', json.dumps(first))
        calls.clear()
        wifi(self.ctx)
        self.assertTrue(any(c[-1] == 'signal_poll' for c, kw in calls))
        self.put('/run/y2/network.json', json.dumps(dict(first, signal_observation=dict(
            first['signal_observation'], monotonic_s=time.monotonic()))))
        peer[0] = 'two'
        calls.clear()
        wifi(self.ctx)
        self.assertTrue(any(c[-1] == 'signal_poll' for c, kw in calls))
        self.put('/proc/sys/kernel/random/boot_id', 'new-boot')
        calls.clear()
        wifi(self.ctx)
        self.assertTrue(any(c[-1] == 'signal_poll' for c, kw in calls))

    def test_readiness_query_failures_are_counted_without_link_events(self):
        from y2_platform import service
        callbacks = {}
        observations = iter([
            dict(state='Starting', association=None, radio_present=True,
                 supplicant_observation=dict(ok=False), traffic_counters={}),
            dict(state='Starting', association=None, radio_present=True,
                 supplicant_observation=dict(ok=False), traffic_counters={}),
            dict(state='Off', association=None, radio_present=False,
                 supplicant_observation=dict(ok=None), traffic_counters={})])
        waits = []
        def wait(seconds, *, wake_fd=None):
            waits.append(seconds)
            if len(waits) == 3:
                callbacks[service.signal.SIGTERM](None, None)
        with patch.object(service.signal, 'signal', side_effect=lambda sig, fn: callbacks.update({sig: fn})), \
             patch.object(service, 'wifi', side_effect=lambda ctx: next(observations)), \
             patch.object(service, 'display_dark', return_value=True), \
             patch.object(service, 'media_reconcile_needed', return_value=False), \
             patch.object(service, 'time_status', return_value={}), \
             patch.object(service.EventMonitor, 'poll'), \
             patch.object(service.EventMonitor, 'wait', side_effect=wait):
            service.serve(self.ctx)
        result = self.ctx.json('/run/y2/network.json')
        self.assertEqual(result['status_query_failures'], 2)
        self.assertFalse(self.ctx.path('/run/y2/wifi-events.json').exists())
        self.assertGreater(waits[0], 9)  # Unknown status must not trigger fast polling.
        self.assertGreater(waits[2], 9)

    def test_implausible_rtc_build_floor_and_network_clock_readiness(self):
        floor = 1790000000
        self.put('/etc/y2linux/build-epoch', str(floor))
        self.assertFalse(status(self.ctx, 0)['tls_ready'])
        self.assertFalse(status(self.ctx, 3600000000)['clock_sane'])
        value = bootstrap(self.ctx, now=3600000000)
        self.assertEqual(value['source'], 'build_floor')
        self.assertIn(['/bin/busybox','date','-u','-s','@'+str(floor)], self.calls)
        self.assertFalse(status(self.ctx, floor)['tls_ready'])
        ntp_event(self.ctx, 'stratum', {'stratum':'3'}, now=floor+100)
        self.assertTrue(status(self.ctx, floor+100)['tls_ready'])
        self.assertFalse(status(self.ctx, floor+1000)['tls_ready'])
        self.assertFalse(any('hwclock' in cmd for cmd in self.calls))
        self.put('/proc/sys/kernel/random/boot_id', 'boot-two')
        self.assertFalse(status(self.ctx, floor+100)['tls_ready'])
        self.put('/sys/class/rtc/rtc0/date', '2026-09-22')
        self.assertEqual(bootstrap(self.ctx, floor+200)['source'], 'retained_rtc')
        self.assertTrue(status(self.ctx, floor+200)['tls_ready'])

    def test_tls_clock_stays_ready_across_suspend(self):
        floor = 1790000000
        self.put('/etc/y2linux/build-epoch', str(floor))
        anchor = ntp_event(self.ctx, 'stratum', {'stratum': '2'}, now=floor + 100)
        mono, boot = anchor['monotonic_s'], anchor['boottime_s']
        # One hour asleep: wall clock and CLOCK_BOOTTIME advance, CLOCK_MONOTONIC
        # only by the few awake seconds. Beta 4 refused update checks here.
        self.assertTrue(status(self.ctx, floor + 3705, monotonic=mono + 5, elapsed=boot + 3605)['tls_ready'])
        # Someone stepping the wall clock is still caught.
        self.assertFalse(status(self.ctx, floor + 7305, monotonic=mono + 5, elapsed=boot + 3605)['tls_ready'])
        # An anchor written before this change keeps the monotonic comparison.
        old = {k: v for k, v in anchor.items() if k != 'boottime_s'}
        self.put('/run/y2/time.json', __import__('json').dumps(old))
        self.assertTrue(status(self.ctx, floor + 105, monotonic=mono + 5, elapsed=boot + 3605)['tls_ready'])
        self.assertFalse(status(self.ctx, floor + 3705, monotonic=mono + 5, elapsed=boot + 3605)['tls_ready'])

    def test_manual_time_is_kept_as_a_floor_never_trusted_and_ntp_replaces_it(self):
        # #53: a player that never had Wi-Fi started from the build date
        # every boot; the owner's own time now carries over a restart.
        floor = 1790000000
        self.put('/etc/y2linux/build-epoch', str(floor))
        self.put('/etc/y2linux/rtc-policy.json', json.dumps({'write_enabled': True, 'source': 'receipt'}))
        for wrong in (floor - 86401, floor + 6 * 366 * 86400, 'now', floor + 0.5, True):
            with self.assertRaisesRegex(ValueError, 'date_out_of_range'):
                set_manual(self.ctx, wrong)
        self.assertEqual(self.calls, [])
        chosen = floor + 3 * 86400
        value = set_manual(self.ctx, chosen)
        self.assertEqual(value['source'], 'manual')
        self.assertIn(['/bin/busybox', 'date', '-u', '-s', '@' + str(chosen)], self.calls)
        self.assertIn(['/bin/busybox', 'hwclock', '-w', '-u'], self.calls)
        clock = status(self.ctx, chosen + 5, monotonic=value['monotonic_s'] + 5, elapsed=value['boottime_s'] + 5)
        self.assertTrue(clock['clock_sane'])
        self.assertFalse(clock['tls_ready'])
        self.assertFalse(clock['ntp_authenticated'])
        self.assertEqual(clock['source'], 'manual')
        written = json.loads(self.ctx.read('/run/y2/time-status.json'))
        self.assertEqual((written['source'], written['tls_ready']), ('manual', False))
        last = json.loads(self.ctx.read('/data/system/platform/time-last.json'))
        self.assertEqual((last['source'], last['epoch']), ('manual', chosen))
        # Next boot, RTC lost: the clock resumes from the owner's time, untrusted.
        self.put('/proc/sys/kernel/random/boot_id', 'boot-two')
        self.calls.clear()
        self.assertEqual(bootstrap(self.ctx, now=1659312000)['source'], 'manual')
        self.assertIn(['/bin/busybox', 'date', '-u', '-s', '@' + str(chosen)], self.calls)
        self.assertFalse(status(self.ctx, chosen)['tls_ready'])
        # An RTC that kept the owner's time is still only the owner's time.
        self.put('/sys/class/rtc/rtc0/date', '2026-09-22')
        self.assertEqual(bootstrap(self.ctx, now=chosen + 100)['source'], 'manual')
        self.assertFalse(status(self.ctx, chosen + 100)['tls_ready'])
        # The first network time replaces it, even within the hour.
        ntp_event(self.ctx, 'stratum', {'stratum': '2'}, now=chosen + 200)
        last = json.loads(self.ctx.read('/data/system/platform/time-last.json'))
        self.assertEqual((last['source'], last['epoch']), ('ntp', chosen + 200))
        self.assertTrue(status(self.ctx, chosen + 200)['tls_ready'])
        self.put('/proc/sys/kernel/random/boot_id', 'boot-three')
        self.assertEqual(bootstrap(self.ctx, now=chosen + 300)['source'], 'retained_rtc')

    def test_running_clock_is_remembered_hourly_only_when_set(self):
        floor = 1790000000
        self.put('/etc/y2linux/build-epoch', str(floor))
        # The build date alone is never remembered as a time.
        bootstrap(self.ctx, now=0)
        self.assertIsNone(remember(self.ctx, now=floor + 7200, monotonic=1, elapsed=1))
        value = set_manual(self.ctx, floor + 100)
        mono, boot = value['monotonic_s'], value['boottime_s']
        path = self.ctx.path('/data/system/platform/time-last.json')
        later = lambda s: dict(now=floor + 100 + s, monotonic=mono + s, elapsed=boot + s)
        self.assertIsNone(remember(self.ctx, **later(1800)))
        written = remember(self.ctx, **later(3700))
        self.assertEqual((written['source'], written['epoch']), ('manual', floor + 3800))
        self.assertEqual(json.loads(path.read_text())['epoch'], floor + 3800)
        self.assertIsNone(remember(self.ctx, **later(3800)))
        # Asleep for two hours: the wall clock and CLOCK_BOOTTIME both ran on.
        asleep = remember(self.ctx, now=floor + 100 + 11000, monotonic=mono + 3900, elapsed=boot + 11000)
        self.assertEqual(asleep['epoch'], floor + 11100)
        # Someone stepped the clock since the anchor: not written.
        self.assertIsNone(remember(self.ctx, now=floor + 100 + 30000, monotonic=mono + 4000, elapsed=boot + 15000))
        # Orderly shutdown asks for it now.
        self.assertEqual(remember(self.ctx, every=0, **later(11010))['epoch'], floor + 11110)
        # Another boot's anchor is not this clock.
        self.put('/proc/sys/kernel/random/boot_id', 'boot-two')
        self.assertIsNone(remember(self.ctx, every=0, **later(11020)))

    def test_qualification_never_passes_invalid_iperf_output_or_offline_wifi(self):
        with patch('y2_platform.network_check.snapshot', return_value={'wifi': {'state':'Off'}}):
            self.assertEqual(network_check(self.ctx, '192.0.2.10')['record']['result'], 'FAILED')
        self.assertFalse(self.calls)
        with patch('y2_platform.network_check.snapshot', return_value={'wifi': {'state':'Online'}}):
            self.assertEqual(network_check(self.ctx, '192.0.2.10', throughput=True)['record']['result'], 'FAILED')


if __name__ == '__main__': unittest.main()
