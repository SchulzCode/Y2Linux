"""reborn-supervise: the bounded restart budget, then the "Couldn't Start" screen
instead of a stale lit framebuffer (#49), a new budget on an SD card change and a
power-off on battery. The script runs for real against a fake file tree."""
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
SUPERVISE = ROOT / 'buildroot/package/reborn/reborn-supervise'
SH = shutil.which('busybox') and ['busybox', 'sh'] or ['sh']


class Supervise(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='rs-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for directory in ('data/reborn/logs', 'data/updates', 'run/reborn', 'run/y2', 'usr/bin', 'usr/sbin',
                          'usr/libexec', 'sys/block', 'sys/class/backlight/y2-backlight',
                          'sys/class/power_supply/BAT0'):
            (self.root / directory).mkdir(parents=True)
        self.put('sys/class/power_supply/BAT0/status', 'Discharging\n')
        self.put('sys/class/backlight/y2-backlight/bl_power', '0\n')
        self.reborn_exit(101)
        self.tool('usr/libexec/reborn-splash', 'echo "start $*" >> %s\n'
                  'trap \'echo stop >> %s; exit 0\' TERM\nwhile :; do sleep 0.02; done\n'
                  % (self.root / 'splash.log', self.root / 'splash.log'))
        self.tool('usr/sbin/y2-platform', 'echo "$*" >> %s\n' % (self.root / 'platform.log'))
        text = SUPERVISE.read_text()
        for path in ('/data', '/run', '/usr/bin/reborn', '/usr/libexec/reborn-splash', '/usr/sbin/y2-platform',
                     '/sys/block', '/sys/class'):
            text = text.replace(path, str(self.root) + path)
        text = text.replace('sleep $((attempt * 2))', 'sleep 0').replace('nap 5', 'nap 0.05')
        self.script = self.root / 'supervise'
        self.script.write_text(text.replace('-eq 600', '-eq 40'))
        self.process = None

    def tearDown(self):
        if self.process and self.process.poll() is None:
            self.process.kill()
            self.process.wait(timeout=5)

    def put(self, name, text):
        (self.root / name).write_text(text)

    def tool(self, name, body):
        path = self.root / name
        path.write_text('#!/bin/sh\n' + body)
        path.chmod(0o755)

    def reborn_exit(self, status):
        self.tool('usr/bin/reborn', 'echo run >> %s\nexit %d\n' % (self.root / 'reborn.log', status))

    def lines(self, name):
        path = self.root / name
        return path.read_text().splitlines() if path.exists() else []

    def start(self):
        self.process = subprocess.Popen(SH + [str(self.script)], start_new_session=True)

    def wait(self, predicate, seconds=10):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if predicate():
                return
            time.sleep(.02)
        self.fail('deadline expired')

    def test_clean_exit_ends_supervision_without_a_failure_screen(self):
        self.reborn_exit(0)
        self.start()
        self.assertEqual(self.process.wait(timeout=10), 0)
        self.assertEqual(self.lines('reborn.log'), ['run'])
        self.assertEqual(self.lines('splash.log'), [])

    def test_exhausted_budget_shows_the_screen_and_a_card_change_retries(self):
        self.put('data/updates/state.json', '{"state": "PendingHealth"}')
        self.start()
        self.wait(lambda: self.lines('splash.log') == ['start --failed'])
        self.assertEqual(len(self.lines('reborn.log')), 5)
        self.assertEqual(self.lines('sys/class/backlight/y2-backlight/bl_power'), ['4'])
        self.assertIn('restart_budget_exhausted', self.lines('data/reborn/logs/supervisor-last.json')[0])
        (self.root / 'sys/block/mmcblk1').mkdir()               # the user inserts or removes the card
        self.wait(lambda: len(self.lines('splash.log')) == 3)
        self.assertEqual(self.lines('splash.log'), ['start --failed', 'stop', 'start --failed'])
        self.assertEqual(len(self.lines('reborn.log')), 10)
        # An update waiting for its health check is never powered off; the light goes out.
        self.wait(lambda: self.lines('splash.log')[-1:] == ['stop'])
        time.sleep(.3)
        self.assertEqual(self.lines('platform.log'), [])
        self.assertEqual(self.lines('sys/class/backlight/y2-backlight/bl_power'), ['4'])
        self.assertIsNone(self.process.poll())

    def test_on_battery_the_player_powers_off_after_the_wait(self):
        self.start()
        self.wait(lambda: self.lines('platform.log') == ['shutdown poweroff --reason service'])
        self.assertEqual(self.lines('splash.log'), ['start --failed', 'stop'])
        self.assertIn('poweroff', self.lines('data/reborn/logs/supervisor-last.json')[0])

    def test_on_the_charger_only_the_light_goes_out(self):
        self.put('sys/class/power_supply/BAT0/status', 'Charging\n')
        self.start()
        self.wait(lambda: self.lines('splash.log') == ['start --failed', 'stop'])
        time.sleep(.3)
        self.assertEqual(self.lines('platform.log'), [])

    def test_a_stop_ends_the_screen_at_once(self):
        self.start()
        self.wait(lambda: self.lines('splash.log') == ['start --failed'])
        self.process.send_signal(signal.SIGTERM)         # S90reborn stop signals the supervisor only
        self.assertEqual(self.process.wait(timeout=3), 0)
        self.wait(lambda: self.lines('splash.log') == ['start --failed', 'stop'], seconds=3)


if __name__ == '__main__':
    unittest.main()
