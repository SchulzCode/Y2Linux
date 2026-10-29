//! The userspace halves of the POWER-FINAL kernel switch tests (test_pwrap_gate, test_md1_gate,
//! test_display_domain, test_pmic_sleep, test_power_final_27), on the port: `power_features.apply` arms
//! each kernel parameter at `application_ready`, the owner's override keeps one off, and the sleep
//! diagnostics rows record the switches.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;
use crate::testing::Fixture;

/// A kernel parameter with its kernel default.
fn parameter(fixture: &Fixture, path: &str, value: &str) {
    fixture.put_mode(path, value, 0o644);
}

/// The owner's A/B override (/data/system/platform/power-features.json).
fn owner(fixture: &Fixture, value: Value) {
    fixture.put(OVERRIDE, &value.to_string());
}

/// `apply(ctx)` with a feature's entry as JSON.
fn apply_json(fixture: &Fixture) -> Value {
    serde_json::to_value(apply(&fixture.root).features).unwrap()
}

#[test]
fn pwrap_gate_userspace_arms_with_owner_override() {
    // test_pwrap_gate.py (POWER-FINAL-19).
    let fixture = Fixture::new();
    let param = "/sys/module/spm/parameters/pwrap_gate";
    parameter(&fixture, param, "N");
    assert_eq!(apply_json(&fixture)["pwrap_gate"]["wanted"], true);
    assert_eq!(fixture.read(param), "Y");
    owner(&fixture, json!({"pwrap_gate": false}));
    apply(&fixture.root);
    assert_eq!(fixture.read(param), "N");
}

#[test]
fn md1_gate_userspace_arms_with_owner_override() {
    // test_md1_gate.py (POWER-FINAL-17).
    let fixture = Fixture::new();
    let param = "/sys/module/spm/parameters/md1_gate";
    parameter(&fixture, param, "N");
    assert_eq!(apply_json(&fixture)["md1_gate"]["wanted"], true);
    assert_eq!(fixture.read(param), "Y");
    owner(&fixture, json!({"md1_gate": false}));
    apply(&fixture.root);
    assert_eq!(fixture.read(param), "N");
}

#[test]
fn userspace_arms_after_first_frame_with_owner_override() {
    // test_display_domain.py (POWER-FINAL-16 G1).
    let fixture = Fixture::new();
    let param = "/sys/module/spm/parameters/dis_pdn";
    let golden = "/sys/module/spm/parameters/golden_apply";
    parameter(&fixture, param, "N");
    parameter(&fixture, golden, "0");
    assert_eq!(apply_json(&fixture)["dis_pdn"]["wanted"], true);
    assert_eq!(fixture.read(param), "Y");
    owner(&fixture, json!({"dis_pdn": false}));
    apply(&fixture.root);
    assert_eq!(fixture.read(param), "N");
    assert_eq!(
        fixture.json("/run/y2/power-features.json")["features"]["dis_pdn"]["wanted"],
        false
    );
    // golden_apply: all groups by default, owner subset, invalid value refused.
    assert_eq!(fixture.read(golden), "15");
    owner(&fixture, json!({"golden_apply": 3}));
    apply(&fixture.root);
    assert_eq!(fixture.read(golden), "3");
    owner(&fixture, json!({"golden_apply": 99}));
    assert!(apply_json(&fixture)["golden_apply"].get("error").is_some());
    // Armed after the first frame: `boot-stage application_ready` applies them, no earlier stage does.
    for (stage, armed) in [("services_started", "N"), ("application_ready", "Y")] {
        let fixture = Fixture::new();
        fixture
            .put("/data/.y2data-schema", "1")
            .put("/run/y2-data-device", "/dev/data")
            .put(
                "/proc/self/mountinfo",
                "5 1 179:7 / /data rw - ext4 /dev/data rw",
            )
            .put("/proc/sys/kernel/random/boot_id", "one");
        parameter(&fixture, param, "N");
        crate::commands::boot::mark(&fixture.root, stage).expect("boot stage");
        assert_eq!(fixture.read(param), armed, "{stage}");
    }
}

#[test]
fn userspace_arms_all_groups_with_owner_override() {
    // test_pmic_sleep.py (POWER-FINAL-18).
    let fixture = Fixture::new();
    let param = "/sys/module/pwrap/parameters/pmic_sleep";
    parameter(&fixture, param, "0");
    assert_eq!(apply_json(&fixture)["pmic_sleep"]["wanted"], 0xf);
    assert_eq!(fixture.read(param), "15");
    owner(&fixture, json!({"pmic_sleep": 1}));
    apply(&fixture.root);
    assert_eq!(fixture.read(param), "1");
    owner(&fixture, json!({"pmic_sleep": 7})); // 26 behaviour: no VSYS
    apply(&fixture.root);
    assert_eq!(fixture.read(param), "7");
    owner(&fixture, json!({"pmic_sleep": 16}));
    assert!(apply_json(&fixture)["pmic_sleep"].get("error").is_some());
}

