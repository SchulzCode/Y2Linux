"""Platform API failure boundaries using Linux-shaped fixtures, no hardware."""
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools/platform'))
from y2_platform.boot import mark, transition
from y2_platform.common import Context, atomic_json, command
from y2_platform.health import check
from y2_platform.observe import snapshot, space_state, storage, utilization, wifi


class PlatformContract(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.ctx = Context(self.root)
        self.put('/proc/sys/kernel/random/boot_id', 'boot-a')
        self.put('/proc/sys/kernel/osrelease', 'fixture-kernel')
        self.put('/etc/y2linux/versions.json', json.dumps({'build_git_commit': 'commit', 'rootfs_version': 'release'}))

    def put(self, name, content):
        p = self.ctx.path(name)
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(content)
        return p

    def mounted_sd(self, generation=51):
        self.put('/proc/self/mountinfo', f'{generation} 1 179:9 / /media/sd rw - vfat /dev/mmcblk1p1 rw\n')
        device = self.ctx.path('/sys/devices/platform/11240000.mmc/mmcblk1/mmcblk1p1')
        device.mkdir(parents=True, exist_ok=True)
        link = self.ctx.path('/sys/dev/block/179:9')
        link.parent.mkdir(parents=True, exist_ok=True)
        if not link.is_symlink():
            link.symlink_to(device)
        self.ctx.path('/media/sd').mkdir(parents=True, exist_ok=True)
        self.ctx.runner = lambda *a, **k: {'ok': True, 'output': 'card-one', 'reason': None}
        self.put('/run/y2/media-mount.json', json.dumps({'boot_id': self.ctx.read('/proc/sys/kernel/random/boot_id'),
                 'mount_id': generation, 'device_id': '179:9', 'uuid': 'card-one'}))
        return device

    def test_disappeared_source_never_looks_like_an_empty_healthy_card(self):
        device = self.mounted_sd()
        first = storage(self.ctx)['volumes'][2]
        self.assertEqual(first['state'], 'Ready')
        device.rmdir()
        absent = storage(self.ctx)['volumes'][2]
        self.assertEqual(absent['state'], 'Failed')
        self.assertEqual(absent['space_state'], 'Failed')
        self.assertIsNone(absent['uuid'])
        self.assertTrue(self.ctx.path('/media/sd').is_dir())
        self.put('/proc/self/mountinfo', '')
        self.assertEqual(storage(self.ctx)['volumes'][2]['state'], 'Unavailable')

    def test_reinsert_and_new_boot_change_generation_even_at_same_path(self):
        self.mounted_sd()
        first = storage(self.ctx)['volumes'][2]['generation']
        self.mounted_sd(52)
        second = storage(self.ctx)['volumes'][2]['generation']
        self.put('/proc/sys/kernel/random/boot_id', 'boot-b')
        third = storage(self.ctx)['volumes'][2]['generation']
        self.assertEqual(len({first, second, third}), 3)

    def test_unknown_sensors_are_null_not_zero_and_die_temperature_is_separate(self):
        self.put('/sys/class/thermal/thermal_zone0/type', 'cpu-thermal')
        self.put('/sys/class/thermal/thermal_zone0/temp', '46800')
        self.put('/sys/class/power_supply/battery/type', 'Battery')
        self.put('/sys/class/power_supply/battery/voltage_now', '3900000')
        self.put('/sys/class/power_supply/battery/constant_charge_current', '450000')
        result = snapshot(self.ctx)
        self.assertEqual(result['thermal']['zones'][0]['temperature_millicelsius'], 46800)
        self.assertIsNone(result['power']['pack_temperature'])
        self.assertIsNone(result['power']['measured_current_ua'])
        self.assertIsNone(result['power']['soc_percent'])
        self.assertEqual(result['power']['supplies'][0]['constant_charge_current'], '450000')
        self.assertEqual(result['record']['evidence_level'], 'IMPLEMENTED')
        json.dumps(result, allow_nan=False)

    def test_cpu_delta_rejects_reset_and_missing_cores(self):
        a = {'cpu0': [10, 0, 10, 80, 0, 0, 0, 0]}
        b = {'cpu0': [20, 0, 20, 160, 0, 0, 0, 0], 'cpu1': [1] * 8}
        self.assertEqual(utilization(a, b), {'cpu0': 20.0})
        self.assertIsNone(utilization(b, a))
        self.assertIsNone(utilization(a, a))

    def test_wifi_authentication_is_insufficient_and_dns_epoch_expires(self):
        self.ctx.path('/sys/class/net/wlan0').mkdir(parents=True)
        address = []
        def run(argv, **kwargs):
            if argv[-1] == 'status':
                output = 'wpa_state=COMPLETED\nbssid=peer1'
            elif 'addr' in argv:
                output = json.dumps([{'addr_info': address}])
            elif 'route' in argv:
                output = '[{"dst":"default","dev":"wlan0"}]'
            else:
                output = ''
            return {'ok': True, 'output': output, 'reason': None}
        self.ctx.runner = run
        self.assertEqual(wifi(self.ctx)['state'], 'AcquiringIP')
        self.put('/run/y2/dhcp.json', '{"boot_id":"boot-a","error":"dhcp_timeout"}')
        self.assertEqual(wifi(self.ctx)['reason'], 'dhcp_timeout')
        address.append({'scope': 'global', 'local': '192.0.2.7'})
        self.assertEqual(wifi(self.ctx)['reason'], 'dns_unavailable')
        record = {'ok': True, 'boot_id': 'boot-a', 'address': '192.0.2.7', 'bssid': 'peer1',
                  'monotonic_s': time.monotonic()}
        self.put('/run/y2/dns.json', json.dumps(record))
        self.assertEqual(wifi(self.ctx)['state'], 'Online')
        record['monotonic_s'] -= 121
        self.put('/run/y2/dns.json', json.dumps(record))
        self.assertFalse(wifi(self.ctx)['dns_ready'])
        record.update(monotonic_s=time.monotonic(), bssid='peer2')
        self.put('/run/y2/dns.json', json.dumps(record))
        self.assertFalse(wifi(self.ctx)['dns_ready'])
        self.put('/run/y2/dns.json', '{"monotonic_s":null}')
        self.assertFalse(wifi(self.ctx)['dns_ready'])

    def test_record_corruption_and_no_devices_are_not_green(self):
        self.put('/etc/y2linux/versions.json', '[]')
        self.put('/data/updates/state.json', '{}')
        self.put('/proc/asound/cards', '--- no soundcards ---')
        self.put('/etc/y2linux/audio-qualified.json', '{}')
        result = snapshot(self.ctx)
        self.assertEqual(result['readiness']['audio']['state'], 'Unavailable')
        self.assertIsNone(result['record']['y2linux_commit'])
        health = check(self.ctx)
        self.assertNotEqual(health['state'], 'OK')
        self.assertEqual(next(c for c in health['checks'] if c['name'] == 'filesystem_errors')['state'], 'UNAVAILABLE')

    def test_space_reserve_and_read_only(self):
        mib = 1024**2
        self.assertEqual(space_state(200 * mib, 800 * mib), 'Normal')
        self.assertEqual(space_state(90 * mib, 800 * mib), 'LowSpace')
        self.assertEqual(space_state(39 * mib, 800 * mib), 'CriticalSpace')
        self.assertEqual(space_state(200 * mib, 800 * mib, readonly=True), 'ReadOnlyRisk')
        self.assertEqual(space_state(200 * mib, 800 * mib, error=True), 'Failed')

    def test_boot_history_preserves_previous_failure_and_bounds_retention(self):
        record = None
        for n in range(15):
            record = transition(record, f'boot-{n}', 'platform_start', {}, n)
        self.assertEqual(record['boot_count'], 15)
        self.assertEqual(record['previous_boot_id'], 'boot-13')
        self.assertEqual(len(record['history']), 8)
        self.assertTrue(record['recovery_recommended'])
        self.assertIsNone(record['reset_cause'])
        record = transition(record, 'boot-14', 'shutdown_complete', {}, 16)
        record = transition(record, 'boot-15', 'platform_start', {}, 17)
        self.assertEqual(record['consecutive_unclean'], 0)
        self.assertTrue(record['history'][-1]['orderly_shutdown'])

    def test_boot_journal_requires_verified_data_and_rejects_symlink(self):
        self.put('/data/.y2data-schema', '1')
        with self.assertRaisesRegex(ValueError, 'mount'):
            mark(self.ctx, 'platform_start')
        self.put('/run/y2-data-device', '/dev/mmcblk0p7')
        self.put('/proc/self/mountinfo', '33 1 179:7 / /data rw - ext4 /dev/mmcblk0p7 rw')
        mark(self.ctx, 'platform_start')
        self.assertEqual(self.ctx.json('/data/system/platform/boot.json')['boot_count'], 1)
        mark(self.ctx, 'services_started')
        self.assertEqual(self.ctx.json('/data/system/platform/boot.json')['boot_count'], 1)
        path = self.ctx.path('/data/system/platform/boot.json')
        self.assertEqual(path.stat().st_mode & 0o777, 0o600)
        path.unlink()
        for p in path.parent.iterdir():
            p.unlink()
        path.parent.rmdir()
        path.parent.symlink_to(self.root)
        with self.assertRaisesRegex(ValueError, 'directory'):
            mark(self.ctx, 'platform_start')

    def test_subprocess_timeout_and_output_limit(self):
        started = time.monotonic()
        result = command([sys.executable, '-c', 'import time; time.sleep(10)'], timeout=0.1)
        self.assertEqual(result['reason'], 'command_timeout')
        self.assertLess(time.monotonic() - started, 2)
        result = command([sys.executable, '-c', 'print("x"*100000)'], limit=100)
        self.assertEqual(result['reason'], 'command_output_limit')


if __name__ == '__main__':
    unittest.main()
