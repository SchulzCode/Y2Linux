//! The maintenance-wake backoff (power plan 2026-11-24, after Android's Doze): the longer the player
//! lies untouched in one sleep, the further apart its battery-safety wakes. Every wake costs ~2 s
//! fully awake plus a journal write and a `sync`; the 23-h night of 2026-11-23 (boot 70c6fea1) woke
//! 47 times. Each wake also ends as soon as the safety owner has sampled, not after a fixed 2 s. A low or unknown battery keeps the 30-minute check, so a low-battery shutdown is never
//! later than before.
// SPDX-License-Identifier: GPL-2.0-only
use super::{Pm, E};
use y2p_contract::power::{BatteryState, PowerRecord};
use y2p_core::Root;

/// The kernel's period (`spm.maintenance_seconds`, 300..14400 s); an older kernel has no such
/// parameter and keeps its fixed 30 minutes.
pub const PARAMETER: &str = "/sys/module/spm/parameters/maintenance_seconds";
/// The period of the first wakes, and whenever the battery asks for care.
pub const BASE_SECONDS: u32 = 1800;
/// At or below this displayed percentage every wake is a 30-minute one again.
const CARE_PERCENT: i64 = 20;
/// (maintenance wakes so far in this sleep, period from then on): 30 min for the first 2 h, 1 h up
/// to 4 h, 2 h up to 8 h, then 4 h (the kernel's ceiling).
const STEPS: [(u32, u32); 4] = [(0, 1800), (4, 3600), (6, 7200), (8, 14400)];

/// The period for the next kernel entry after `wakes` maintenance wakes, given the battery safety
/// owner's last record (`None`: unreadable).
pub fn period(wakes: u32, power: Option<&PowerRecord>) -> u32 {
    let healthy = power.is_some_and(|p| {
        p.state == BatteryState::Normal && p.battery.percent.is_some_and(|n| n > CARE_PERCENT)
    });
    if !healthy {
        return BASE_SECONDS;
    }
    STEPS
        .iter()
        .rev()
        .find(|(after, _)| wakes >= *after)
        .map_or(BASE_SECONDS, |(_, seconds)| *seconds)
}

/// power.json as the battery safety owner last wrote it (`None` when missing or not a record).
pub fn power_record(root: &Root) -> Option<PowerRecord> {
    let value = root.json_object("/run/y2/power.json");
    if value.is_empty() {
        return None;
    }
    serde_json::from_value(serde_json::Value::Object(value)).ok()
}

/// Program the period for the next entry. Not a reason to refuse sleep: a kernel without the
/// parameter keeps its own 30 minutes, and the kernel only rejects a value outside its bounds, which
/// `STEPS` never produces (a failed write would leave the period the last entry used).
pub fn arm(root: &Root, pm: &mut dyn Pm, wakes: u32) -> Result<u32, E> {
    let seconds = period(wakes, power_record(root).as_ref());
    if root.path(PARAMETER).exists() {
        // A rejected value is not fatal (see above); an interrupted helper still ends here.
        if let Err(error) = pm.control(root, PARAMETER, &format!("{seconds}\n")) {
            if !error.is_os() {
                return Err(error);
            }
        }
    }
    Ok(seconds)
}

/// Steps of the wait for the battery safety owner after a maintenance return (at most 2 s, the
/// fixed pause this wait replaced).
const TICK_STEPS: u32 = 20;
const TICK_STEP_SECONDS: f64 = 0.1;

/// Give the battery safety owner its tick after a maintenance return: done as soon as power.json
/// holds a sample taken after the return (it samples every second), at the latest after 2 s.
pub fn await_safety_tick(root: &Root, pm: &mut dyn Pm) -> Result<(), E> {
    let returned = root.clock().monotonic_ns();
    for _ in 0..TICK_STEPS {
        pm.pause(TICK_STEP_SECONDS)?;
        if power_record(root).is_some_and(|p| p.monotonic_ns > returned) {
            break;
        }
    }
    Ok(())
}
