//! `battery.py`: battery SOC policy; estimates never create measured current or temperature.
//!
//! Every float operation is Python's, in Python's order (int operands converted where Python converts
//! them, int/int true division on exact operands), so the percentages match the Python daemon's.
// SPDX-License-Identifier: GPL-2.0-only
use serde_json::Value;
use std::collections::VecDeque;
use y2p_core::{
    power_json::{Json, Object},
    power_text::{python_split, sense_current},
    py::{eq_int, exact_int},
    root::python_int,
    Root,
};

pub const PROFILE: &str = "/etc/y2linux/battery-profile.json";
pub const OVERRIDE: &str = "/data/system/platform/battery-profile.json";
// POWER-FINAL-28: the displayed percentage survives a restart, as stock keeps
// UI_SOC in an MT6323 RTC spare register (set_rtc_spare_fg_value, ledger F29).
pub const UI_STATE: &str = "/data/system/platform/battery-ui.json";
// Stock dod_init reuses the saved value only within 40 points of the boot
// estimate; oam_init prefers the power-on OCV unless a charger moved it 31+.
const UI_RESTORE_POINTS: f64 = 40.0;
const OCV_CHARGER_POINTS: f64 = 31.0;
// F36 (97f11bb1): the charging screen (offline-charge) can run an hour before
// the platform's first sample; the power-on OCV then predates that charge.
const BOOT_OCV_FRESH_SECONDS: f64 = 120.0;
// Stock dod_init (0xc039be10) keeps the saved UI_SOC whatever the boot sample
// says after boot_reason 3..6 (BR_WDT, BR_WDT_BY_PASS_PWK, BR_TOOL_BY_PASS_PWK,
// BR_2SEC_REBOOT). The PMIC stayed on, so 0x724 still holds the OCV of the last
// cold start (F54, 98499743: an update restart at 95 % showed 13 % at 4.02 V).
pub const BOOT_METADATA: &str = "/sys/firmware/y2_boot/metadata";
const WARM_BOOT_REASONS: [i64; 4] = [3, 4, 5, 6];
// A rested OCV is never below the loaded battery voltage (ADC tolerance aside);
// boot load sags 60-120 mV below it (ledger F30), never 300 mV (oam_run clamp).
const OCV_BELOW_LOAD_UV: i64 = 50000;
const OCV_ABOVE_LOAD_UV: i64 = 300000;
// Stock mt_battery_update_status: at Full the display counts up one point per
// 10-s battery-thread tick to 100 (100Percent tracking).
const FULL_STEP_SECONDS: f64 = 10.0;
// Stock oam_run integrates over getrawmonotonic, i.e. awake time only. Longer
// gaps are suspend: the voltage after them is rested and taken directly.
const OAM_MAX_STEP_SECONDS: f64 = 60.0;
// The charger tops off at 4.175 V (28: 70 mA finish stage), so the curve's top
// point is never seen at rest. The kernel reports Full only after a hold that
// rests at or above 4.07 V; 4.0 V is the plausibility floor for that claim.
const FULL_FLOOR_UV: i64 = 4000000;
/// `deque(maxlen=3601)`.
const SAMPLES: usize = 3601;

pub type Table = Vec<(i64, i64)>;

/// A validated battery profile (`load_profile`).
#[derive(Clone, Debug)]
pub struct Profile {
    pub voltage_soc: Table,
    /// Stock r_profile (POWER-FINAL-16).
    pub resistance: Option<Table>,
    pub design_capacity_uah: Option<i64>,
    pub filter_seconds: i64,
    pub rest_seconds: i64,
    pub rest_band_uv: i64,
    pub charging_offset_uv: i64,
    pub seconds_per_percent: i64,
    pub provisional: bool,
    pub source: String,
    /// `p.get('calibrated_resistance_milliohm')` when it is an int.
    pub calibrated_resistance: Option<i64>,
}