#[test]
fn userspace_arms_both_with_owner_override() {
    // test_power_final_27.py (ledger F20/F22).
    let fixture = Fixture::new();
    let panel = "/sys/module/panel/parameters/stock_suspend";
    let vgp2 = "/sys/module/y2_cs43131/parameters/vgp2_idle_off";
    for p in [panel, vgp2] {
        parameter(&fixture, p, "N");
    }
    let result = apply_json(&fixture);
    assert_eq!(
        (fixture.read(panel), fixture.read(vgp2)),
        ("Y".into(), "Y".into())
    );
    assert_eq!(result["panel_stock_suspend"]["wanted"], true);
    owner(
        &fixture,
        json!({"panel_stock_suspend": false, "vgp2_idle_off": false}),
    );
    apply(&fixture.root);
    assert_eq!(
        (fixture.read(panel), fixture.read(vgp2)),
        ("N".into(), "N".into())
    );
}

#[test]
fn deep_idle_audio_is_armed_by_userspace() {
    // POWER-FINAL-32: kernel default off, power_features turns it on.
    let fixture = Fixture::new();
    let param = "/sys/module/clocks/parameters/deep_idle_audio";
    parameter(&fixture, param, "N");
    assert_eq!(apply_json(&fixture)["deep_idle_audio"]["value"], "Y");
    owner(&fixture, json!({"deep_idle_audio": false}));
    apply(&fixture.root);
    assert_eq!(fixture.read(param), "N");
    // The sleep diagnostics rows record deep_audio_entries.
    let fixture = diagnostics_fixture();
    parameter(
        &fixture,
        "/sys/module/clocks/parameters/deep_audio_entries",
        "41",
    );
    for features in sleep_row_features(&fixture) {
        assert_eq!(features["deep_audio_entries"], "41");
    }
}

#[test]
fn panel_power_down_is_on_only_for_a_confirmed_panel() {
    // #45: armed at application_ready on an ST7701S (confirmed 6456) and a
    // GC9503V (confirmed on #45/#42, Beta 8.30); an unknown panel keeps the
    // lit panel unless the owner opts in.
    let param = "/sys/module/panel/parameters/power_down";
    for (metadata, expected) in [
        ("panel=st7701s", "Y"),
        ("panel=gc9503v", "Y"),
        ("panel=unknown", "N"),
        ("", "N"),
    ] {
        let fixture = Fixture::new();
        parameter(&fixture, param, "N");
        fixture.put(
            "/sys/firmware/y2_boot/metadata",
            &format!("valid=1 boot_mode=0 boot_reason=4 offline=0 recovery_image=0 {metadata}\n"),
        );
        apply(&fixture.root);
        assert_eq!(fixture.read(param), expected, "{metadata}");
        owner(&fixture, json!({"panel_power_down": expected == "N"}));
        apply(&fixture.root);
        let opposite = if expected == "N" { "Y" } else { "N" };
        assert_eq!(fixture.read(param), opposite, "{metadata}");
    }
}

