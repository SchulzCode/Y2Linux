"""POWER-FINAL-16/17/18/19/27/32 runtime power features, armed once the UI has drawn its first frame.

Each feature is a kernel parameter whose kernel default is the safe (15)
behaviour. This module turns them on at `application_ready`, never earlier: the
display domain must not be cut while the loader-lit panel is still being taken
over. The owner keeps a feature off for an A/B night with
/data/system/platform/power-features.json, e.g. {"dis_pdn": false, "golden_apply": 3, "md1_gate": false}.
A failure is recorded and never blocks boot, sleep or the UI.
"""
# SPDX-License-Identifier: GPL-2.0-only
import os
from .common import atomic_json

OVERRIDE = '/data/system/platform/power-features.json'
STATE = '/run/y2/power-features.json'
FEATURES = {
    # G1: display (DIS) power domain off at screen-off and in suspend.
    'dis_pdn': ('/sys/module/spm/parameters/dis_pdn', True),
    # G3b: golden sleep entry, bitmask: 1 PLLs, 2 mux power-down, 4 PERI
    # gates, 8 INFRA gates (golden-apply-policy.h).
    'golden_apply': ('/sys/module/spm/parameters/golden_apply', 0xf),
    # 17: gate the never-used modem's peripheral clock requests (ledger F11);
    # re-checked at every sleep entry.
    'md1_gate': ('/sys/module/spm/parameters/md1_gate', True),
    # 18: stock PMIC suspend fields, bitmask: 1 audio clocks, 2 unused PMIC
    # clocks, 4 VREF18, 8 VSYS buck sleep state (27) (pmic-sleep-policy.h).
    'pmic_sleep': ('/sys/module/pwrap/parameters/pmic_sleep', 0xf),
    # 19: gate the AP's PMIC_WRAP clock (INFRA bit 23) for the PCM sleep only.
    'pwrap_gate': ('/sys/module/spm/parameters/pwrap_gate', True),
    # 27 (ledger F22): screen-off sends the stock GC9503V power-down sequence
    # and leaves reset high (panel.c); N keeps 26's reset-low sequence.
    'panel_stock_suspend': ('/sys/module/panel/parameters/stock_suspend', True),
    # 27 (ledger F20): the CS43131's VGP2 rail turns off while the DAC is
    # idle, as in stock; N keeps 26's always-on rail (y2-cs43131.c).
    'vgp2_idle_off': ('/sys/module/y2_cs43131/parameters/vgp2_idle_off', True),
    # 32 (ledger F34/F36): deep idle (SLIDLE/DORMANT) while the audio clock is
    # on, i.e. during screen-off playback, as stock dpidle; N keeps 31's rule.
    'deep_idle_audio': ('/sys/module/clocks/parameters/deep_idle_audio', True),
    # #45: screen-off powers the panel down with its stock lcm_suspend and
    # screen-on replays the stock lcm_init (panel.c power_down); N keeps the
    # LK-lit panel awake as before. On by default only for a panel it was
    # confirmed on (CONFIRMED_PANELS); others turn it on with the override.
    'panel_power_down': ('/sys/module/panel/parameters/power_down', True),
}
# The ST7701S was confirmed on the owner's player (2026-10-18, 6456). The
# GC9503V's stock-packet init has not run on hardware yet.
CONFIRMED_PANELS = {'st7701s'}


PANEL_RECORD = '/data/system/platform/panel.json'
PANEL_HINT = '/sys/module/panel/parameters/panel_hint'
PANEL_HINTS = {'gc9503v': 1, 'st7701s': 2}
PANEL_KIND = '/sys/module/panel/parameters/panel_kind'


def panel(ctx):
    """This player's panel: LK's identification (y2_boot/metadata panel=, only from
    lcm=1-, i.e. LK could read the panel), recorded on Y2DATA; on boots where LK
    could not (soft restarts report lcm=0- with a fallback name), the record.
    The kernel gets the record as panel_hint; it never overrides LK."""
    lk = None
    for field in (ctx.read('/sys/firmware/y2_boot/metadata') or '').split():
        if field.startswith('panel='):
            lk = field[len('panel='):]
    if lk not in PANEL_HINTS:
        # The driver's own ID read at the first screen-on (stock compare_id registers).
        lk = {str(v): k for k, v in PANEL_HINTS.items()}.get(ctx.read(PANEL_KIND))
    record = ctx.json(PANEL_RECORD, {})
    record = record if isinstance(record, dict) else {}
    if lk in PANEL_HINTS:
        if record.get('panel') != lk:
            try:
                path = ctx.path(PANEL_RECORD)
                path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
                atomic_json(path, {'schema': 1, 'panel': lk, 'lcm': ctx.read('/sys/firmware/y2_boot/lcm')},
                            durable=True)
            except OSError:
                pass
        return lk
    remembered = record.get('panel')
    if remembered in PANEL_HINTS:
        try:
            ctx.path(PANEL_HINT).write_text(str(PANEL_HINTS[remembered]))
        except OSError:
            return None
        return remembered
    return None


def encode(default, value):
    if isinstance(default, bool):
        return 'Y' if value is not False else 'N'
    if value is True:
        value = default
    if value is False:
        value = 0
    if type(value) is not int or not 0 <= value <= default:
        raise ValueError('invalid_power_feature')
    return str(value)


def apply(ctx):
    override = ctx.json(OVERRIDE, {})
    if not isinstance(override, dict):
        override = {}
    result = {}
    for name, (path, default) in FEATURES.items():
        if name == 'panel_power_down' and name not in override:
            wanted = panel(ctx) in CONFIRMED_PANELS
        else:
            wanted = override.get(name, default)
        try:
            value = encode(default, wanted)
            ctx.path(path).write_text(value)
            result[name] = {'wanted': wanted, 'value': ctx.read(path)}
        except (OSError, ValueError) as error:
            result[name] = {'wanted': wanted, 'error': getattr(error, 'strerror', None) or str(error)}
    try:
        runtime = ctx.path(STATE)
        runtime.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        atomic_json(runtime, {'schema': 1, 'features': result})
    except OSError:
        pass
    return result