/// `[[uv, value], ...]`: 2..128 rows, strictly ascending voltage, bounded values.
pub fn ascending_table(
    points: Option<&Value>,
    lower: i64,
    upper: i64,
    increasing: bool,
) -> Option<Table> {
    let rows = points?.as_array()?;
    if !(2..=128).contains(&rows.len()) {
        return None;
    }
    let mut previous = (0, lower - 1);
    let mut table = Vec::new();
    for row in rows {
        let pair = row.as_array().filter(|p| p.len() == 2)?;
        let (uv, value) = (exact_int(Some(&pair[0]))?, exact_int(Some(&pair[1]))?);
        if !(2500000..=4300000).contains(&uv)
            || !(lower..=upper).contains(&value)
            || uv <= previous.0
            || (increasing && value <= previous.1)
        {
            return None;
        }
        previous = (uv, value);
        table.push(previous);
    }
    Some(table)
}

pub fn load_profile(root: &Root) -> Option<Profile> {
    let value = match root.json(OVERRIDE) {
        None | Some(Value::Null) => root.json(PROFILE),
        found => found,
    };
    let value = value?;
    let value = value.as_object()?;
    if !eq_int(value.get("schema"), 1) {
        return None;
    }
    let voltage_soc = ascending_table(value.get("voltage_soc"), 0, 100, true)?;
    if voltage_soc[0].1 != 0 || voltage_soc[voltage_soc.len() - 1].1 != 100 {
        return None;
    }
    let resistance = match value.get("voltage_resistance_milliohm") {
        None | Some(Value::Null) => None,
        table => Some(ascending_table(table, 20, 1000, false)?),
    };
    let design_capacity_uah = match value.get("design_capacity_uah") {
        None | Some(Value::Null) => None,
        capacity => Some(exact_int(capacity).filter(|c| (100000..=5000000).contains(c))?),
    };
    let bounded = |name: &str, lower: i64, upper: i64| {
        exact_int(value.get(name)).filter(|v| (lower..=upper).contains(v))
    };
    let filter_seconds = bounded("filter_seconds", 1, 300)?;
    let rest_seconds = bounded("rest_seconds", 10, 3600)?;
    let rest_band_uv = bounded("rest_band_uv", 1000, 100000)?;
    let charging_offset_uv = bounded("charging_offset_uv", 0, 200000)?;
    let seconds_per_percent = bounded("seconds_per_percent", 10, 600)?;
    let provisional = value.get("provisional")?.as_bool()?;
    let source = value.get("source")?.as_str()?.to_owned();
    Some(Profile {
        voltage_soc,
        resistance,
        design_capacity_uah,
        filter_seconds,
        rest_seconds,
        rest_band_uv,
        charging_offset_uv,
        seconds_per_percent,
        provisional,
        source,
        calibrated_resistance: exact_int(value.get("calibrated_resistance_milliohm")),
    })
}

/// Inverse of `interpolate()` on the ascending voltage/SOC curve.
pub fn voltage_at(points: &Table, percent: f64) -> f64 {
    if percent <= points[0].1 as f64 {
        return points[0].0 as f64;
    }
    for pair in points.windows(2) {
        let ((low, a), (high, b)) = (pair[0], pair[1]);
        if percent <= b as f64 {
            return low as f64 + (high - low) as f64 * (percent - a as f64) / (b - a) as f64;
        }
    }
    points[points.len() - 1].0 as f64
}

/// `interpolate(points, uv)` for a float voltage.
pub fn interpolate(points: &Table, uv: f64) -> f64 {
    if uv <= points[0].0 as f64 {
        return points[0].1 as f64;
    }
    for pair in points.windows(2) {
        let ((low, a), (high, b)) = (pair[0], pair[1]);
        if uv <= high as f64 {
            return a as f64 + (b - a) as f64 * (uv - low as f64) / (high - low) as f64;
        }
    }
    points[points.len() - 1].1 as f64
}

/// `interpolate(points, uv)` for an int voltage: `(b - a) * (uv - low) / (high - low)` is int/int true
/// division in Python, exact operands divided once.
pub fn interpolate_int(points: &Table, uv: i64) -> f64 {
    if uv <= points[0].0 {
        return points[0].1 as f64;
    }
    for pair in points.windows(2) {
        let ((low, a), (high, b)) = (pair[0], pair[1]);
        if uv <= high {
            return a as f64 + ((b - a) * (uv - low)) as f64 / (high - low) as f64;
        }
    }
    points[points.len() - 1].1 as f64
}