#[test]
fn the_panel_is_remembered_for_boots_lk_cannot_identify() {
    // fm-lk 0x81e0af60: lcm=%1d is LK's "read 0x0A succeeded" flag; the name is
    // the first driver whose compare_id matched, else the last (st7701). The
    // kernel's panel= (gc9503v with either digit, st7701 with lcm=1-) is
    // recorded on Y2DATA and handed to the kernel (panel_hint) when it is unknown.
    // (The kernel half, gc9503v_identify's order, stays in test_power_final_27.py.)
    let fixture = Fixture::new();
    let params = "/sys/module/panel/parameters";
    let power_down = format!("{params}/power_down");
    let hint = format!("{params}/panel_hint");
    let boot = |panel: &str, token: &str| {
        fixture
            .put(
                "/sys/firmware/y2_boot/metadata",
                &format!(
                    "valid=1 boot_mode=0 boot_reason=4 offline=0 recovery_image=0 panel={panel}\n"
                ),
            )
            .put("/sys/firmware/y2_boot/lcm", &format!("{token}\n"))
            .put(&power_down, "N")
            .put(&hint, "0");
        apply(&fixture.root);
        (fixture.read(&power_down), fixture.read(&hint))
    };
    let pair = |a: &str, b: &str| (a.to_owned(), b.to_owned());
    let recorded = || fixture.json(PANEL_RECORD)["panel"].clone();
    // No record yet and LK could not read the panel: stays lit.
    assert_eq!(
        boot("unknown", "lcm=0-st7701_hvga_dsi_vdo_boe"),
        pair("N", "0")
    );
    // An identified boot records it; LK itself identifies, no hint needed.
    assert_eq!(
        boot("st7701s", "lcm=1-st7701_hvga_dsi_vdo_boe"),
        pair("Y", "0")
    );
    assert_eq!(recorded(), "st7701s");
    // A soft restart uses the record.
    assert_eq!(
        boot("unknown", "lcm=0-st7701_hvga_dsi_vdo_boe"),
        pair("Y", "2")
    );
    // The driver's own ID read identifies it too (and is recorded).
    fixture.put(PANEL_KIND, "2").remove(PANEL_RECORD);
    assert_eq!(
        boot("unknown", "lcm=0-st7701_hvga_dsi_vdo_boe"),
        pair("Y", "0")
    );
    assert_eq!(recorded(), "st7701s");
    fixture.put(PANEL_KIND, "0");
    // A GC9503V is armed too (8.30) and its record handed over. LK names
    // it with lcm=0- too (#45/#46: a cold boot on the reporter's player).
    assert_eq!(
        boot("gc9503v", "lcm=0-gc9503v_hvga_dsi_vdo_hsd"),
        pair("Y", "0")
    );
    assert_eq!(recorded(), "gc9503v");
    assert_eq!(
        boot("gc9503v", "lcm=1-gc9503v_hvga_dsi_vdo_hsd"),
        pair("Y", "0")
    );
    assert_eq!(
        boot("unknown", "lcm=0-st7701_hvga_dsi_vdo_boe"),
        pair("Y", "1")
    );
}

/// A verified data mount, where the sleep diagnostics rows are written.
fn diagnostics_fixture() -> Fixture {
    let fixture = Fixture::new();
    fixture
        .put("/data/.y2data-schema", "1")
        .put("/run/y2-data-device", "/dev/data")
        .put(
            "/proc/self/mountinfo",
            "5 1 179:7 / /data rw - ext4 /dev/data rw",
        )
        .put("/proc/sys/kernel/random/boot_id", "one");
    fixture
}

/// The `features` of one sleep diagnostics row from each writer of the sleep diagnostics: the suspend
/// helper's (`y2-suspend-record`) and the power daemon's (USB rows).
fn sleep_row_features(fixture: &Fixture) -> Vec<Value> {
    assert_eq!(
        crate::commands::suspend::diagnostics::record(&fixture.root, "kernel_suspend").ok(),
        Some(true)
    );
    let mut daemon = crate::commands::power::journal::Diagnostics::default();
    assert!(daemon.record(&fixture.root, "usb", 1., false, Some("udc_change")));
    let rows: Vec<Value> = fixture
        .read("/data/system/platform/sleep-diagnostics.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 2);
    rows.into_iter()
        .map(|row| row["features"].clone())
        .collect()
}

#[test]
fn sleep_rows_record_both() {
    // test_power_final_27.py: the panel power-down and VGP2 switches are in every sleep row.
    let fixture = diagnostics_fixture();
    for (path, value) in [
        ("/sys/module/panel/parameters/stock_suspend", "Y"),
        ("/sys/module/panel/parameters/power_down", "N"),
        ("/sys/module/y2_cs43131/parameters/vgp2_idle_off", "Y"),
    ] {
        parameter(&fixture, path, value);
    }
    for features in sleep_row_features(&fixture) {
        assert_eq!(features["panel_stock_suspend"], "Y");
        assert_eq!(features["panel_power_down"], "N");
        assert_eq!(features["vgp2_idle_off"], "Y");
    }
}

#[test]
fn pmic_sleep_state_is_recorded_in_sleep_rows() {
    // The sleep rows record the PMIC sleep state (test_pmic_sleep.py::test_wiring_order_and_switch).
    let fixture = diagnostics_fixture();
    parameter(
        &fixture,
        "/sys/module/pwrap/parameters/pmic_sleep_state",
        "groups=15 applied=15",
    );
    for features in sleep_row_features(&fixture) {
        assert_eq!(features["pmic_sleep_state"], "groups=15 applied=15");
    }
}
