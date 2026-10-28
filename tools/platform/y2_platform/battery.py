"""Battery SOC policy; estimates never create measured current or temperature."""
# SPDX-License-Identifier: GPL-2.0-only
from collections import deque
import math
import time

PROFILE = '/etc/y2linux/battery-profile.json'
OVERRIDE = '/data/system/platform/battery-profile.json'
# POWER-FINAL-28: the displayed percentage survives a restart, as stock keeps
# UI_SOC in an MT6323 RTC spare register (set_rtc_spare_fg_value, ledger F29).
UI_STATE = '/data/system/platform/battery-ui.json'
# Stock dod_init reuses the saved value only within 40 points of the boot
# estimate; oam_init prefers the power-on OCV unless a charger moved it 31+.
UI_RESTORE_POINTS = 40
OCV_CHARGER_POINTS = 31
# F36 (97f11bb1): the charging screen (offline-charge) can run an hour before
# the platform's first sample; the power-on OCV then predates that charge.
BOOT_OCV_FRESH_SECONDS = 120
# Stock dod_init (0xc039be10) keeps the saved UI_SOC whatever the boot sample
# says after boot_reason 3..6 (BR_WDT, BR_WDT_BY_PASS_PWK, BR_TOOL_BY_PASS_PWK,
# BR_2SEC_REBOOT). The PMIC stayed on, so 0x724 still holds the OCV of the last
# cold start (F54, 98499743: an update restart at 95 % showed 13 % at 4.02 V).
BOOT_METADATA = '/sys/firmware/y2_boot/metadata'
WARM_BOOT_REASONS = (3, 4, 5, 6)
# A rested OCV is never below the loaded battery voltage (ADC tolerance aside);
# boot load sags 60-120 mV below it (ledger F30), never 300 mV (oam_run clamp).
OCV_BELOW_LOAD_UV = 50000
OCV_ABOVE_LOAD_UV = 300000
# Stock mt_battery_update_status: at Full the display counts up one point per
# 10-s battery-thread tick to 100 (100Percent tracking).
FULL_STEP_SECONDS = 10
# Stock oam_run integrates over getrawmonotonic, i.e. awake time only. Longer
# gaps are suspend: the voltage after them is rested and taken directly.
OAM_MAX_STEP_SECONDS = 60
# The charger tops off at 4.175 V (28: 70 mA finish stage), so the curve's top
# point is never seen at rest. The kernel reports Full only after a hold that
# rests at or above 4.07 V; 4.0 V is the plausibility floor for that claim.
FULL_FLOOR_UV = 4000000


def load_profile(ctx):
    value = ctx.json(OVERRIDE, None)
    if value is None:
        value = ctx.json(PROFILE, None)
    if not isinstance(value, dict) or value.get('schema') != 1:
        return None
    points = value.get('voltage_soc')
    if not ascending_table(points, 0, 100, True) or points[0][1] != 0 or points[-1][1] != 100:
        return None
    # Stock r_profile (POWER-FINAL-16): internal resistance by voltage, used
    # with the charger-path current exactly where stock oam_run uses it.
    resistance = value.get('voltage_resistance_milliohm')
    if resistance is not None and not ascending_table(resistance, 20, 1000, False):
        return None
    capacity = value.get('design_capacity_uah')
    if capacity is not None and (type(capacity) is not int or not 100000 <= capacity <= 5000000):
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


def ascending_table(points, lower, upper, increasing):
    """[[uv, value], ...]: 2..128 rows, strictly ascending voltage, bounded values."""
    if not isinstance(points, list) or not 2 <= len(points) <= 128:
        return False
    previous = (0, lower - 1)
    for point in points:
        if (not isinstance(point, list) or len(point) != 2 or
                any(type(x) is not int for x in point) or
                not 2500000 <= point[0] <= 4300000 or not lower <= point[1] <= upper or
                point[0] <= previous[0] or (increasing and point[1] <= previous[1])):
            return False
        previous = point
    return True


def voltage_at(points, percent):
    """Inverse of interpolate() on the ascending voltage/SOC curve."""
    if percent <= points[0][1]:
        return float(points[0][0])
    for (low, a), (high, b) in zip(points, points[1:]):
        if percent <= b:
            return low + (high - low) * (percent - a) / (b - a)
    return float(points[-1][0])


def interpolate(points, uv):
    if uv <= points[0][0]:
        return float(points[0][1])
    for (low, a), (high, b) in zip(points, points[1:]):
        if uv <= high:
            return a + (b - a) * (uv - low) / (high - low)
    return float(points[-1][1])


def boot_kind(text):
    """'warm' (SoC reset, PMIC on), 'charged' (LK's charging screen ran first:
    charge added while the saved value waited) or 'cold'; None if unknown."""
    fields = dict(f.split('=', 1) for f in (text or '').split() if '=' in f)
    try:
        if fields.get('valid') != '1':
            return None
        if fields.get('offline') == '1':
            return 'charged'
        return 'warm' if int(fields['boot_reason']) in WARM_BOOT_REASONS else 'cold'
    except (KeyError, ValueError):
        return None


