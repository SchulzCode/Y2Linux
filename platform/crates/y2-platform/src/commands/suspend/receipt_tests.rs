//! Product sleep state is derived from same-boot kernel and application proof
//! (tests/test_sleep_product.py), and the durable pre-entry receipt (tests/test_cpu_fix01.py).
// SPDX-License-Identifier: GPL-2.0-only
use super::{record, Stage};
use crate::commands::{
    observation::snapshot::timer_runtime,
    sleep::{product, status},
    suspend::{
        request,
        test_support::{answer, Reborn},
    },
};
use crate::testing::Fixture;
use serde_json::{json, Map, Value};
use std::fs::OpenOptions;

const KERNEL: &str = "valid=1 stage=EXIT failed_stage=NONE error=0 wake=0x20 \
                      reset_resume_entry=0x59325253 pcm_length=596";

/// `SleepProduct.setUp`.
fn player() -> Fixture {
    let fixture = Fixture::new();
    fixture.put("/proc/sys/kernel/random/boot_id", "boot-one");
    fixture.put("/sys/firmware/y2_pm/state", KERNEL);
    fixture.put("/sys/firmware/y2_pm/stage", "");
    fixture.put("/sys/power/state", "");
    fixture.put(
        "/sys/power/pm_test",
        "[none] core processors platform devices freezer",
    );
    fixture
}

/// Reborn acknowledging every restore (`Reply(ok)` in the Python test).
fn reborn(fixture: &Fixture, ok: bool) -> Reborn {
    Reborn::serve(
        fixture,
        Box::new(move |_, _| {
            Some(json!({"version": 1, "id": 6582, "ok": ok,
                        "result": {"sleep_ready": true, "screen_off": false}}))
        }),
    )
}

fn stage(fixture: &Fixture, stage: &str, result: i64) -> Map<String, Value> {
    record(&fixture.root, Stage::new(stage, Some(result)))
        .unwrap()
        .0
}

fn sleep_request(fixture: &Fixture) -> Value {
    serde_json::from_str(&request::request(&fixture.root).unwrap().stdout).unwrap()
}

fn sleep_status(fixture: &Fixture) -> Value {
    serde_json::from_str(&status(&fixture.root).unwrap().stdout).unwrap()
}

fn object(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

#[test]
fn product_request_is_typed_durable_refusal_without_kernel_or_radio_work() {
    let fixture = player();
    let result = sleep_request(&fixture);
    assert_eq!(result["schema"], "org.y2linux.sleep/v1");
    assert_eq!(result["state"], "refused");
    assert_eq!(result["reason"], "physical_qualification_required");
    assert_eq!(sleep_status(&fixture), result);
    assert_eq!(fixture.root.read("/sys/power/state").as_deref(), Some(""));
    assert_eq!(
        fixture.root.read("/sys/firmware/y2_pm/stage").as_deref(),
        Some("")
    );
}

#[test]
fn charging_refusal_is_specific_and_never_bypasses_the_kernel_guard() {
    let fixture = player();
    fixture.put("/sys/class/power_supply/BAT0/status", "Charging");
    let value = sleep_request(&fixture);
    assert_eq!(value["state"], "refused");
    assert_eq!(value["reason"], "charger_active");
    assert_eq!(fixture.root.read("/sys/power/state").as_deref(), Some(""));
}

#[test]
fn parallel_product_request_does_not_replace_owner_receipt() {
    let fixture = player();
    stage(&fixture, "quiescing_radios", 0);
    stage(&fixture, "kernel_suspend", 0);
    let lock = fixture.path("/run/y2/suspend.lock");
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    let stream = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&lock)
        .unwrap();
    stream.try_lock().unwrap();
    assert_eq!(
        sleep_request(&fixture)["reason"],
        "sleep_operation_in_progress"
    );
    drop(stream);
    assert_eq!(sleep_status(&fixture)["state"], "sleeping");
}

#[test]
fn completed_kernel_and_reborn_on_same_boot_restore() {
    let fixture = player();
    stage(&fixture, "quiescing_radios", 0);
    stage(&fixture, "kernel_suspend", 0);
    let restoring = stage(&fixture, "restoring_radios", 0);
    assert_eq!(restoring["product"]["kernel_completed"], true);
    fixture.put(
        "/sys/firmware/y2_pm/state",
        "valid=1 stage=RADIOS_RESTORING error=0",
    );
    let reborn = reborn(&fixture, true);
    let result = stage(&fixture, "complete_result_0", 0);
    drop(reborn);
    assert_eq!(result["product"]["state"], "restored");
    assert_eq!(result["product"]["same_boot"], true);
    assert_eq!(
        fixture.root.read("/sys/firmware/y2_pm/stage").as_deref(),
        Some("REBORN_READY")
    );
    // SPM EINT bit 5 alone does not distinguish the Power and RTC PMIC IRQs.
    assert_eq!(result["product"]["wake_reason"], "unknown");
}