/// `boot_kind(text)`: 'warm' (SoC reset, PMIC on), 'charged' (LK's charging screen ran first: charge
/// added while the saved value waited) or 'cold'; `None` if unknown.
pub fn boot_kind(text: Option<&str>) -> Option<&'static str> {
    let mut fields: Vec<(&str, &str)> = Vec::new();
    for field in python_split(text.unwrap_or_default()) {
        if let Some((key, value)) = field.split_once('=') {
            match fields.iter_mut().find(|(k, _)| *k == key) {
                Some(entry) => entry.1 = value,
                None => fields.push((key, value)),
            }
        }
    }
    let get = |key: &str| fields.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
    if get("valid") != Some("1") {
        return None;
    }
    if get("offline") == Some("1") {
        return Some("charged");
    }
    let reason = python_int(get("boot_reason")?)?;
    Some(if WARM_BOOT_REASONS.contains(&reason) {
        "warm"
    } else {
        "cold"
    })
}

/// Python `round(x)` to an int (ties to even).
fn round_int(x: f64) -> i64 {
    x.round_ties_even() as i64
}

/// Python `round(x, 2)`: the decimal rounding of the exact binary value (ties to even), read back.
pub fn round2(x: f64) -> f64 {
    format!("{x:.2}").parse().expect("float")
}

/// Python `min(a, b)` / `max(a, b)` on floats: the first argument unless the second is smaller/larger.
fn py_min(a: f64, b: f64) -> f64 {
    if b < a {
        b
    } else {
        a
    }
}

fn py_max(a: f64, b: f64) -> f64 {
    if b > a {
        b
    } else {
        a
    }
}

/// One BAT0 reading (`sample()`'s arguments to `Soc.observe`).
#[derive(Clone, Debug, Default)]
pub struct Reading {
    pub voltage: Option<i64>,
    pub present: Option<i64>,
    pub status: Option<String>,
    pub hardware_percent: Option<i64>,
    pub current: Option<i64>,
    pub temperature: Option<i64>,
    pub counter: Option<i64>,
    pub charge_current: Option<i64>,
    pub boot_ocv: Option<i64>,
    pub boot: Option<&'static str>,
}

/// `Soc`: bounded filter and monotonic trend, resetting on loss/replacement of pack.
///
/// The voltage curve is an estimate under load. Rest means stable voltage, not proof of open-circuit
/// voltage. Current compensation is used only when a real battery current and separately calibrated
/// resistance are supplied.
#[derive(Clone, Debug)]
pub struct Soc {
    profile: Option<Profile>,
    filtered: Option<f64>,
    pub percent: Option<f64>,
    last: Option<f64>,
    samples: VecDeque<(f64, i64)>,
    // Displayed value (stock UI_SOC), separate from the curve estimate.
    // `anchor` is the curve estimate at the last Full: that is 100 % of
    // what this charger fills (stock reset_parameter_dod_full, F28).
    ui: Option<i64>,
    anchor: Option<f64>,
    full_latched: bool,
    full_step_at: Option<f64>,
    saved: Option<Object>,
    // How the first sample started, e.g. 'warm/saved' (F54 diagnostics).
    start: Option<String>,
}

impl Soc {
    pub fn new(profile: Option<Profile>, saved: Option<Json>) -> Soc {
        Soc {
            profile,
            filtered: None,
            percent: None,
            last: None,
            samples: VecDeque::new(),
            ui: None,
            anchor: None,
            full_latched: false,
            full_step_at: None,
            saved: match saved {
                Some(Json::Object(o)) => Some(o),
                _ => None,
            },
            start: None,
        }
    }

    /// What survives a restart (stock RTC spare register), or `None`: (ui_percent, anchor_percent).
    pub fn state(&self) -> Option<(i64, Option<f64>)> {
        Some((self.ui?, self.anchor.map(round2)))
    }

    fn display(&self, estimate: f64) -> f64 {
        match self.anchor {
            Some(anchor) if anchor != 0.0 => py_max(0., py_min(100., estimate * 100. / anchor)),
            _ => estimate,
        }
    }

    /// The display value; set by the first sample (restore) or a fuel-gauge sample.
    fn ui(&self) -> i64 {
        self.ui.expect("set by the first sample")
    }

