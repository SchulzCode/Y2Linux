"""POWER-FINAL-27: stock panel power-down (ledger F22) and the idle VGP2 rail (F20).

The stock rows are the Y2 FM kernel's GC9503V lcm_suspend table 0xc09ff464
(72-byte rows, decoded 2026-10-14) and cs43131_power_down_hw 0xc054b620.
"""
import json
from pathlib import Path
import re
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools/platform'))

# (cmd, params, delay ms after) in stock order; reset HIGH and 50 ms follow.
STOCK_PANEL_SUSPEND = [
    (0xf0, [0x55, 0xaa, 0x52, 0x08, 0x00], 0), (0xc1, [0x3f], 0), (0x6c, [0x60], 20),
    (0xb1, [0x00], 0), (0xfa, [0x7f, 0x00, 0x00, 0x00], 20), (0x6c, [0x50], 10),
    (0x28, [], 50), (0x10, [], 20),
    (0xf0, [0x55, 0xaa, 0x52, 0x08, 0x00], 0), (0xc2, [0xce], 0), (0xc3, [0xcd], 0),
    (0xc6, [0xfc], 0), (0xc5, [0x03], 0), (0xcd, [0x64], 0), (0xc4, [0xff], 0),
    (0xc9, [0xcd], 0), (0xf6, [0x5a, 0x87], 0), (0xfd, [0xaa, 0xaa, 0x0a], 0),
    (0xfe, [0x6a, 0x0a], 0), (0x78, [0x2a, 0xaa], 0), (0x92, [0x17, 0x08], 0),
    (0x77, [0xaa, 0x2a], 0), (0x76, [0xaa, 0xaa], 0), (0x84, [0x00], 0),
    (0x78, [0x2b, 0xba], 0), (0x89, [0x73], 0), (0x88, [0x3a], 0), (0x85, [0xb0], 0),
    (0x76, [0xeb, 0xaa], 0), (0x94, [0x80], 0), (0x87, [0x04, 0x07, 0x30], 0),
    (0x93, [0x27], 0), (0xaf, [0x02], 0),
]


def number(token):
    token = token.strip()
    return {'MIPI_DCS_SET_DISPLAY_OFF': 0x28, 'MIPI_DCS_ENTER_SLEEP_MODE': 0x10}.get(token) \
        if not token[:1].isdigit() else int(token, 0)


class StockPanelSuspend(unittest.TestCase):
    source = (ROOT / 'kernel/platform/panel.c').read_text()

    def test_table_is_the_stock_lcm_suspend(self):
        table = self.source[self.source.index('gc9503v_stock_suspend_cmds[] = {'):]
        table = table[:table.index('\n};')]
        rows = []
        for cmd, length, delay, data in re.findall(r'\{(\w+), (\d+), (\d+), \{([^}]*)\}\}', table):
            params = [int(x, 0) for x in data.split(',') if x.strip()]
            self.assertEqual(int(length), len(params))
            rows.append((number(cmd), params, int(delay)))
        self.assertEqual(rows, STOCK_PANEL_SUSPEND)

    def test_reset_high_after_the_table_and_fallback_to_26(self):
        stock = self.source[self.source.index('static int gc9503v_stock_unprepare('):]
        stock = stock[:stock.index('\n}\n')]
        self.assertLess(stock.index('mipi_dsi_dcs_write('), stock.index('reset_control_deassert(ctx->reset);'))
        self.assertNotIn('reset_control_assert', stock)
        self.assertIn('msleep(50);', stock[stock.index('reset_control_deassert'):])
        body = self.source[self.source.index('static int gc9503v_unprepare('):]
        body = body[:body.index('\n}\n')]
        # Only a fully successful stock sequence skips 26's display-off, sleep-in, reset-low.
        self.assertIn('if (READ_ONCE(stock_suspend) && !gc9503v_stock_unprepare(ctx))\n\t\treturn 0;', body)
        self.assertLess(body.index('gc9503v_stock_unprepare'), body.index('reset_control_assert(ctx->reset)'))
        # Kernel default is 26's sequence; prepare() always pulses reset and replays init.
        self.assertIn('static bool stock_suspend;\nmodule_param(stock_suspend, bool, 0644);', self.source)
        prepare = self.source[self.source.index('static int gc9503v_prepare('):]
        self.assertLess(prepare.index('ctx->inherited = false;'), prepare.index('reset_control_assert(ctx->reset);'))

    def test_the_lk_panel_is_adopted_so_screen_off_runs_the_power_down(self):
        # Y2Linux #45: mtk-mmsys had no reset .status, so reset_control_status()
        # returned -ENOTSUPP, prepare() failed every time and DRM skipped every
        # unprepare ("Skipping unprepare of already unprepared panel").
        prepare = self.source[self.source.index('static int gc9503v_prepare('):]
        prepare = prepare[:prepare.index('\n}\n')]
        inherited = prepare[prepare.index('if (ctx->inherited) {'):prepare.index('ctx->inherited = false;')]
        self.assertIn('if (ret == -ENOTSUPP)\n            ret = 0;', inherited)
        self.assertLess(inherited.index('-ENOTSUPP'), inherited.index('if (ret < 0) {'))
        self.assertIn('dev_err(dev, "panel reset status unreadable', inherited)
        mmsys = (ROOT / 'kernel/patches/0010-mt6582-mmsys.patch').read_text()
        self.assertIn('+static int mtk_mmsys_reset_status(struct reset_controller_dev *rcdev, unsigned long id)', mmsys)
        self.assertIn('+	.status = mtk_mmsys_reset_status,', mmsys)
        # Asserted = bit cleared (mtk_mmsys_reset_update writes 0 on assert).
        self.assertIn('+	return !(readl_relaxed(mmsys->regs + reg) & BIT(id % MMSYS_SW_RESET_PER_REG));', mmsys)