#[test]
fn wake_requires_serviced_same_boot_child_irq_delta() {
    let fixture = player();
    for source in ["rtc", "power", "multiple"] {
        stage(&fixture, "quiescing_radios", 0);
        fixture.put(
            "/proc/interrupts",
            "  66: 1 0 0 0 mt6323 20 Level mt6397-rtc\n  70: 2 0 0 0 mt6323 5 Level mtk-pmic-keys\n",
        );
        stage(&fixture, "kernel_suspend", 0);
        let rtc = if ["rtc", "multiple"].contains(&source) {
            2
        } else {
            1
        };
        let power = if ["power", "multiple"].contains(&source) {
            3
        } else {
            2
        };
        fixture.put(
            "/proc/interrupts",
            &format!(
                "  66: {rtc} 0 0 0 mt6323 20 Level mt6397-rtc\n  70: {power} 0 0 0 mt6323 5 Level mtk-pmic-keys\n"
            ),
        );
        let value = stage(&fixture, "restoring_radios", 0);
        assert_eq!(value["product"]["wake_reason"], source);
    }
}

#[test]
fn terminal_previous_boot_success_or_refusal_does_not_report_current_failure() {
    for state in ["restored", "refused"] {
        let value = object(
            json!({"product": {"state": state, "request_boot_id": "boot-old",
                                              "kernel_completed": state == "restored",
                                              "wake_reason": "rtc"}}),
        );
        let current = product(&value, Some("boot-new")).unwrap();
        assert_eq!(current["state"], "idle");
        assert_eq!(current["same_boot"], Value::Null);
        assert_eq!(current["kernel_completed"], false);
    }
    let failed =
        object(json!({"product": {"state": "restore_failed", "request_boot_id": "boot-old"}}));
    assert_eq!(
        product(&failed, Some("boot-new")).unwrap()["state"],
        "restore_failed"
    );
}

#[test]
fn reset_is_not_same_boot_resume() {
    let fixture = player();
    stage(&fixture, "quiescing_radios", 0);
    stage(&fixture, "kernel_suspend", 0);
    fixture.put("/proc/sys/kernel/random/boot_id", "boot-two");
    assert_eq!(sleep_status(&fixture)["state"], "restore_failed");
    assert_eq!(
        sleep_status(&fixture)["reason"],
        "boot_changed_before_restoration"
    );
    stage(&fixture, "restoring_radios", 0);
    let reborn = reborn(&fixture, true);
    let result = stage(&fixture, "complete_result_0", 0);
    drop(reborn);
    assert_eq!(result["product"]["state"], "restore_failed");
    assert_eq!(result["product"]["same_boot"], false);
    assert_eq!(result["result"], 1);
    assert_eq!(
        fixture.root.read("/sys/firmware/y2_pm/stage").as_deref(),
        Some("")
    );
}

#[test]
fn missing_kernel_exit_and_reborn_failure_are_not_success() {
    let fixture = player();
    for (kernel, ready, reason) in [
        (
            "valid=1 stage=DEVICES_RESUMED error=0",
            true,
            "kernel_exit_not_proven",
        ),
        ("valid=0 stage=EXIT error=0", true, "kernel_exit_not_proven"),
        (
            "valid=1 stage=EXIT error=0",
            false,
            "reborn_resume_probe_failed",
        ),
    ] {
        stage(&fixture, "quiescing_radios", 0);
        stage(&fixture, "kernel_suspend", 0);
        fixture.put("/sys/firmware/y2_pm/state", kernel);
        stage(&fixture, "restoring_radios", 0);
        let reborn = reborn(&fixture, ready);
        let result = stage(&fixture, "complete_result_0", 0);
        drop(reborn);
        assert_eq!(result["product"]["reason"], reason);
        assert_eq!(result["product"]["state"], "restore_failed");
        assert_eq!(result["result"], 1);
    }
}

#[test]
fn charger_device_prepare_refusal_survives_radio_restore() {
    let fixture = player();
    stage(&fixture, "quiescing_radios", 0);
    stage(&fixture, "kernel_suspend", 0);
    fixture.put(
        "/sys/firmware/y2_pm/state",
        "valid=1 stage=EXIT failed_stage=DPM_PREPARED error=-16",
    );
    stage(&fixture, "restoring_radios", 1);
    let result = stage(&fixture, "complete_result_1", 1);
    assert_eq!(result["product"]["state"], "refused");
    assert_eq!(result["product"]["reason"], "device_prepare_refused");
}

#[test]
fn older_or_malformed_receipt_is_idle_without_resume_claim() {
    let fixture = player();
    assert_eq!(
        product(&Map::new(), Some("boot-one")).unwrap()["state"],
        "idle"
    );
    assert_eq!(
        product(&Map::new(), None).unwrap()["same_boot"],
        Value::Null
    );
    fixture.put("/data/system/platform/suspend-last.json", "[]");
    assert_eq!(sleep_status(&fixture)["state"], "idle");
}