class Soc:
    """Bounded filter and monotonic trend, resetting on loss/replacement of pack.

    The voltage curve is an estimate under load. Rest means stable voltage,
    not proof of open-circuit voltage. Current compensation is used only when
    a real battery current and separately calibrated resistance are supplied.
    """
    def __init__(self, profile, saved=None):
        self.profile = profile
        self.filtered = self.percent = self.last = None
        self.samples = deque(maxlen=3601)
        self.rest_since = None
        self.charging = None
        # Displayed value (stock UI_SOC), separate from the curve estimate.
        # `anchor` is the curve estimate at the last Full: that is 100 % of
        # what this charger fills (stock reset_parameter_dod_full, F28).
        self.ui = self.anchor = None
        self.full_latched = False
        self.full_step_at = None
        self.saved = saved if isinstance(saved, dict) else None
        # How the first sample started, e.g. 'warm/saved' (F54 diagnostics).
        self.start = None

    def state(self):
        """What survives a restart (stock RTC spare register), or None."""
        if self.ui is None:
            return None
        return {'schema': 1, 'ui_percent': int(self.ui),
                'anchor_percent': None if self.anchor is None else round(self.anchor, 2)}

    def display(self, estimate):
        if not self.anchor:
            return estimate
        return max(0., min(100., estimate * 100 / self.anchor))

    def restore(self, estimate, charging=False, boot=None):
        """First sample: reuse the saved display value within 40 points, or
        after a warm restart unless the live estimate is 40+ points above it."""
        saved = self.saved or {}
        anchor = saved.get('anchor_percent')
        if type(anchor) in (int, float) and 80 <= anchor <= 100:
            self.anchor = float(anchor)
        ui = saved.get('ui_percent')
        shown = self.display(estimate)
        # F36 (97f11bb1): a player that turned off at 5 % and charged while off
        # started at 5 % again and turned off at once, on the charger, for 13 h.
        # A charger only adds charge: a lower saved value gives way to the live one.
        # A warm restart had no time off to charge in; its first sample on a
        # charger reads high (F54, c0a4f35d: 28 % shown as 42 %).
        if (charging and boot != 'warm' or boot == 'charged') and type(ui) is int and ui < shown:
            ui = None
        restored = (type(ui) is int and 1 <= ui <= 100 and
                    (shown - ui if boot == 'warm' else abs(ui - shown)) < UI_RESTORE_POINTS)
        if restored:
            self.ui = ui
            # The estimate restarts there too (stock dod_init sets the gauge).
            self.percent = min(100., ui * (self.anchor or 100.) / 100)
        else:
            self.ui = int(round(shown))
        self.start = '%s/%s' % (boot or 'unknown', 'saved' if restored else self.start)
        self.saved = None

    def observe(self, now, voltage, present, status, hardware_percent=None,
                current=None, temperature=None, counter=None, charge_current=None,
                boot_ocv=None, boot=None):
        charging = status == 'Charging'
        result = dict(percent=None, source='unavailable', confidence='unavailable', status=status,
                      charging=charging, voltage_uv=voltage, current_ua=current,
                      temperature_millicelsius=temperature, charge_counter_uah=counter,
                      provisional=True, resting=False)
        valid_voltage = type(voltage) is int and 2500000 <= voltage <= 4400000
        if present != 1 or not valid_voltage or not math.isfinite(now):
            self.filtered = self.percent = self.last = None
            self.ui = self.anchor = None
            self.full_latched = False
            self.samples.clear()
            return result
        # Real elapsed time includes suspend. Capping a ten-minute observation
        # gap to one minute would conceal loss even with a fresh ADC reading.
        # The configured filter/rate and voltage curve remain unchanged.
        dt = max(0., now - self.last) if self.last is not None else 0.
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
        table = p.get('voltage_resistance_milliohm')
        oam = (charging and table is not None and type(charge_current) is int and
               0 <= charge_current <= 1200000)
        if oam:
            # Stock oam_run: OCV = V - I * R(V) while the charger drives current
            # in. The current is the charger-path sense estimate (200 mOhm basis).
            drop = charge_current * interpolate(table, self.filtered) / 1000
            estimate_uv = self.filtered - max(0, min(300000, drop))
        resistance = p.get('calibrated_resistance_milliohm')
        hybrid = type(current) is int and type(resistance) is int and 0 < resistance <= 1000
        if hybrid:
            # power_supply convention: positive current flows into the battery.
            estimate_uv -= max(-150000, min(150000, current * resistance / 1000))
        target = interpolate(p['voltage_soc'], estimate_uv)
        first = self.percent is None
        if first:
            self.start = 'live'
            # Stock oam_init: the PMIC's power-on OCV predates the boot load;
            # a charger that moved it by 31+ points leaves the live sample.
            # It is latched at PMIC power-on only: never after a warm restart.
            if (type(boot_ocv) is int and 3000000 <= boot_ocv <= 4400000 and
                    now <= BOOT_OCV_FRESH_SECONDS and boot not in ('warm', 'charged')):
                ocv = interpolate(p['voltage_soc'], boot_ocv)
                if status in ('Discharging', 'Unknown'):
                    if voltage - OCV_BELOW_LOAD_UV <= boot_ocv <= voltage + OCV_ABOVE_LOAD_UV:
                        target, self.start = ocv, 'ocv'
                elif abs(ocv - target) < OCV_CHARGER_POINTS:
                    target, self.start = ocv, 'ocv'
            self.percent = target
            self.restore(target, charging, boot)
        elif (status == 'Discharging' and not hybrid and table is not None and
              p.get('design_capacity_uah') and 0 < dt <= OAM_MAX_STEP_SECONDS):
            # POWER-FINAL-29: stock oam_run on battery. The model current is
            # (OCV(SOC) - V) / R(V); integrated over awake time it follows the
            # real drain instead of settling at the loaded-voltage SOC, which
            # sits I*R (about 50 mV, 4-5 points) low whenever the Y2 is busy.
            model_ma = (voltage_at(p['voltage_soc'], self.percent) - self.filtered) / \
                interpolate(table, self.filtered)
            self.percent -= model_ma * dt / 36. / (p['design_capacity_uah'] / 1000.)
            self.percent = max(0., min(100., self.percent))
        else:
            # Positive net current is unknown unless measured. A charging state
            # permits recovery but never itself proves a full battery.
            if not charging and status != 'Full':
                target = min(target, self.percent)
            change = dt / p['seconds_per_percent']
            self.percent += max(-change, min(change, target - self.percent))
        full = status == 'Full' and voltage >= FULL_FLOOR_UV
        if voltage <= p['voltage_soc'][0][0]:
            self.percent = min(self.percent, 1.)
        self.track(now, status, full, target)
        if voltage <= p['voltage_soc'][0][0]:
            self.ui = min(self.ui, 1)
        self.charging = charging
        capacity = p.get('design_capacity_uah')
        if capacity is not None:
            result.update(design_capacity_uah=capacity,
                          remaining_uah_estimate=round(capacity * self.percent / 100))
        result.update(percent=int(self.ui), estimate_percent=int(round(self.display(self.percent))),
                      anchor_percent=None if self.anchor is None else round(self.anchor, 2),
                      full_latched=self.full_latched,
                      source='hybrid' if hybrid else 'stock_oam' if oam else 'voltage_estimate',
                      confidence='provisional' if p['provisional'] else 'calibrated_estimate',
                      provisional=p['provisional'], resting=resting, filtered_voltage_uv=round(self.filtered),
                      start=self.start,
                      calibration_source=p['source'])
        return result


    def track(self, now, status, full, target):
        """Stock mt_battery_update_status, on the display value only."""
        shown = self.display(self.percent)
        if status == 'Discharging':
            # Charger gone: the display only follows the estimate down.
            self.full_latched = False
            self.full_step_at = None
            self.ui = min(self.ui, int(round(shown)))
            return
        if full and not self.full_latched:
            # Count up one point per tick, then re-anchor at this Full.
            if self.ui < 100:
                if self.full_step_at is None or now >= self.full_step_at + FULL_STEP_SECONDS:
                    self.ui += 1
                    self.full_step_at = now
                if self.ui < 100:
                    return
            self.full_latched = True
            self.full_step_at = None
            # The kernel says Full only after a rest: anchor on that voltage,
            # not on the rate-limited estimate that may still be catching up.
            self.anchor = max(80., min(100., target))
            self.percent = self.anchor
            return
        if self.full_latched:
            # Maintenance holds and recharges after Full stay at 100 until the
            # charger is removed (stock bat_in_recharging_state); the estimate
            # stays at the Full anchor as stock's gauge does after its reset.
            # Unplugged, it falls from there at the normal rate (stock Sync).
            self.percent = self.anchor
            self.ui = 100
            return
        # Charging or holding, not yet Full: follow the estimate up, at most 99.
        self.ui = min(99, max(self.ui, int(round(shown))))


def sample(ctx, engine, now=None):
    if now is None:
        now = time.clock_gettime(time.CLOCK_BOOTTIME)
    base = '/sys/class/power_supply/BAT0/'
    # Read BAT0 only: PMIC/CPU temperatures and USB configured limits cannot
    # become pack sensor values. Missing hardware properties stay null.
    temp = ctx.integer(base + 'temp')
    from .observe import charge_field
    return engine.observe(now, ctx.integer(base + 'voltage_now'), ctx.integer(base + 'present'),
                          ctx.read(base + 'status'), ctx.integer(base + 'capacity'),
                          ctx.integer(base + 'current_now'), temp * 100 if temp is not None else None,
                          ctx.integer(base + 'charge_counter'),
                          charge_field(ctx.read(base + 'charging_state'), 'sense_current_est_ua'),
                          ctx.integer(base + 'voltage_ocv'),
                          boot_kind(ctx.read(BOOT_METADATA)) if engine.percent is None else None)
