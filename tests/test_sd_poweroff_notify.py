"""F59: an SD card that never acknowledges its power-off notification.

Install 49d6c732 (reports 39778b8a on 9.1, 8b00b513 on 8.43; SDXC 116 GiB,
manfid 0x11 oemid 0x455a, 03/2026): every first sleep failed after 1.1 s with
`mmc1: Card stuck being busy! __mmc_poll_for_busy` and `mmc_bus_suspend returns
-110`. Upstream's sd_poweroff_notify() fails the suspend when the Power Off
Notification Ready bit does not appear within SD_POWEROFF_NOTIFY_TIMEOUT_MS; the
stock 3.4 kernel never sent the notification.
"""
from pathlib import Path
import json
import unittest

ROOT = Path(__file__).resolve().parents[1]
PATCH = '0071-y2-sd-poweroff-notify-fallback.patch'


def added(patch):
    return [l[1:] for l in patch.splitlines() if l.startswith('+') and not l.startswith('+++')]


class SdPoweroffNotifyFallback(unittest.TestCase):
    def setUp(self):
        self.patch = (ROOT / 'kernel/patches' / PATCH).read_text()
        self.added = added(self.patch)

    def test_only_the_y2_sd_host_takes_the_fallback(self):
        gate = [l for l in self.added if 'of_device_is_compatible' in l]
        self.assertEqual(len(gate), 1)
        self.assertIn('"innioasis,y2-sd"', gate[0])
        self.assertIn('\treturn IS_ENABLED(CONFIG_Y2_POWER) && host->parent &&', self.added)
        # Every other host keeps upstream's notify/deselect choice, which follows unchanged.
        start = self.added.index('\tif (y2_sd_host(host))')
        self.assertEqual(self.added[start:start + 3], ['\tif (y2_sd_host(host))',
                                                       '\t\terr = y2_sd_suspend_card(host);',
                                                       '\telse if (sd_can_poweroff_notify(card))'])
        self.assertIn(' \telse if (!mmc_host_is_spi(host))', self.patch.splitlines())

    def test_cache_is_flushed_before_the_notification_and_only_a_timeout_falls_back(self):
        body = self.added[self.added.index('static int y2_sd_suspend_card(struct mmc_host *host)'):]
        flush = body.index('\terr = sd_flush_cache(host);')
        notify = body.index('\t\terr = sd_poweroff_notify(card);')
        self.assertLess(flush, notify)
        # A flush failure still fails the suspend; any notify error but a timeout is returned.
        self.assertEqual(body[flush + 1:flush + 3], ['\tif (err)', '\t\treturn err;'])
        self.assertEqual(body[notify + 1:notify + 3], ['\t\tif (err != -ETIMEDOUT)', '\t\t\treturn err;'])
        # The card is not asked again until it is re-detected, then suspended like a card without
        # the power management function.
        self.assertIn('\t\tcard->ext_power.feature_support &= ~SD_EXT_POWER_OFF_NOTIFY;', body)
        self.assertIn('\treturn mmc_deselect_cards(host);', body)

    def test_manifest_overlays_sd_c_with_this_patch_only(self):
        specs = json.loads((ROOT / 'kernel/patches/manifest.json').read_text())['overlays']
        self.assertEqual([s['patch'] for s in specs if s['path'] == 'drivers/mmc/core/sd.c'], [PATCH])


if __name__ == '__main__':
    unittest.main()
