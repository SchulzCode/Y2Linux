"""Suspend-relevant charger state, separate from mere USB presence."""
# SPDX-License-Identifier: GPL-2.0-only
import re
from .common import read

# The kernel refuses system sleep in y2_charger_prepare_pm only while the
# charger engine is active or failed to stop (mt6323-charger.c). USB online
# with a HOLD/FULL/INHIBITED engine or a "Not charging" battery is NOT a
# refusal condition. Fix01's harness equated USB online with charging.
ACTIVE_PHASES = ('PRECHARGE', 'CONSTANT_CURRENT', 'CONSTANT_VOLTAGE')


def classify(charging_state, usb_online, battery_status):
    fields = dict(re.findall(r'(\w+)=(-?[\w.]+)', charging_state or ''))
    phase = fields.get('phase')
    active = fields.get('active') == '1'
    stop_error = fields.get('stop_error') not in (None, '0')
    if charging_state is None:
        category = 'unknown'
    elif stop_error:
        category = 'charger_stop_error'
    elif active:
        category = 'charging_active'
    elif phase in ('HOLD', 'FULL'):
        category = 'charger_hold'
    elif usb_online:
        category = 'usb_present_not_charging'
    else:
        category = 'no_usb'
    return {'category': category, 'usb_present': bool(usb_online), 'phase': phase,
            'charging_active': active, 'stop_error': stop_error,
            'battery_status': battery_status,
            'expect_refusal': active or stop_error,
            'phase_consistent': (phase in ACTIVE_PHASES) == active if phase else None}


def observe(ctx):
    online = [read(p) for p in ctx.glob('/sys/class/power_supply/*/online')]
    return classify(ctx.read('/sys/class/power_supply/BAT0/charging_state'),
                    any(v == '1' for v in online),
                    ctx.read('/sys/class/power_supply/BAT0/status'))
