import json
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools/platform'))
from y2_platform.common import Context
from y2_platform.network import dhcp_event, lease_event, probe_dns, EventMonitor
from y2_platform.timekeeping import status, bootstrap, ntp_event
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
        with patch('y2_platform.network.wifi', return_value=before):
            self.assertTrue(probe_dns(self.ctx, before)['ok'])
        monitor = EventMonitor(self.ctx)
        monitor.event('<3>CTRL-EVENT-SSID-TEMP-DISABLED id=0 ssid="private-name" reason=WRONG_KEY')
        record = self.ctx.json('/run/y2/wifi-failure.json')
        self.assertEqual(record['error'], 'wrong_credentials')
        self.assertNotIn('private-name', json.dumps(record))

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

    def test_qualification_never_passes_invalid_iperf_output_or_offline_wifi(self):
        with patch('y2_platform.network_check.snapshot', return_value={'wifi': {'state':'Off'}}):
            self.assertEqual(network_check(self.ctx, '192.0.2.10')['record']['result'], 'FAILED')
        self.assertFalse(self.calls)
        with patch('y2_platform.network_check.snapshot', return_value={'wifi': {'state':'Online'}}):
            self.assertEqual(network_check(self.ctx, '192.0.2.10', throughput=True)['record']['result'], 'FAILED')


if __name__ == '__main__': unittest.main()
