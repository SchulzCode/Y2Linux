"""Hardware Final integration boundaries; source fixtures, never device access. The platform's parts (timer,
storage and USB observations, capabilities, the RTC alarm, NTP's RTC write) are tested in Rust
(platform/crates/y2-platform: tests/connectivity.rs, src/commands/capabilities_tests.rs,
src/commands/devices/timekeeping_tests.rs)."""
import json
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from tools.build.dev_initramfs import boot_module_paths
from tools.production.system_update import CONFIGURATION_FILES, configuration_inventory


class HardwareFinalContract(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def put(self, name, value):
        path = self.root/name.lstrip('/')
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value)
        return path

    def test_capability_template_keeps_unqualified_usb_host_disabled(self):
        caps = json.loads((ROOT/'tools/platform/capabilities.json').read_text())['capabilities']
        self.assertFalse(caps['usb_host']['enabled'])
        self.assertFalse(caps['bluetooth_aac']['enabled'])
        self.assertFalse(caps['bluetooth_aac']['qualified'])

    def test_module_allowlist_cannot_smuggle_extra_modules(self):
        paths = ['drivers/gpu/drm/mediatek/mediatek-drm', 'drivers/usb/gadget/function/usb_f_ncm',
                 'drivers/usb/gadget/legacy/g_ncm']
        self.put('/kernel/.config', 'CONFIG_USB_G_NCM=m\n')
        self.put('/kernel/modules.order', '\n'.join(path+'.o' for path in paths)+'\n')
        self.assertEqual(boot_module_paths(self.root), paths)
        self.put('/kernel/modules.order', '\n'.join(path+'.o' for path in paths)+ '\ndrivers/unknown.o\n')
        with self.assertRaises(ValueError): boot_module_paths(self.root)

    def test_package_configuration_hashes_and_independent_states(self):
        for name in CONFIGURATION_FILES:
            source = ROOT/'buildroot/board/y2/production-overlay/etc/y2linux'/name
            value = (ROOT/'tools/platform/capabilities.json').read_text() if name == 'capabilities.json' else (
                source.read_text() if source.exists() else '{"schema":1}')
            self.put('/etc/y2linux/'+name, value)
        before = configuration_inventory(self.root)
        self.put('/etc/y2linux/rtc-policy.json', '{"schema":1,"write_enabled":false}')
        after = configuration_inventory(self.root)
        self.assertNotEqual(before['files']['rtc-policy.json']['sha256'], after['files']['rtc-policy.json']['sha256'])
        caps = json.loads((self.root/'etc/y2linux/capabilities.json').read_text())
        caps['capabilities']['wired_48']['qualified'] = True
        self.put('/etc/y2linux/capabilities.json', json.dumps(caps))
        with self.assertRaises(ValueError): configuration_inventory(self.root)