    /// First sample: reuse the saved display value within 40 points, or after a warm restart unless
    /// the live estimate is 40+ points above it.
    fn restore(&mut self, estimate: f64, charging: bool, boot: Option<&str>) {
        let saved = self.saved.take().unwrap_or_default();
        if let Some(Json::Number(n)) = saved.get("anchor_percent") {
            let value = n.as_f64().unwrap_or(f64::NAN);
            if (80.0..=100.0).contains(&value) {
                self.anchor = Some(value);
            }
        }
        let mut ui = saved.get("ui_percent").and_then(Json::exact_int);
        let shown = self.display(estimate);
        // F36 (97f11bb1): a player that turned off at 5 % and charged while off
        // started at 5 % again and turned off at once, on the charger, for 13 h.
        // A charger only adds charge: a lower saved value gives way to the live one.
        // A warm restart had no time off to charge in; its first sample on a
        // charger reads high (F54, c0a4f35d: 28 % shown as 42 %).
        if ((charging && boot != Some("warm")) || boot == Some("charged"))
            && ui.is_some_and(|u| (u as f64) < shown)
        {
            ui = None;
        }
        let restored = ui.filter(|u| {
            (1..=100).contains(u)
                && if boot == Some("warm") {
                    shown - *u as f64
                } else {
                    (*u as f64 - shown).abs()
                } < UI_RESTORE_POINTS
        });
        if let Some(ui) = restored {
            self.ui = Some(ui);
            // The estimate restarts there too (stock dod_init sets the gauge).
            let anchor = self.anchor.filter(|a| *a != 0.0).unwrap_or(100.);
            self.percent = Some(py_min(100., ui as f64 * anchor / 100.));
        } else {
            self.ui = Some(round_int(shown));
        }
        self.start = Some(format!(
            "{}/{}",
            boot.unwrap_or("unknown"),
            if restored.is_some() {
                "saved"
            } else {
                self.start.as_deref().unwrap_or("None")
            }
        ));
    }

