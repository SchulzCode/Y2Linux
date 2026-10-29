//! The SOC estimate (stock oam_init/oam_run on the profile's curve): bounded filter, monotonic trend,
//! the power-on OCV at a cold start, the hybrid current correction.
//!
//! Every float operation is Python's, in Python's order (int operands converted where Python converts
//! them, int/int true division on exact operands), so the percentages match the Python daemon's.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    profile::{interpolate, interpolate_int, voltage_at, Profile},
    status::{BatteryStatus, Estimate, Saved},
    Reading,
};
use serde_json::Value;
use std::collections::VecDeque;

// Stock oam_init prefers the power-on OCV unless a charger moved it 31+ points.
const OCV_CHARGER_POINTS: f64 = 31.0;
// F36 (97f11bb1): the charging screen (offline-charge) can run an hour before
// the platform's first sample; the power-on OCV then predates that charge.
const BOOT_OCV_FRESH_SECONDS: f64 = 120.0;
// A rested OCV is never below the loaded battery voltage (ADC tolerance aside);
// boot load sags 60-120 mV below it (ledger F30), never 300 mV (oam_run clamp).
const OCV_BELOW_LOAD_UV: i64 = 50000;
const OCV_ABOVE_LOAD_UV: i64 = 300000;
// Stock oam_run integrates over getrawmonotonic, i.e. awake time only. Longer
// gaps are suspend: the voltage after them is rested and taken directly.
const OAM_MAX_STEP_SECONDS: f64 = 60.0;
// The charger tops off at 4.175 V (28: 70 mA finish stage), so the curve's top
// point is never seen at rest. The kernel reports Full only after a hold that
// rests at or above 4.07 V; 4.0 V is the plausibility floor for that claim.
const FULL_FLOOR_UV: i64 = 4000000;
/// `deque(maxlen=3601)`.
const SAMPLES: usize = 3601;

/// Python `round(x)` to an int (ties to even).
pub(super) fn round_int(x: f64) -> i64 {
    x.round_ties_even() as i64
}

/// Python `round(x, 2)`: the decimal rounding of the exact binary value (ties to even), read back.
pub fn round2(x: f64) -> f64 {
    format!("{x:.2}").parse().expect("float")
}

/// Python `min(a, b)` / `max(a, b)` on floats: the first argument unless the second is smaller/larger.
pub(super) fn py_min(a: f64, b: f64) -> f64 {
    if b < a {
        b
    } else {
        a
    }
}

pub(super) fn py_max(a: f64, b: f64) -> f64 {
    if b > a {
        b
    } else {
        a
    }
}

/// `Soc`: bounded filter and monotonic trend, resetting on loss/replacement of pack.
///
/// The voltage curve is an estimate under load. Rest means stable voltage, not proof of open-circuit
/// voltage. Current compensation is used only when a real battery current and separately calibrated
/// resistance are supplied.
#[derive(Clone, Debug)]
pub struct Soc {
    pub(super) profile: Option<Profile>,
    pub(super) filtered: Option<f64>,
    pub percent: Option<f64>,
    pub(super) last: Option<f64>,
    pub(super) samples: VecDeque<(f64, i64)>,
    // Displayed value (stock UI_SOC), separate from the curve estimate.
    // `anchor` is the curve estimate at the last Full: that is 100 % of
    // what this charger fills (stock reset_parameter_dod_full, F28).
    pub(super) ui: Option<i64>,
    pub(super) anchor: Option<f64>,
    pub(super) full_latched: bool,
    pub(super) full_step_at: Option<f64>,
    pub(super) saved: Option<Saved>,
    // How the first sample started, e.g. 'warm/saved' (F54 diagnostics).
    pub(super) start: Option<String>,
}

impl Soc {
    pub fn new(profile: Option<Profile>, saved: Option<Value>) -> Soc {
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
            saved: Saved::of(saved),
            start: None,
        }
    }

    pub fn observe(&mut self, now: f64, r: &Reading) -> BatteryStatus {
        let charging = r.status.as_deref() == Some("Charging");
        let mut result = BatteryStatus {
            status: r.status.clone(),
            charging,
            voltage_uv: r.voltage,
            current_ua: r.current,
            temperature_millicelsius: r.temperature,
            charge_counter_uah: r.counter,
            ..BatteryStatus::default()
        };
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
            result.percent = Some(hardware);
            result.source = "fuel_gauge";
            result.confidence = "hardware";
            result.provisional = false;
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
        result.percent = Some(self.ui());
        result.source = if hybrid {
            "hybrid"
        } else if oam {
            "stock_oam"
        } else {
            "voltage_estimate"
        };
        result.confidence = if p.provisional {
            "provisional"
        } else {
            "calibrated_estimate"
        };
        result.provisional = p.provisional;
        result.resting = resting;
        result.estimate = Some(Estimate {
            design_capacity_uah: p.design_capacity_uah,
            remaining_uah_estimate: p
                .design_capacity_uah
                .map(|capacity| round_int(capacity as f64 * percent / 100.)),
            estimate_percent: round_int(self.display(percent)),
            anchor_percent: self.anchor.map(round2),
            full_latched: self.full_latched,
            filtered_voltage_uv: round_int(filtered),
            start: self.start.clone(),
            calibration_source: p.source.clone(),
        });
        result
    }
}