class IdleVgp2(unittest.TestCase):
    card = (ROOT / 'kernel/audio/y2-cs43131.c').read_text()
    dts = (ROOT / 'kernel/dts/innioasis-y2.dts').read_text()

    def test_rail_is_no_longer_always_on_and_the_card_holds_it_by_default(self):
        vgp2 = self.dts[self.dts.index('vgp2: ldo_vgp2 {'):]
        vgp2 = vgp2[:vgp2.index('};')]
        self.assertNotIn('regulator-always-on', vgp2)
        self.assertNotIn('regulator-boot-on', vgp2)
        sound = self.dts[self.dts.index('sound { compatible = "innioasis,y2-audio";'):]
        self.assertIn('vgp2-supply = <&vgp2>;', sound[:sound.index('};')])
        # The codec still takes VA/VP/VCP from VGP2: its runtime PM is the stock owner.
        self.assertIn('VA-supply = <&vgp2>; VP-supply = <&vgp2>; VCP-supply = <&vgp2>;', self.dts)
        self.assertIn('static bool y2_vgp2_held, vgp2_idle_off;', self.card)   # default: hold (26)
        self.assertIn('module_param_cb(vgp2_idle_off, &y2_vgp2_idle_off_ops, &vgp2_idle_off, 0644);', self.card)

    def test_hold_is_balanced(self):
        sync = self.card[self.card.index('static int y2_vgp2_sync(void)'):]
        sync = sync[:sync.index('\n}\n')]
        self.assertIn('if (!vgp2_idle_off && !y2_vgp2_held) {\n\t\tret = regulator_enable(y2_vgp2);\n\t\ty2_vgp2_held = !ret;', sync)
        self.assertIn('} else if (vgp2_idle_off && y2_vgp2_held) {\n\t\tret = regulator_disable(y2_vgp2);\n\t\ty2_vgp2_held = !!ret;', sync)
        probe = self.card[self.card.index('static int y2_audio_probe('):]
        self.assertLess(probe.index('devm_regulator_get(dev, "vgp2")'), probe.index('devm_add_action_or_reset(dev, y2_vgp2_release'))
        self.assertLess(probe.index('y2_vgp2_release'), probe.index('devm_snd_soc_register_card'))
        release = self.card[self.card.index('static void y2_vgp2_release('):]
        release = release[:release.index('\n}\n')]
        self.assertIn('if (y2_vgp2_held && !regulator_disable(y2_vgp2))', release)
        self.assertIn('y2_vgp2 = NULL;', release)

    def test_dtb_validator_requires_the_new_contract(self):
        validator = (ROOT / 'tools/validation/dev_dtb.py').read_text()
        self.assertIn("'regulator-always-on' not in nodes[rail]", validator)
        self.assertIn("nodes['/sound']['vgp2-supply']==cells(handle(rail))", validator)


class Switches(unittest.TestCase):
    def test_userspace_arms_both_with_owner_override(self):
        from y2_platform.common import Context
        from y2_platform.power_features import apply
        with tempfile.TemporaryDirectory() as temp:
            ctx = Context(temp)
            panel = ctx.path('/sys/module/panel/parameters/stock_suspend')
            vgp2 = ctx.path('/sys/module/y2_cs43131/parameters/vgp2_idle_off')
            for p in (panel, vgp2):
                p.parent.mkdir(parents=True)
                p.write_text('N')
            result = apply(ctx)
            self.assertEqual((panel.read_text(), vgp2.read_text()), ('Y', 'Y'))
            self.assertEqual(result['panel_stock_suspend']['wanted'], True)
            override = ctx.path('/data/system/platform/power-features.json')
            override.parent.mkdir(parents=True)
            override.write_text(json.dumps({'panel_stock_suspend': False, 'vgp2_idle_off': False}))
            apply(ctx)
            self.assertEqual((panel.read_text(), vgp2.read_text()), ('N', 'N'))

    def test_deep_idle_audio_is_armed_by_userspace(self):
        # POWER-FINAL-32: kernel default off, power_features turns it on.
        from y2_platform.common import Context
        from y2_platform.power_features import apply
        with tempfile.TemporaryDirectory() as temp:
            ctx = Context(temp)
            param = ctx.path('/sys/module/clocks/parameters/deep_idle_audio')
            param.parent.mkdir(parents=True)
            param.write_text('N')
            self.assertEqual(apply(ctx)['deep_idle_audio']['value'], 'Y')
            override = ctx.path('/data/system/platform/power-features.json')
            override.parent.mkdir(parents=True)
            override.write_text(json.dumps({'deep_idle_audio': False}))
            apply(ctx)
            self.assertEqual(param.read_text(), 'N')
        diagnostics = (ROOT / 'tools/platform/y2_platform/sleep_diagnostics.py').read_text()
        self.assertIn("'/sys/module/clocks/parameters/deep_audio_entries'", diagnostics)

    def test_sleep_rows_record_both(self):
        diagnostics = (ROOT / 'tools/platform/y2_platform/sleep_diagnostics.py').read_text()
        self.assertIn("'/sys/module/panel/parameters/stock_suspend'", diagnostics)
        self.assertIn("'/sys/module/y2_cs43131/parameters/vgp2_idle_off'", diagnostics)


if __name__ == '__main__':
    unittest.main()
