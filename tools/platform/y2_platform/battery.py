"""Battery SOC policy; estimates never create measured current or temperature."""
# SPDX-License-Identifier: GPL-2.0-only
from collections import deque
import math

PROFILE = '/etc/y2linux/battery-profile.json'
OVERRIDE = '/data/system/platform/battery-profile.json'


def load_profile(ctx):
    value = ctx.json(OVERRIDE, None)
    if value is None:
        value = ctx.json(PROFILE, None)
    if not isinstance(value, dict) or value.get('schema') != 1:
        return None
    points = value.get('voltage_soc')
    if not isinstance(points, list) or not 2 <= len(points) <= 32:
        return None
    previous = (0, -1)
    for point in points:
        if (not isinstance(point, list) or len(point) != 2 or
                any(type(x) is not int for x in point) or
                not 2500000 <= point[0] <= 4300000 or not 0 <= point[1] <= 100 or
                point[0] <= previous[0] or point[1] <= previous[1]):
            return None
        previous = point
    if points[0][1] != 0 or points[-1][1] != 100:
        return None
    for name, lower, upper in [('filter_seconds', 1, 300), ('rest_seconds', 10, 3600),
                               ('rest_band_uv', 1000, 100000),
                               ('charging_offset_uv', 0, 200000),
                               ('seconds_per_percent', 10, 600)]:
        if type(value.get(name)) is not int or not lower <= value[name] <= upper:
            return None
    if type(value.get('provisional')) is not bool or not isinstance(value.get('source'), str):
        return None
    return value


def interpolate(points, uv):
    if uv <= points[0][0]:
        return 0.
    for (low, a), (high, b) in zip(points, points[1:]):
        if uv <= high:
            return a + (b - a) * (uv - low) / (high - low)
    return 100.


class Soc:
    """Bounded filter and monotonic trend, resetting on loss/replacement of pack.

    The voltage curve is an estimate under load. Rest means stable voltage,
    not proof of open-circuit voltage. Current compensation is used only when
    a real battery current and separately calibrated resistance are supplied.
    """
    def __init__(self, profile):
        self.profile = profile
        self.filtered = self.percent = self.last = None
        self.samples = deque(maxlen=3601)
        self.rest_since = None
        self.charging = None

    def observe(self, now, voltage, present, status, hardware_percent=None,
                current=None, temperature=None, counter=None):
        charging = status == 'Charging'
        result = dict(percent=None, source='unavailable', confidence='unavailable',
                      charging=charging, voltage_uv=voltage, current_ua=current,
                      temperature_millicelsius=temperature, charge_counter_uah=counter,
                      provisional=True, resting=False)
        valid_voltage = type(voltage) is int and 2500000 <= voltage <= 4400000
        if present != 1 or not valid_voltage or not math.isfinite(now):
            self.filtered = self.percent = self.last = None
            self.samples.clear()
            return result
        dt = max(0., min(60., now - self.last)) if self.last is not None else 0.
        if self.last is not None and now < self.last:
            self.__init__(self.profile)
        self.last = now
        if type(hardware_percent) is int and 0 <= hardware_percent <= 100:
            # Only BAT0's genuine capacity property can select FuelGaugeSoc.
            self.percent = float(hardware_percent)
            result.update(percent=hardware_percent, source='fuel_gauge',
                          confidence='hardware', provisional=False)
            return result
        p = self.profile
        if p is None:
            return result
        self.filtered = (float(voltage) if self.filtered is None else
                         self.filtered + (voltage - self.filtered) * dt / (p['filter_seconds'] + dt))
        self.samples.append((now, voltage))
        while self.samples and now - self.samples[0][0] > p['rest_seconds']:
            self.samples.popleft()
        resting = (len(self.samples) > 1 and now - self.samples[0][0] >= p['rest_seconds'] - 1 and
                   max(v for _, v in self.samples) - min(v for _, v in self.samples) <= p['rest_band_uv'])
        estimate_uv = self.filtered - (p['charging_offset_uv'] if charging else 0)
        resistance = p.get('calibrated_resistance_milliohm')
        hybrid = type(current) is int and type(resistance) is int and 0 < resistance <= 1000
        if hybrid:
            # power_supply convention: positive current flows into the battery.
            estimate_uv -= max(-150000, min(150000, current * resistance / 1000))
        target = interpolate(p['voltage_soc'], estimate_uv)
        if self.percent is None:
            self.percent = target
        else:
            # Positive net current is unknown unless measured. A charging state
            # permits recovery but never itself proves a full battery.
            if not charging and status != 'Full':
                target = min(target, self.percent)
            change = dt / p['seconds_per_percent']
            self.percent += max(-change, min(change, target - self.percent))
        if status == 'Full' and voltage >= p['voltage_soc'][-1][0] - p['rest_band_uv']:
            self.percent = 100.
        if voltage <= p['voltage_soc'][0][0]:
            self.percent = min(self.percent, 1.)
        self.charging = charging
        result.update(percent=int(round(self.percent)), source='hybrid' if hybrid else 'voltage_estimate',
                      confidence='provisional' if p['provisional'] else 'calibrated_estimate',
                      provisional=p['provisional'], resting=resting, filtered_voltage_uv=round(self.filtered),
                      calibration_source=p['source'])
        return result


def sample(ctx, engine, now):
    base = '/sys/class/power_supply/BAT0/'
    # Read BAT0 only: PMIC/CPU temperatures and USB configured limits cannot
    # become pack sensor values. Missing hardware properties stay null.
    temp = ctx.integer(base + 'temp')
    return engine.observe(now, ctx.integer(base + 'voltage_now'), ctx.integer(base + 'present'),
                          ctx.read(base + 'status'), ctx.integer(base + 'capacity'),
                          ctx.integer(base + 'current_now'), temp * 100 if temp is not None else None,
                          ctx.integer(base + 'charge_counter'))
