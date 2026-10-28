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
        self.assertLess(stock.index('gc9503v_write('), stock.index('reset_control_deassert(ctx->reset);'))
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

    def test_the_lk_panel_is_adopted_only_with_power_down_on(self):
        # Y2Linux #45: mtk-mmsys had no reset .status, prepare() failed and DRM
        # skipped every unprepare. Now the line is reported; adoption (and with it
        # the power-down at screen-off) is behind power_down, default off.
        self.assertIn('static bool power_down;\nmodule_param(power_down, bool, 0644);', self.source)
        prepare = self.source[self.source.index('static int gc9503v_prepare('):]
        prepare = prepare[:prepare.index('\n}\n')]
        inherited = prepare[prepare.index('if (ctx->inherited) {'):prepare.index('ctx->inherited = false;')]
        self.assertIn('if (ret == -ENOTSUPP)\n            ret = 0;', inherited)
        self.assertIn('if (!ret && !READ_ONCE(power_down))\n            return -EOPNOTSUPP;', inherited)
        # Never one panel's commands to the other: identified first (LK's lcm=, then
        # the GC9503V's own ID read), before adoption and before any full init.
        self.assertLess(inherited.index('!READ_ONCE(power_down)'), inherited.index('if (gc9503v_identify(ctx))\n            return -EOPNOTSUPP;'))
        self.assertLess(prepare.index('if (gc9503v_identify(ctx))'), prepare.index('return st7701s_init(ctx);'))
        identify = self.source[self.source.index('static int gc9503v_identify('):]
        identify = identify[:identify.index('\n}\n')]
        self.assertIn('case Y2_PANEL_ST7701S:\n\t\tctx->kind = Y2LCM_ST7701S;', identify)
        self.assertIn('mipi_dsi_set_maximum_return_packet_size(ctx->dsi, sizeof(id));', identify)
        self.assertIn('mipi_dsi_dcs_read(ctx->dsi, MIPI_DCS_GET_DISPLAY_ID, id, sizeof(id));', identify)
        self.assertIn('(id[1] << 8 | id[2]) == 0x9503 || (id[1] << 8 | id[2]) == 0x9504', identify)
        unprepare = self.source[self.source.index('static int gc9503v_unprepare('):]
        unprepare = unprepare[:unprepare.index('\n}\n')]
        # Switched off again: the next screen-off leaves the panel lit and re-inherits it.
        self.assertLess(unprepare.index('if (!READ_ONCE(power_down)) {\n\t\tctx->inherited = true;\n\t\treturn 0;'),
                        unprepare.index('gc9503v_stock_unprepare'))
        mmsys = (ROOT / 'kernel/patches/0010-mt6582-mmsys.patch').read_text()
        self.assertIn('+	.status = mtk_mmsys_reset_status,', mmsys)
        # Asserted = bit cleared (mtk_mmsys_reset_update writes 0 on assert).
        self.assertIn('+	return !(readl_relaxed(mmsys->regs + reg) & BIT(id % MMSYS_SW_RESET_PER_REG));', mmsys)

    def test_commands_use_the_stock_packet_types(self):
        # Stock DSI_set_cmdq_V2 (FM kernel 0xc05227f8): cmd > 0xAF generic
        # (0x13/0x23/0x29), else DCS (0x05/0x15/0x39). All-DCS gave white stripes.
        import subprocess, tempfile
        helper = self.source[self.source.index('static int gc9503v_write('):]
        helper = helper[:helper.index('\n}\n') + 3]
        table = self.source[self.source.index('#define GAMMA'):self.source.index('\n};', self.source.index('gc9503v_init_cmds[]')) + 3]
        stock = self.source[self.source.index('static const struct gc9503v_suspend_cmd gc9503v_stock_suspend_cmds[]'):]
        stock = stock[:stock.index('\n};') + 3]
        st7701 = self.source[self.source.index('struct st7701s_cmd {'):]
        st7701 = st7701[:st7701.index('static const struct st7701s_cmd st7701s_suspend_cmds[] = {')]
        st7701 += self.source[self.source.index('static const struct st7701s_cmd st7701s_suspend_cmds[] = {'):]
        st7701 = st7701[:st7701.index('\n};', st7701.index('st7701s_suspend_cmds[] = {')) + 3]
        program = r"""
#include <stdio.h>
#include <string.h>
#include <sys/types.h>
typedef unsigned char u8;
#define EINVAL 22
#define ARRAY_SIZE(a) (sizeof(a) / sizeof((a)[0]))
#define sizeof_field(t, f) sizeof(((t *)0)->f)
#define MIPI_DCS_SET_DISPLAY_OFF 0x28
#define MIPI_DCS_ENTER_SLEEP_MODE 0x10
#define MIPI_DCS_EXIT_SLEEP_MODE 0x11
#define MIPI_DCS_SET_DISPLAY_ON 0x29
struct mipi_dsi_device { int unused; };
struct gc9503v_cmd { u8 cmd; u8 len; u8 data[52]; };
struct gc9503v_suspend_cmd { u8 cmd; u8 len; u8 delay_ms; u8 data[5]; };
static int type; static u8 sent[64]; static size_t sent_len;
static ssize_t mipi_dsi_dcs_write(struct mipi_dsi_device *d, u8 cmd, const void *p, size_t n)
{ (void)d; type = n == 0 ? 0x05 : n == 1 ? 0x15 : 0x39; sent[0] = cmd; memcpy(sent + 1, p, n); sent_len = n + 1; return 0; }
static ssize_t mipi_dsi_generic_write(struct mipi_dsi_device *d, const void *p, size_t n)
{ (void)d; type = n == 1 ? 0x13 : n == 2 ? 0x23 : 0x29; memcpy(sent, p, n); sent_len = n; return 0; }
""" + helper + table + stock + st7701 + r"""
static int stock_type(u8 cmd, u8 len)
{ return cmd > 0xaf ? (len == 0 ? 0x13 : len == 1 ? 0x23 : 0x29) : (len == 0 ? 0x05 : len == 1 ? 0x15 : 0x39); }
int main(void)
{
	struct mipi_dsi_device d; unsigned i, bad = 0, generic = 0;
	for (i = 0; i < ARRAY_SIZE(gc9503v_init_cmds); i++) {
		const struct gc9503v_cmd *c = &gc9503v_init_cmds[i];
		if (gc9503v_write(&d, c->cmd, c->data, c->len) || type != stock_type(c->cmd, c->len) ||
		    sent_len != c->len + 1u || sent[0] != c->cmd || memcmp(sent + 1, c->data, c->len)) bad++;
		generic += c->cmd > 0xaf;
	}
	for (i = 0; i < ARRAY_SIZE(gc9503v_stock_suspend_cmds); i++) {
		const struct gc9503v_suspend_cmd *c = &gc9503v_stock_suspend_cmds[i];
		if (gc9503v_write(&d, c->cmd, c->data, c->len) || type != stock_type(c->cmd, c->len)) bad++;
	}
	for (i = 0; i < ARRAY_SIZE(st7701s_init_cmds) + ARRAY_SIZE(st7701s_suspend_cmds); i++) {
		const struct st7701s_cmd *c = i < ARRAY_SIZE(st7701s_init_cmds) ? &st7701s_init_cmds[i] :
			&st7701s_suspend_cmds[i - ARRAY_SIZE(st7701s_init_cmds)];
		if (gc9503v_write(&d, c->cmd, c->data, c->len) || type != stock_type(c->cmd, c->len) ||
		    sent[0] != c->cmd || memcmp(sent + 1, c->data, c->len)) bad++;
	}
	printf("%u %u\n", bad, generic);
	return bad != 0;
}
"""
        with tempfile.TemporaryDirectory() as temporary:
            source, binary = Path(temporary) / 't.c', Path(temporary) / 't'
            source.write_text(program)
            subprocess.run(['cc', '-std=gnu11', '-Wall', '-Werror', '-o', str(binary), str(source)], check=True)
            result = subprocess.run([str(binary)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout)
        self.assertEqual(result.stdout.split(), ['0', '15'])   # 15 of the 37 rows are generic

    def test_st7701s_tables_are_the_stock_ones(self):
        # Stock FM kernel st7701_hvga_dsi_vdo_boe: lcm_init table 0xc09fffb8 (47 rows,
        # delay rows folded into delay_ms), lcm_suspend table 0xc0a00cf0 + mdelay(50).
        names = {'MIPI_DCS_EXIT_SLEEP_MODE': 0x11, 'MIPI_DCS_SET_DISPLAY_ON': 0x29,
                 'MIPI_DCS_SET_DISPLAY_OFF': 0x28, 'MIPI_DCS_ENTER_SLEEP_MODE': 0x10}
        def table(name):
            t = self.source[self.source.index(name + '[] = {'):]
            t = t[:t.index('\n};')]
            rows = []
            for cmd, length, delay, data in re.findall(r'\{(\w+), (\d+), (\d+), \{([^}]*)\}\}', t):
                params = [int(x, 0) for x in data.split(',') if x.strip()]
                self.assertEqual(int(length), len(params))
                rows.append((names.get(cmd) or int(cmd, 0), params, int(delay)))
            return rows
        page = lambda n: (0xff, [0x77, 0x01, 0x00, 0x00, n], 0)
        init = table('st7701s_init_cmds')
        self.assertEqual(len(init), 43)
        self.assertEqual(init[:3], [page(0x13), (0xef, [0x08], 0), page(0x10)])
        self.assertEqual(init[20], (0xd0, [0x88], 100))
        self.assertEqual(init[37:], [(0x11, [], 120), page(0x13), (0xe8, [0x00, 0x0c], 10),
                                     (0xe8, [0x00, 0x00], 0), page(0x00), (0x29, [], 0)])
        self.assertEqual(table('st7701s_suspend_cmds'), [
            (0x28, [], 10), (0x10, [], 120), (0xff, [0x77, 0x01, 0x00, 0x00, 0x00, 0x80], 0),
            (0xff, [0x77, 0x01, 0x00, 0x00, 0x80], 50)])
        init_fn = self.source[self.source.index('static int st7701s_init('):]
        init_fn = init_fn[:init_fn.index('\n}\n')]
        # Stock reset: 1, 20 ms, 0, 50 ms, 1, 150 ms.
        self.assertLess(init_fn.index('msleep(20);'), init_fn.index('msleep(50);'))
        self.assertLess(init_fn.index('msleep(50);'), init_fn.index('msleep(150);'))

    def test_a_lagging_dsi_fails_panel_commands_at_once(self):
        # A DSI programmed while its register reads lagged fails panel commands at
        # once instead of waiting 2 s each (the first screen-off after such a boot).
        dsi = (ROOT / 'kernel/patches/0017-mt6582-dsi.patch').read_text()
        self.assertIn('+		dsi->y2_regs_lag = lag;', dsi)
        transfer = dsi[dsi.index('return -EHOSTDOWN;'):]
        self.assertLess(transfer.index('if (dsi->driver_data->mt6582 && dsi->y2_regs_lag) {'),
                        transfer.index('dsi_mode = readl(dsi->regs + DSI_MODE_CTRL);'))

    def test_a_lag_that_outlives_the_power_cycles_reads_every_dsi_register_twice(self):
        # F17 (51f3b06d): the lag survived 3 power cycles and a DIS power-down,
        # and the wake stayed dark. Each read answers with the previous read's
        # data, so reading twice gives the right value: the engine is programmed
        # once more that way before panel commands are given up.
        dsi = (ROOT / 'kernel/patches/0017-mt6582-dsi.patch').read_text()
        helper = dsi[dsi.index('+static u32 y2_dsi_readl('):]
        helper = helper[:helper.index('\n+}\n')]
        self.assertIn('addr >= base + 0x1000', helper)            # only the DSI block
        self.assertIn('raw_spin_lock_irqsave(&y2_dsi_read_lock, flags);', helper)
        self.assertLess(helper.index('(void)readl(addr);'), helper.index('v = readl(addr);'))
        # Every later readl() of the file, including the IRQ handler's and the
        # read-modify-write helper's, goes through it.
        self.assertLess(dsi.index('+#define readl(c) y2_dsi_readl(c)'), dsi.index('+static bool y2_dsi_reads_lag('))
        poweron = dsi[dsi.index('+	lag = dsi->driver_data->mt6582 && y2_dsi_reads_lag(dsi);'):]
        poweron = poweron[:poweron.index('+		dsi->y2_regs_lag = lag;')]
        self.assertIn('if (++attempt <= 3 || !READ_ONCE(y2_dsi_twice)) {', poweron)
        self.assertLess(poweron.index('WRITE_ONCE(y2_dsi_twice, dsi->regs);'), poweron.index('goto y2_retry;'))
        # The report's dark-display trigger keeps matching a real failure.
        self.assertIn('DSI register reads still lag after 3 power cycles', poweron)
        self.assertNotIn('still lag', poweron[:poweron.index('dev_err(dev,')])

    def test_no_panel_read_goes_out_while_dsi_reads_need_the_double_read(self):
        # F17 (3e55b45e): the only double-read boot that sent the stock ID reads
        # (lcm=0-st7701) got 0 bytes and never streamed; readb() bypasses the
        # double read. Writes still go out.
        dsi = (ROOT / 'kernel/patches/0017-mt6582-dsi.patch').read_text()
        transfer = dsi[dsi.index('return -EHOSTDOWN;'):]
        guard = transfer.index('READ_ONCE(y2_dsi_twice) && MTK_DSI_HOST_IS_READ(msg->type)) {')
        self.assertLess(guard, transfer.index('dsi_mode = readl(dsi->regs + DSI_MODE_CTRL);'))
        self.assertIn('return -EIO;', transfer[guard:guard + 400])

    def test_an_enabled_dsi_without_frames_is_restarted_at_most_twice(self):
        # F17 (3e55b45e): DSI idle (status 0) and vblank stuck at 1 from the
        # first modeset; no screen-off ever cycled it, which cleared 51f3b06d.
        dsi = (ROOT / 'kernel/patches/0017-mt6582-dsi.patch').read_text()
        sample = dsi[dsi.index('+static void y2_scanout_sample('):]
        sample = sample[:sample.index('\n+}\n')]
        check = sample[sample.index('if (!dsi->y2_samples && dsi->y2_vblank && dsi->enabled'):]
        check = check[:check.index('return;')]
        self.assertIn('dsi->y2_restarts < 2', check)
        self.assertIn('drm_crtc_vblank_count(dsi->y2_crtc) - dsi->y2_vblank_start < 2', check)
        self.assertIn('!(readl(dsi->regs + DSI_INTSTA) & DSI_BUSY)', check)   # a streaming DSI is busy
        self.assertLess(check.index('dsi->y2_restarts++;'), check.index('y2_dsi_restart(dsi);'))
        self.assertIn('schedule_delayed_work(&dsi->y2_sample, msecs_to_jiffies(250));', check)
        restart = dsi[dsi.index('+static void y2_dsi_restart(struct mtk_dsi *dsi)\n'):]
        restart = restart[:restart.index('\n+}\n')]
        order = ['mtk_dsi_stop(dsi);', 'phy_power_off(dsi->phy);', 'dsi->refcount = 0;',
                 'mtk_dsi_poweron(dsi);', 'mtk_output_dsi_enable(dsi);']
        self.assertEqual([restart.index(s) for s in order], sorted(restart.index(s) for s in order))
        self.assertNotIn('switch_to_cmd_mode', restart)       # idle engine: no VM_DONE wait
        enable = dsi[dsi.index('mtk_output_dsi_enable(dsi);\n+    if (dsi->driver_data->mt6582) {'):]
        enable = enable[:enable.index('schedule_delayed_work(&dsi->y2_sample')]
        self.assertIn('dsi->y2_restarts = 0;', enable)
        self.assertIn('dsi->y2_vblank_start = drm_crtc_vblank_count(dsi->y2_crtc);', enable)
        # The bridge disable cancels the sampler before it touches the DSI.
        disable = dsi[dsi.index('+	if (dsi->driver_data->mt6582) {\n+        cancel_delayed_work_sync(&dsi->y2_sample);'):]
        self.assertLess(disable.index('cancel_delayed_work_sync'), disable.index('mtk_output_dsi_disable(dsi);'))


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

    def test_panel_power_down_is_on_only_for_a_confirmed_panel(self):
        # #45: armed at application_ready on an ST7701S (confirmed 6456) and a
        # GC9503V (confirmed on #45/#42, Beta 8.30); an unknown panel keeps the
        # lit panel unless the owner opts in.
        from y2_platform.common import Context
        from y2_platform.power_features import apply
        for metadata, expected in (('panel=st7701s', 'Y'), ('panel=gc9503v', 'Y'), ('panel=unknown', 'N'), ('', 'N')):
            with tempfile.TemporaryDirectory() as temp:
                ctx = Context(temp)
                param = ctx.path('/sys/module/panel/parameters/power_down')
                param.parent.mkdir(parents=True)
                param.write_text('N')
                meta = ctx.path('/sys/firmware/y2_boot/metadata')
                meta.parent.mkdir(parents=True)
                meta.write_text('valid=1 boot_mode=0 boot_reason=4 offline=0 recovery_image=0 ' + metadata + '\n')
                apply(ctx)
                self.assertEqual(param.read_text(), expected, metadata)
                override = ctx.path('/data/system/platform/power-features.json')
                override.parent.mkdir(parents=True, exist_ok=True)
                override.write_text(json.dumps({'panel_power_down': expected == 'N'}))
                apply(ctx)
                self.assertEqual(param.read_text(), 'Y' if expected == 'N' else 'N', metadata)

    def test_the_panel_is_remembered_for_boots_lk_cannot_identify(self):
        # fm-lk 0x81e0af60: lcm=%1d is LK's "read 0x0A succeeded" flag; the name is
        # the first driver whose compare_id matched, else the last (st7701). The
        # kernel's panel= (gc9503v with either digit, st7701 with lcm=1-) is
        # recorded on Y2DATA and handed to the kernel (panel_hint) when it is unknown.
        from y2_platform.common import Context
        from y2_platform.power_features import apply
        with tempfile.TemporaryDirectory() as temp:
            ctx = Context(temp)
            params = ctx.path('/sys/module/panel/parameters')
            params.mkdir(parents=True)
            (params / 'power_down').write_text('N')
            (params / 'panel_hint').write_text('0')
            meta = ctx.path('/sys/firmware/y2_boot/metadata')
            meta.parent.mkdir(parents=True)
            lcm = ctx.path('/sys/firmware/y2_boot/lcm')
            def boot(panel, token):
                meta.write_text('valid=1 boot_mode=0 boot_reason=4 offline=0 recovery_image=0 panel=%s\n' % panel)
                lcm.write_text(token + '\n')
                (params / 'power_down').write_text('N')
                (params / 'panel_hint').write_text('0')
                apply(ctx)
                return (params / 'power_down').read_text(), (params / 'panel_hint').read_text()
            # No record yet and LK could not read the panel: stays lit.
            self.assertEqual(boot('unknown', 'lcm=0-st7701_hvga_dsi_vdo_boe'), ('N', '0'))
            # An identified boot records it; LK itself identifies, no hint needed.
            self.assertEqual(boot('st7701s', 'lcm=1-st7701_hvga_dsi_vdo_boe'), ('Y', '0'))
            self.assertEqual(ctx.json('/data/system/platform/panel.json')['panel'], 'st7701s')
            # A soft restart uses the record.
            self.assertEqual(boot('unknown', 'lcm=0-st7701_hvga_dsi_vdo_boe'), ('Y', '2'))
            # The driver's own ID read identifies it too (and is recorded).
            (params / 'panel_kind').write_text('2')
            ctx.path('/data/system/platform/panel.json').unlink()
            self.assertEqual(boot('unknown', 'lcm=0-st7701_hvga_dsi_vdo_boe'), ('Y', '0'))
            self.assertEqual(ctx.json('/data/system/platform/panel.json')['panel'], 'st7701s')
            (params / 'panel_kind').write_text('0')
            # A GC9503V is armed too (8.30) and its record handed over. LK names
            # it with lcm=0- too (#45/#46: a cold boot on the reporter's player).
            self.assertEqual(boot('gc9503v', 'lcm=0-gc9503v_hvga_dsi_vdo_hsd'), ('Y', '0'))
            self.assertEqual(ctx.json('/data/system/platform/panel.json')['panel'], 'gc9503v')
            self.assertEqual(boot('gc9503v', 'lcm=1-gc9503v_hvga_dsi_vdo_hsd'), ('Y', '0'))
            self.assertEqual(boot('unknown', 'lcm=0-st7701_hvga_dsi_vdo_boe'), ('Y', '1'))
        source = StockPanelSuspend.source[StockPanelSuspend.source.index('static int gc9503v_identify('):]
        source = source[:source.index('\n}\n')]
        # LK's identification always wins; the record only fills a boot LK could not read.
        self.assertLess(source.index('switch (y2_boot_panel())'), source.index('switch (READ_ONCE(panel_hint))'))
        # Then the stock compare_id registers, before the record.
        self.assertLess(source.index('MIPI_DCS_GET_DISPLAY_ID'), source.index('mipi_dsi_dcs_read(ctx->dsi, 0xa1, st, sizeof(st))'))
        self.assertLess(source.index('0xa1, st, sizeof(st)'), source.index('switch (READ_ONCE(panel_hint))'))
        self.assertIn('(st[0] == 0x88 || st[0] == 0x99)', source)

    def test_sleep_rows_record_both(self):
        diagnostics = (ROOT / 'tools/platform/y2_platform/sleep_diagnostics.py').read_text()
        self.assertIn("'/sys/module/panel/parameters/stock_suspend'", diagnostics)
        self.assertIn("'/sys/module/panel/parameters/power_down'", diagnostics)
        self.assertIn("'/sys/module/y2_cs43131/parameters/vgp2_idle_off'", diagnostics)


if __name__ == '__main__':
    unittest.main()