#[test]
fn a_product_that_is_no_object_is_a_refusal_record() {
    let fixture = player();
    for (product, failure) in [
        ("5", "'int' object is not iterable"),
        (
            "[[1, \"a\"], [\"state\", \"requested\"]]",
            "'<' not supported between instances of 'int' and 'str'",
        ),
    ] {
        fixture.put(
            "/data/system/platform/suspend-last.json",
            &format!("{{\"product\": {product}}}"),
        );
        let reply = status(&fixture.root).unwrap();
        assert_eq!(reply.code, 1);
        assert_eq!(
            reply.stdout,
            format!("{{\"failure\":\"{failure}\",\"state\":\"Failed\"}}")
        );
    }
}

#[test]
fn pm_test_return_never_proves_deep_sleep() {
    let fixture = player();
    for mode in ["freezer", "devices", "platform", "processors", "core"] {
        fixture.put("/sys/power/pm_test", &format!("[{mode}] none"));
        stage(&fixture, "quiescing_radios", 0);
        stage(&fixture, "kernel_suspend", 0);
        stage(&fixture, "restoring_radios", 0);
        let reborn = reborn(&fixture, true);
        let value = stage(&fixture, "complete_result_0", 0);
        drop(reborn);
        assert_eq!(value["result"], 0);
        assert_eq!(value["product"]["state"], "idle");
        assert_eq!(value["product"]["kernel_completed"], false);
        assert_ne!(
            fixture.root.read("/sys/firmware/y2_pm/stage").as_deref(),
            Some("REBORN_READY")
        );
    }
}

#[test]
fn exit_without_reset_pcm_or_wake_is_not_deep_resume() {
    let fixture = player();
    for kernel in [
        "valid=1 stage=EXIT error=0",
        "valid=1 stage=EXIT error=0 reset_resume_entry=0x59325253 pcm_length=479 wake=0x20",
        "valid=1 stage=EXIT error=0 reset_resume_entry=0x59325253 pcm_length=596 wake=0x0",
    ] {
        stage(&fixture, "quiescing_radios", 0);
        stage(&fixture, "kernel_suspend", 0);
        fixture.put("/sys/firmware/y2_pm/state", kernel);
        let value = stage(&fixture, "restoring_radios", 0);
        assert_eq!(value["product"]["kernel_completed"], false);
    }
}

#[test]
fn failed_suspend_still_restores_the_application() {
    let fixture = player();
    stage(&fixture, "quiescing_radios", 0);
    stage(&fixture, "kernel_suspend", 0);
    stage(&fixture, "restoring_radios", 1);
    let rpc = reborn(&fixture, true);
    let value = stage(&fixture, "complete_result_1", 1);
    assert_eq!(rpc.ops().len(), 1);
    assert_eq!(value["reborn_restore"], true);
    assert_eq!(value["result"], 1);
    assert_eq!(value["product"]["state"], "restore_failed");
}

#[test]
fn preparation_has_a_distinct_state_and_requires_acknowledgement() {
    let fixture = player();
    stage(&fixture, "requested", 0);
    assert_eq!(sleep_status(&fixture)["state"], "requested");
    let rpc = Reborn::serve(
        &fixture,
        Box::new(|_, _| answer(json!({"sleep_ready": true, "screen_off": true}))),
    );
    assert_eq!(
        request::prepare_owned(&fixture.root).unwrap()["state"],
        "preparing"
    );
    let value = stage(&fixture, "quiescing_radios", 0);
    assert_eq!(value["reborn_prepared"], true);
    rpc.handle(Box::new(|_, _| answer(json!({"sleep_ready": false}))));
    assert!(request::prepare_owned(&fixture.root)
        .unwrap_err()
        .is_value());
}

#[test]
fn state_directory_symlink_is_refused() {
    let fixture = player();
    fixture.mkdir("/data");
    std::os::unix::fs::symlink(fixture.base(), fixture.path("/data/system")).unwrap();
    let error = record(&fixture.root, Stage::new("quiescing_radios", Some(0))).unwrap_err();
    assert!(error.is_value(), "{error:?}");
    assert!(
        error.text().contains("state_directory_not_directory"),
        "{error:?}"
    );
}

// ---- tests/test_cpu_fix01.py --------------------------------------------------------------------

#[test]
fn durable_pre_entry_receipt_and_nohz_highres_mode() {
    let fixture = Fixture::new();
    fixture.put(
        "/sys/bus/platform/drivers/y2-usb/11200000.usb/status",
        "pm_suspends=1 pm_restores=1\n",
    );
    let value = stage(&fixture, "quiescing_radios", 0);
    assert!(value["usb_restore"]["controllers"]["11200000.usb"]
        .as_str()
        .unwrap()
        .contains("pm_restores=1"));
    stage(&fixture, "kernel_suspend", 0);
    assert_eq!(
        fixture.json("/data/system/platform/suspend-last.json")["stage"],
        "kernel_suspend"
    );
    assert_eq!(value["requested_mode"], "deep");
    fixture.put(
        "/proc/timer_list",
        "cpu: 0\n .hres_active : 1\n .nohz : 2\n",
    );
    let result = timer_runtime(&fixture.root).unwrap();
    assert_eq!(result["highres_active"], true);
    assert_eq!(result["no_hz_active"], true);
}
