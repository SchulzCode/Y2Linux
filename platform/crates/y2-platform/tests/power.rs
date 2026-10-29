//! `power-daemon` runs on a fixture root (group POWER of the Python host tests).
// SPDX-License-Identifier: GPL-2.0-only
mod common;

use common::{repository, Fixture};
use serde_json::json;

const ETC: &str = "buildroot/board/y2/production-overlay/etc/y2linux";
const BAT0: &str = "/sys/class/power_supply/BAT0/";
const UI_STATE: &str = "/data/system/platform/battery-ui.json";

/// BAT0 as the charger driver publishes it, with the stock profile and policy installed.
fn battery(fixture: &Fixture, voltage: i64, status: &str) {
    for name in ["battery-profile.json", "power-policy.json"] {
        let text = std::fs::read_to_string(repository().join(ETC).join(name)).unwrap();
        fixture.put(&format!("/etc/y2linux/{name}"), &text);
    }
    fixture
        .put(&format!("{BAT0}present"), "1\n")
        .put(&format!("{BAT0}status"), &format!("{status}\n"))
        .put(&format!("{BAT0}voltage_now"), &format!("{voltage}\n"));
}

/// test_battery_soc.py::test_service_persists_the_display_value: the daemon starts its SOC from the
/// saved display value (`Soc(load_profile(ctx), ctx.json(UI_STATE, None))`) and writes the value it
/// shows back to the same file.
#[test]
fn service_persists_the_display_value() {
    let fixture = Fixture::new();
    battery(&fixture, 4021655, "Discharging");
    fixture
        .put(&format!("{BAT0}voltage_ocv"), "3545000\n")
        .put(
            "/sys/firmware/y2_boot/metadata",
            "valid=1 boot_mode=0 boot_reason=4 offline=0 recovery_image=0\n",
        )
        .put_json(
            UI_STATE,
            &json!({"schema": 1, "ui_percent": 95, "anchor_percent": null, "stale": true}),
        );
    let run = fixture.run_as(None, &["power-daemon"], &[("Y2_DAEMON_TICKS", "1")]);
    assert_eq!(run.code, Some(0), "{}", run.stderr);
    let published = fixture.json("/run/y2/power.json");
    assert_eq!(published["battery"]["percent"], 95);
    assert_eq!(published["battery"]["start"], "warm/saved");
    assert_eq!(
        fixture.json(UI_STATE),
        json!({"schema": 1, "ui_percent": 95, "anchor_percent": null})
    );
}