    pub fn observe(&mut self, now: f64, r: &Reading) -> Object {
        let charging = r.status.as_deref() == Some("Charging");
        let mut result = Object::new()
            .with("percent", Json::Null)
            .with("source", "unavailable")
            .with("confidence", "unavailable")
            .with("status", r.status.clone())
            .with("charging", charging)
            .with("voltage_uv", r.voltage)
            .with("current_ua", r.current)
            .with("temperature_millicelsius", r.temperature)
            .with("charge_counter_uah", r.counter)
            .with("provisional", true)
            .with("resting", false);
        let valid_voltage = r.voltage.is_some_and(|v| (2500000..=4400000).contains(&v));
        if r.present != Some(1) || !valid_voltage || !now.is_finite() {
            self.filtered = None;
            self.percent = None;
            self.last = None;
            self.ui = None;
            self.anchor = None;
            self.full_latched = false;
            self.samples.clear();
            return result;
        }
        let voltage = r.voltage.expect("valid");
        // Real elapsed time includes suspend. Capping a ten-minute observation
        // gap to one minute would conceal loss even with a fresh ADC reading.
        // The configured filter/rate and voltage curve remain unchanged.
        let dt = self.last.map_or(0., |last| py_max(0., now - last));
        if self.last.is_some_and(|last| now < last) {
            *self = Soc::new(self.profile.take(), None);
        }
        self.last = Some(now);
        if let Some(hardware) = r.hardware_percent.filter(|p| (0..=100).contains(p)) {
            // Only BAT0's genuine capacity property can select FuelGaugeSoc.
            self.percent = Some(hardware as f64);
            result
                .insert("percent", hardware)
                .insert("source", "fuel_gauge")
                .insert("confidence", "hardware")
                .insert("provisional", false);
            return result;
        }
        let Some(p) = self.profile.clone() else {
            return result;
        };
        let filtered = match self.filtered {
            None => voltage as f64,
            Some(f) => f + (voltage as f64 - f) * dt / (p.filter_seconds as f64 + dt),
        };
        self.filtered = Some(filtered);
        if self.samples.len() == SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back((now, voltage));
        while self
            .samples
            .front()
            .is_some_and(|s| now - s.0 > p.rest_seconds as f64)
        {
            self.samples.pop_front();
        }
        let resting = self.samples.len() > 1
            && now - self.samples[0].0 >= (p.rest_seconds - 1) as f64
            && self.samples.iter().map(|s| s.1).max().unwrap_or(0)
                - self.samples.iter().map(|s| s.1).min().unwrap_or(0)
                <= p.rest_band_uv;
        let mut estimate_uv = filtered - if charging { p.charging_offset_uv } else { 0 } as f64;
        let table = p.resistance.as_ref();
        let oam = charging
            && table.is_some()
            && r.charge_current.is_some_and(|c| (0..=1200000).contains(&c));
        if oam {
            // Stock oam_run: OCV = V - I * R(V) while the charger drives current
            // in. The current is the charger-path sense estimate (200 mOhm basis).
            let drop = r.charge_current.expect("oam") as f64
                * interpolate(table.expect("oam"), filtered)
                / 1000.;
            estimate_uv = filtered - py_max(0., py_min(300000., drop));
        }
        let hybrid =
            r.current.is_some() && p.calibrated_resistance.is_some_and(|r| 0 < r && r <= 1000);
        if hybrid {
            // power_supply convention: positive current flows into the battery.
            let product = r.current.expect("hybrid") * p.calibrated_resistance.expect("hybrid");
            estimate_uv -= py_max(-150000., py_min(150000., product as f64 / 1000.));
        }
        let mut target = interpolate(&p.voltage_soc, estimate_uv);
        let status = r.status.as_deref();
        // After fuel-gauge samples the voltage estimate has no display value yet: it starts as a
        // first sample (the Python platform went on with `ui` None and its daemon ended on a
        // TypeError).
        if self.percent.is_none() || self.ui.is_none() {
            self.start = Some("live".into());
            // Stock oam_init: the PMIC's power-on OCV predates the boot load;
            // a charger that moved it by 31+ points leaves the live sample.
            // It is latched at PMIC power-on only: never after a warm restart.
            if let Some(ocv_uv) = r.boot_ocv.filter(|o| {
                (3000000..=4400000).contains(o)
                    && now <= BOOT_OCV_FRESH_SECONDS
                    && !matches!(r.boot, Some("warm" | "charged"))
            }) {
                let ocv = interpolate_int(&p.voltage_soc, ocv_uv);
                if matches!(status, Some("Discharging" | "Unknown")) {
                    if voltage - OCV_BELOW_LOAD_UV <= ocv_uv
                        && ocv_uv <= voltage + OCV_ABOVE_LOAD_UV
                    {
                        target = ocv;
                        self.start = Some("ocv".into());
                    }
                } else if (ocv - target).abs() < OCV_CHARGER_POINTS {
                    target = ocv;
                    self.start = Some("ocv".into());
                }
            }
            self.percent = Some(target);
            self.restore(target, charging, r.boot);
        } else if status == Some("Discharging")
            && !hybrid
            && table.is_some()
            && p.design_capacity_uah.is_some()
            && 0. < dt
            && dt <= OAM_MAX_STEP_SECONDS
        {
            // POWER-FINAL-29: stock oam_run on battery. The model current is
            // (OCV(SOC) - V) / R(V); integrated over awake time it follows the
            // real drain instead of settling at the loaded-voltage SOC, which
            // sits I*R (about 50 mV, 4-5 points) low whenever the Y2 is busy.
            let percent = self.percent.expect("not first");
            let model_ma = (voltage_at(&p.voltage_soc, percent) - filtered)
                / interpolate(table.expect("oam"), filtered);
            let capacity = p.design_capacity_uah.expect("capacity") as f64;
            let percent = percent - model_ma * dt / 36. / (capacity / 1000.);
            self.percent = Some(py_max(0., py_min(100., percent)));
        } else {
            // Positive net current is unknown unless measured. A charging state
            // permits recovery but never itself proves a full battery.
            let percent = self.percent.expect("not first");
            if !charging && status != Some("Full") {
                target = py_min(target, percent);
            }
            let change = dt / p.seconds_per_percent as f64;
            self.percent = Some(percent + py_max(-change, py_min(change, target - percent)));
        }
        let full = status == Some("Full") && voltage >= FULL_FLOOR_UV;
        if voltage <= p.voltage_soc[0].0 {
            self.percent = Some(py_min(self.percent.expect("set"), 1.));
        }
        self.track(now, status, full, target);
        if voltage <= p.voltage_soc[0].0 {
            self.ui = Some(self.ui().min(1));
        }
        let percent = self.percent.expect("set");
        if let Some(capacity) = p.design_capacity_uah {
            result.insert("design_capacity_uah", capacity).insert(
                "remaining_uah_estimate",
                round_int(capacity as f64 * percent / 100.),
            );
        }
        result
            .insert("percent", self.ui())
            .insert("estimate_percent", round_int(self.display(percent)))
            .insert("anchor_percent", self.anchor.map(round2))
            .insert("full_latched", self.full_latched)
            .insert(
                "source",
                if hybrid {
                    "hybrid"
                } else if oam {
                    "stock_oam"
                } else {
                    "voltage_estimate"
                },
            )
            .insert(
                "confidence",
                if p.provisional {
                    "provisional"
                } else {
                    "calibrated_estimate"
                },
            )
            .insert("provisional", p.provisional)
            .insert("resting", resting)
            .insert("filtered_voltage_uv", round_int(filtered))
            .insert("start", self.start.clone())
            .insert("calibration_source", p.source.clone());
        result
    }

