"""Regressions for the owner-observed health crash and charging-time read hang."""
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools/platform'))
from y2_platform import cli
from y2_platform.common import Context, command
from y2_platform.observe import cpu


class TelemetryRegressions(unittest.TestCase):
    def test_health_cli_dispatches_quick_and_full_checks_and_preserves_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            ctx = Context(directory)
            errors = ctx.path('/sys/fs/ext4/test/errors_count')
            errors.parent.mkdir(parents=True)
            errors.write_text('1\n')
            for full in ([], ['--full']):
                with self.subTest(full=full), patch.object(cli, 'Context', return_value=ctx), \
                        patch.object(sys, 'argv', ['y2-health', 'health', '--json', *full]), \
                        patch('sys.stdout', new_callable=io.StringIO) as stdout:
                    exit_code = cli.main()
                result = json.loads(stdout.getvalue())
                self.assertEqual(exit_code, 1)
                self.assertEqual(result['schema'], 'org.y2linux.health/v1')
                self.assertEqual(result['state'], 'FAILED')
                self.assertIn('filesystem_errors', result['record']['failure'])
                self.assertEqual(result['record']['parameters']['mode'], 'full' if full else 'quick')
                self.assertEqual(any(c['name'] == 'alsa_query' for c in result['checks']), bool(full))

    def test_network_cli_still_dispatches_to_network_check(self):
        result = {'record': {'result': 'OK'}, 'state': 'Observed'}
        with tempfile.TemporaryDirectory() as directory, \
                patch.object(cli, 'Context', return_value=Context(directory)), \
                patch.object(sys, 'argv', ['y2-platform', 'network-check', '--peer', '192.0.2.1']), \
                patch('y2_platform.network_check.check', return_value=result) as network, \
                patch('sys.stdout', new_callable=io.StringIO) as stdout:
            self.assertEqual(cli.main(), 0)
            self.assertEqual(json.loads(stdout.getvalue()), result)
            self.assertEqual(network.call_args.args[1:], ('192.0.2.1', 10, False, 'wlan0'))

    def test_active_wakeup_read_returns_without_leaving_a_reader(self):
        with tempfile.TemporaryDirectory() as directory:
            ctx = Context(directory, runner=command)
            counter = ctx.path('/sys/power/wakeup_count')
            counter.parent.mkdir(parents=True)
            # With no writer, opening this FIFO blocks like the active-wakeup
            # sysfs read. Exercise a real child, deadline, termination and reap.
            os.mkfifo(counter)
            children = []
            popen = subprocess.Popen
            def spawn(*args, **kwargs):
                child = popen(*args, **kwargs)
                children.append(child)
                return child
            start = time.monotonic()
            with patch('y2_platform.common.subprocess.Popen', side_effect=spawn):
                result = cpu(ctx)
            self.assertLess(time.monotonic() - start, 2)
            self.assertIsNone(result['wakeup_count'])
            self.assertEqual(result['wakeup_count_reason'], 'command_timeout')
            self.assertEqual(len(children), 1)
            self.assertIsNotNone(children[0].poll())

    def test_wakeup_zero_is_observed_but_invalid_or_missing_data_is_unknown(self):
        with tempfile.TemporaryDirectory() as directory:
            ctx = Context(directory, runner=command)
            counter = ctx.path('/sys/power/wakeup_count')
            counter.parent.mkdir(parents=True)
            for text, expected in [('0\n', 0), ('42\n', 42), ('-1\n', None), ('bad\n', None)]:
                counter.write_text(text)
                result = cpu(ctx)
                self.assertEqual(result['wakeup_count'], expected)
                self.assertEqual(result['wakeup_count_reason'], None if expected is not None else 'counter_unavailable')
            counter.unlink()
            result = cpu(ctx)
            self.assertIsNone(result['wakeup_count'])
            self.assertEqual(result['wakeup_count_reason'], 'command_failed')


if __name__ == '__main__':
    unittest.main()
