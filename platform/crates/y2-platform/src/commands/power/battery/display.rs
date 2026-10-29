//! The displayed percentage (stock UI_SOC), separate from the curve estimate: restored at the first
//! sample, then stock mt_battery_update_status (follow down on battery, up to 99 on a charger, one
//! point per tick at Full, the Full anchor).
// SPDX-License-Identifier: GPL-2.0-only
use super::soc::{py_max, py_min, round2, round_int, Soc};

// Stock dod_init reuses the saved value only within 40 points of the boot estimate.
const UI_RESTORE_POINTS: f64 = 40.0;
// Stock mt_battery_update_status: at Full the display counts up one point per
// 10-s battery-thread tick to 100 (100Percent tracking).
const FULL_STEP_SECONDS: f64 = 10.0;

impl Soc {
    /// What survives a restart (stock RTC spare register), or `None`: (ui_percent, anchor_percent).
    pub fn state(&self) -> Option<(i64, Option<f64>)> {
        Some((self.ui?, self.anchor.map(round2)))
    }

    pub(super) fn display(&self, estimate: f64) -> f64 {
        match self.anchor {
            Some(anchor) if anchor != 0.0 => py_max(0., py_min(100., estimate * 100. / anchor)),
            _ => estimate,
        }
    }

    /// First sample: reuse the saved display value within 40 points, or after a warm restart unless
    /// the live estimate is 40+ points above it. The estimate and the display value it starts from.
    pub(super) fn restore(
        &mut self,
        estimate: f64,
        charging: bool,
        boot: Option<&str>,
    ) -> (f64, i64) {
        let saved = self.saved.take().unwrap_or_default();
        if let Some(anchor) = saved.anchor_percent.filter(|a| (80.0..=100.0).contains(a)) {
            self.anchor = Some(anchor);
        }
        let mut ui = saved.ui_percent;
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
        let start = match restored {
            // The estimate restarts there too (stock dod_init sets the gauge).
            Some(ui) => {
                let anchor = self.anchor.filter(|a| *a != 0.0).unwrap_or(100.);
                (py_min(100., ui as f64 * anchor / 100.), ui)
            }
            None => (estimate, round_int(shown)),
        };
        self.start = Some(format!(
            "{}/{}",
            boot.unwrap_or("unknown"),
            if restored.is_some() {
                "saved"
            } else {
                self.start.as_deref().unwrap_or("None")
            }
        ));
        start
    }

    /// Stock mt_battery_update_status, on the display value only: the estimate and the display
    /// value after this sample.
    pub(super) fn track(
        &mut self,
        now: f64,
        status: Option<&str>,
        full: bool,
        target: f64,
        (percent, ui): (f64, i64),
    ) -> (f64, i64) {
        let shown = self.display(percent);
        if status == Some("Discharging") {
            // Charger gone: the display only follows the estimate down.
            self.full_latched = false;
            self.full_step_at = None;
            return (percent, ui.min(round_int(shown)));
        }
        if full && !self.full_latched {
            // Count up one point per tick, then re-anchor at this Full.
            let mut ui = ui;
            if ui < 100 {
                if self
                    .full_step_at
                    .is_none_or(|at| now >= at + FULL_STEP_SECONDS)
                {
                    ui += 1;
                    self.full_step_at = Some(now);
                }
                if ui < 100 {
                    return (percent, ui);
                }
            }
            self.full_latched = true;
            self.full_step_at = None;
            // The kernel says Full only after a rest: anchor on that voltage,
            // not on the rate-limited estimate that may still be catching up.
            let anchor = py_max(80., py_min(100., target));
            self.anchor = Some(anchor);
            return (anchor, ui);
        }
        if self.full_latched {
            // Maintenance holds and recharges after Full stay at 100 until the
            // charger is removed (stock bat_in_recharging_state); the estimate
            // stays at the Full anchor as stock's gauge does after its reset.
            // Unplugged, it falls from there at the normal rate (stock Sync).
            // (The anchor is set whenever the Full is latched.)
            return (self.anchor.unwrap_or(percent), 100);
        }
        // Charging or holding, not yet Full: follow the estimate up, at most 99.
        (percent, 99.min(ui.max(round_int(shown))))
    }
}