    /// Stock mt_battery_update_status, on the display value only.
    fn track(&mut self, now: f64, status: Option<&str>, full: bool, target: f64) {
        let shown = self.display(self.percent.expect("set"));
        if status == Some("Discharging") {
            // Charger gone: the display only follows the estimate down.
            self.full_latched = false;
            self.full_step_at = None;
            self.ui = Some(self.ui().min(round_int(shown)));
            return;
        }
        if full && !self.full_latched {
            // Count up one point per tick, then re-anchor at this Full.
            if self.ui() < 100 {
                if self
                    .full_step_at
                    .is_none_or(|at| now >= at + FULL_STEP_SECONDS)
                {
                    self.ui = Some(self.ui() + 1);
                    self.full_step_at = Some(now);
                }
                if self.ui() < 100 {
                    return;
                }
            }
            self.full_latched = true;
            self.full_step_at = None;
            // The kernel says Full only after a rest: anchor on that voltage,
            // not on the rate-limited estimate that may still be catching up.
            let anchor = py_max(80., py_min(100., target));
            self.anchor = Some(anchor);
            self.percent = Some(anchor);
            return;
        }
        if self.full_latched {
            // Maintenance holds and recharges after Full stay at 100 until the
            // charger is removed (stock bat_in_recharging_state); the estimate
            // stays at the Full anchor as stock's gauge does after its reset.
            // Unplugged, it falls from there at the normal rate (stock Sync).
            self.percent = self.anchor;
            self.ui = Some(100);
            return;
        }
        // Charging or holding, not yet Full: follow the estimate up, at most 99.
        self.ui = Some(99.min(self.ui().max(round_int(shown))));
    }
}

/// `sample(ctx, engine)`: one BAT0 reading on CLOCK_BOOTTIME (physical elapsed time incl. suspend).
pub fn sample(root: &Root, engine: &mut Soc) -> Object {
    let now = y2p_core::clock::boottime_ns() as f64 / 1e9;
    sample_at(root, engine, now)
}

pub fn sample_at(root: &Root, engine: &mut Soc, now: f64) -> Object {
    let base = "/sys/class/power_supply/BAT0/";
    let int = |name: &str| root.integer(&format!("{base}{name}"));
    // Read BAT0 only: PMIC/CPU temperatures and USB configured limits cannot
    // become pack sensor values. Missing hardware properties stay null.
    let reading = Reading {
        voltage: int("voltage_now"),
        present: int("present"),
        status: root.read(&format!("{base}status")),
        hardware_percent: int("capacity"),
        current: int("current_now"),
        temperature: int("temp").map(|t| t * 100),
        counter: int("charge_counter"),
        charge_current: sense_current(root.read(&format!("{base}charging_state")).as_deref()),
        boot_ocv: int("voltage_ocv"),
        boot: if engine.percent.is_none() {
            boot_kind(root.read(BOOT_METADATA).as_deref())
        } else {
            None
        },
    };
    engine.observe(now, &reading)
}

#[cfg(test)]
mod tests {
    use super::*;
    use y2p_core::json::{dumps, Separators};

