"""POWER-FINAL-20: no colour flash at boot, no white/colour screen after the shutdown animation.

Owner report 2026-10-13: the startup sometimes flashes colours; after the shutdown
animation the panel turns white or shows colours.
"""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]


class QuietDisplayPower(unittest.TestCase):
    def test_drm_never_switches_the_backlight_on(self):
        # Linked, every pipeline enable (e.g. the fbdev restore when Reborn exits
        # at shutdown) called backlight_enable() over a stale buffer.
        panel = (ROOT / 'kernel/platform/panel.c').read_text()
        self.assertNotIn('drm_panel_of_backlight(&ctx->panel)', panel.replace('no drm_panel_of_backlight()', ''))

    def test_backlight_sinks_go_dark_at_power_off(self):
        bl = (ROOT / 'kernel/platform/backlight.c').read_text()
        self.assertIn('.shutdown = y2_bl_shutdown', bl)
        body = bl[bl.index('static void y2_bl_shutdown('):]
        body = body[:body.index('\n}\n')]
        self.assertIn('regmap_update_bits(b->map, 0x356, 15, 0)', body)   # all four ISINKs off
        self.assertIn('platform_set_drvdata(pdev, bl);', bl)

    def test_splash_modeset_is_dark_and_always_relit(self):
        splash = (ROOT / 'tools/graphics/reborn-splash.c').read_text()
        show = splash[splash.index('static int show(Display *d) {'):]
        show = show[:show.index('\n}\n')]
        self.assertLess(show.index('light("4\\n");'), show.index('drmModeSetCrtc('))
        self.assertLess(show.index('drmModeSetCrtc('), show.index('drmWaitVBlank('))
        relit = show.index('light("0\\n");')
        self.assertGreater(relit, show.index('drmWaitVBlank('))
        # Relit unconditionally, before the return: a failed modeset never stays dark.
        self.assertLess(relit, show.index('return ret;'))
        self.assertNotIn('return', show[show.index('drmModeSetCrtc('):relit])

    def test_splash_cycles_the_crtc_once_when_its_modeset_gives_no_frame(self):
        # F17 (3e55b45e): no vblank from the first modeset and no CRTC off all
        # boot; an off/on cleared the same symptom on 51f3b06d.
        splash = (ROOT / 'tools/graphics/reborn-splash.c').read_text()
        show = splash[splash.index('static int show(Display *d) {'):]
        show = show[:show.index('\n}\n')]
        failed = show[show.index('if (drmWaitVBlank(d->fd,&vbl)) {'):]
        self.assertLess(failed.index('if (cycled) poll(NULL,0,40);'), failed.index('cycled=true;'))
        off = failed.index('drmModeSetCrtc(d->fd,d->crtc,0,0,0,NULL,0,NULL);')
        self.assertLess(failed.index('cycled=true;'), off)
        self.assertLess(off, failed.index('ret=drmModeSetCrtc(d->fd,d->crtc,d->fb,'))
        self.assertLess(failed.index('ret=drmModeSetCrtc(d->fd,d->crtc,d->fb,'), show.index('light("0\\n");'))
        self.assertIn('static bool cycled;', splash)

    def test_dsi_handoff_is_dark_until_the_first_linux_frame(self):
        # 21's boot log: "park video" at 0.489 s, Linux DSI modeset at 0.532 s,
        # splash frame at 0.642 s; the lit panel had no stream in between.
        patch = (ROOT / 'kernel/patches/0017-mt6582-dsi.patch').read_text()
        park = patch[patch.index('+static int mt6582_dsi_park(struct mtk_dsi *dsi)'):]
        self.assertLess(park.index('y2_handoff_dark(dsi);'), park.index('park video'))
        dark = patch[patch.index('+static void y2_handoff_dark('):]
        dark = dark[:dark.index('\n+}\n')]
        self.assertIn('BACKLIGHT_POWER_OFF', dark)
        self.assertIn('msecs_to_jiffies(3000)', dark)          # never stays dark
        self.assertIn('props.power != BACKLIGHT_POWER_ON) return;', dark)
        enable = patch[patch.index('mtk_output_dsi_enable(dsi);\n+    if (dsi->driver_data->mt6582) {'):]
        self.assertIn('mod_delayed_work(system_wq, &dsi->y2_relight, msecs_to_jiffies(50))', enable[:400])
        release = patch[patch.index('+static void y2_relight_release('):]
        release = release[:release.index('\n+}\n')]
        self.assertIn('BACKLIGHT_POWER_ON', release)            # unbind/probe failure relights


if __name__ == '__main__':
    unittest.main()
