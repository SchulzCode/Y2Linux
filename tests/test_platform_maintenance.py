"""The BlueZ init script's maintenance guard (tools/production/service_guards.py).

The owner reset and state export themselves are tested in
platform/crates/y2-platform/src/commands/reports/maintenance_tests.rs.
"""
import unittest
from tools.production.service_guards import bluetooth_maintenance_guard

class Maintenance(unittest.TestCase):
    def test_bluez_guard_distinguishes_start_from_restart_and_is_idempotent(self):
        source = '#!/bin/sh\nstart() {\n\tstart-stop-daemon --start\n}\nrestart() {\n\tstop\n\tstart\n}\n'
        result = bluetooth_maintenance_guard(source)
        self.assertIn('start() {\n\t[ ! -e /data/system/platform/maintenance-pending ] || return 1\n', result)
        self.assertIn('restart() {\n\tstop\n\tstart\n}', result)
        self.assertEqual(bluetooth_maintenance_guard(result), result)
        for unknown in ('restart() {\n}\n', source + 'start() {\n}\n'):
            with self.assertRaisesRegex(ValueError, 'structure'):
                bluetooth_maintenance_guard(unknown)

if __name__=='__main__':unittest.main()