    fn profile() -> Profile {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../buildroot/board/y2/production-overlay/etc/y2linux/battery-profile.json"
        ))
        .unwrap();
        let dir = y2p_core_tempdir();
        std::fs::create_dir_all(dir.join("etc/y2linux")).unwrap();
        std::fs::write(dir.join("etc/y2linux/battery-profile.json"), text).unwrap();
        let profile = load_profile(&Root::new(&dir)).expect("stock profile");
        let _ = std::fs::remove_dir_all(&dir);
        profile
    }

    fn y2p_core_tempdir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        std::env::temp_dir().join(format!(
            "y2-battery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ))
    }

    #[test]
    fn a_sample_without_the_fuel_gauge_goes_on_from_its_percent() {
        let mut soc = Soc::new(Some(profile()), None);
        let reading = |hardware_percent| Reading {
            voltage: Some(3900000),
            present: Some(1),
            status: Some("Discharging".into()),
            hardware_percent,
            ..Reading::default()
        };
        assert_eq!(
            soc.observe(10.0, &reading(Some(55))).get("percent"),
            Some(&Json::from(55i64))
        );
        let next = soc.observe(11.0, &reading(None));
        // The voltage estimate takes over as on a first sample (no TypeError, no crash).
        assert_eq!(next.get("source"), Some(&Json::from("voltage_estimate")));
        assert!(next.get("percent").and_then(Json::exact_int).is_some());
        assert_eq!(next.get("start"), Some(&Json::from("unknown/live")));
    }

    #[test]
    fn rounding_and_boot_kinds_follow_python() {
        assert_eq!(round2(97.125), 97.12);
        assert_eq!(round_int(2.5), 2);
        assert_eq!(round_int(3.5), 4);
        assert_eq!(boot_kind(Some("valid=1 boot_reason=4")), Some("warm"));
        assert_eq!(
            boot_kind(Some("valid=1 offline=1 boot_reason=4")),
            Some("charged")
        );
        assert_eq!(boot_kind(Some("valid=1 boot_reason=0")), Some("cold"));
        assert_eq!(boot_kind(Some("valid=1 boot_reason=x")), None);
        assert_eq!(boot_kind(Some("valid=0 boot_reason=4")), None);
        assert_eq!(boot_kind(None), None);
    }

    /// Sequences of readings: every result as the Python platform's battery.Soc gave it (recorded).
    #[test]
    fn soc_matches_python_battery_py() {
        // (name, saved state, [(now, voltage, present, status, capacity, current, ocv, boot, charge_current)])
        type Step = (
            f64,
            i64,
            i64,
            &'static str,
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<&'static str>,
            Option<i64>,
        );
        let mut scenarios: Vec<(&str, &str, Vec<Step>)> = vec![
            (
                "discharge",
                "null",
                (0..40)
                    .map(|i| {
                        (
                            5000. + i as f64 * 7.3,
                            3_800_000 - i * 900,
                            1,
                            "Discharging",
                            None,
                            None,
                            None,
                            Some("cold"),
                            None,
                        )
                    })
                    .collect(),
            ),
            (
                "warm restart keeps 95 (F54)",
                r#"{"schema": 1, "ui_percent": 95, "anchor_percent": 97.5}"#,
                vec![
                    (
                        14.,
                        4_020_000,
                        1,
                        "Discharging",
                        None,
                        None,
                        Some(3_700_000),
                        Some("warm"),
                        None,
                    ),
                    (
                        15.1,
                        4_019_000,
                        1,
                        "Discharging",
                        None,
                        None,
                        None,
                        None,
                        None,
                    ),
                ],
            ),
            (
                "cold start uses the ocv",
                r#"{"schema": 1, "ui_percent": 13}"#,
                vec![
                    (
                        30.,
                        3_900_000,
                        1,
                        "Discharging",
                        None,
                        None,
                        Some(3_950_000),
                        Some("cold"),
                        None,
                    ),
                    (
                        31.,
                        3_899_000,
                        1,
                        "Discharging",
                        None,
                        None,
                        None,
                        None,
                        None,
                    ),
                ],
            ),
            (
                "charged boot drops a lower saved value (F36)",
                r#"{"schema": 1, "ui_percent": 5}"#,
                vec![
                    (
                        4000.,
                        4_027_000,
                        1,
                        "Charging",
                        None,
                        None,
                        Some(3_440_000),
                        Some("charged"),
                        Some(500_000),
                    ),
                    (
                        4001.5,
                        4_028_000,
                        1,
                        "Charging",
                        None,
                        None,
                        None,
                        None,
                        Some(480_000),
                    ),
                ],
            ),
            ("charging up to full", "null", {
                let mut v: Vec<Step> = (0..30)
                    .map(|i| {
                        (
                            100. + i as f64 * 3.1,
                            4_100_000 + i * 2_000,
                            1,
                            "Charging",
                            None,
                            None,
                            None,
                            Some("unknown"),
                            Some(300_000 - i * 9_000),
                        )
                    })
                    .collect();
                v.extend((0..30).map(|i| {
                    (
                        200. + i as f64 * 4.7,
                        4_175_000,
                        1,
                        "Full",
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                }));
                v.extend((0..5).map(|i| {
                    (
                        400. + i as f64,
                        4_150_000,
                        1,
                        "Discharging",
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                }));
                v
            }),
            (
                "hardware and pack removal",
                "null",
                vec![
                    (
                        10.,
                        3_700_000,
                        1,
                        "Discharging",
                        Some(55),
                        None,
                        None,
                        None,
                        None,
                    ),
                    (
                        11.,
                        3_700_000,
                        0,
                        "Discharging",
                        None,
                        None,
                        None,
                        None,
                        None,
                    ),
                    (
                        12.,
                        3_690_000,
                        1,
                        "Not charging",
                        None,
                        None,
                        None,
                        None,
                        None,
                    ),
                    (
                        5.,
                        3_690_000,
                        1,
                        "Discharging",
                        None,
                        None,
                        None,
                        None,
                        None,
                    ),
                    (
                        6.,
                        3_350_000,
                        1,
                        "Discharging",
                        None,
                        None,
                        None,
                        None,
                        None,
                    ),
                ],
            ),
        ];
        // `calibrated_resistance_milliohm` + `current_now`: the hybrid source.
        scenarios.push((
            "hybrid",
            "null",
            (0..10)
                .map(|i| {
                    (
                        50. + i as f64,
                        3_750_000,
                        1,
                        "Discharging",
                        None,
                        Some(-150_000 - i * 1000),
                        None,
                        None,
                        None,
                    )
                })
                .collect(),
        ));
        // Stock oam_init on a charger: the power-on OCV within 31 points wins, beyond it the live
        // sample; a stale OCV (after 120 s) and a sagging one on battery are ignored; status None.
        for (name, status, ocv, now) in [
            ("charger ocv near", "Charging", 3_980_000, 40.),
            ("charger ocv far", "Charging", 3_500_000, 40.),
            ("ocv too old", "Discharging", 3_950_000, 121.),
            ("ocv below load", "Discharging", 3_700_000, 20.),
            ("ocv no status", "", 3_950_000, 20.),
        ] {
            scenarios.push((
                name,
                r#"{"schema": 1, "ui_percent": 70, "anchor_percent": 90}"#,
                vec![
                    (
                        now,
                        3_900_000,
                        1,
                        status,
                        None,
                        None,
                        Some(ocv),
                        Some("cold"),
                        Some(200_000),
                    ),
                    (
                        now + 1.0,
                        3_901_000,
                        1,
                        status,
                        None,
                        None,
                        None,
                        None,
                        Some(200_000),
                    ),
                ],
            ));
        }
        let mut ours = String::new();
        for (name, saved, steps) in &scenarios {
            let mut p = profile();
            if *name == "hybrid" {
                p.calibrated_resistance = Some(180);
            }
            let mut soc = Soc::new(Some(p), Json::parse(saved.as_bytes()).ok());
            let mut out = Vec::new();
            for &(now, v, present, status, cap, cur, ocv, boot, cc) in steps {
                let reading = Reading {
                    voltage: Some(v),
                    present: Some(present),
                    status: Some(status.into()),
                    hardware_percent: cap,
                    current: cur,
                    charge_current: cc,
                    boot_ocv: ocv,
                    boot,
                    ..Reading::default()
                };
                let result = soc.observe(now, &reading);
                let state = soc.state().map(|(ui, anchor)| {
                    Object::new()
                        .with("schema", 1i64)
                        .with("ui_percent", ui)
                        .with("anchor_percent", anchor)
                });
                out.push(Json::Array(vec![result.into(), state.into()]));
            }
            ours += &(dumps(&out, Separators::Spaced).unwrap() + "\n");
        }
        y2p_core::recorded::check(
            env!("CARGO_MANIFEST_DIR"),
            "battery-soc.jsonl",
            ours.as_bytes(),
        );
    }
}
