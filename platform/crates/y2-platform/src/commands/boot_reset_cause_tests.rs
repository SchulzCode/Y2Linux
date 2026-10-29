//! tests/test_cpu_fix03.py ResumeCompletionDiagnostics: the boot journal records the reset cause the
//! watchdog driver decoded from the RGU status at probe.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;

fn map(reset: [(&'static str, Value); 3]) -> Map<String, Value> {
    reset.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

#[test]
fn boot_journal_records_the_decoded_cause() {
    assert_eq!(
        Value::Object(map(reset_cause(Some(
            "valid=1 raw=0x80000000 cause=watchdog_timeout hw_watchdog=1"
        )))),
        json!({"reset_cause": "watchdog_timeout", "reset_cause_reason": "rgu_wdt_status_at_probe",
               "reset_status_raw": "0x80000000"})
    );
    let zero = map(reset_cause(Some("valid=1 raw=0x0 cause=none_reported")));
    assert_eq!(zero["reset_cause"], Value::Null);
    assert_eq!(
        zero["reset_cause_reason"],
        json!("rgu_status_none_reported")
    );
    assert_eq!(
        map(reset_cause(None))["reset_cause_reason"],
        json!("not_observed")
    );
    let start = BootStage::parse("platform_start").unwrap();
    let record = transition(
        None,
        "b1",
        start,
        json!({}),
        1,
        reset_cause(Some("valid=1 raw=0x40000000 cause=software_reset")),
    );
    assert_eq!(record["reset_cause"], json!("software_reset"));
    // Without a reading (Python's default `reset=None`: `reset_cause(None)`).
    let record = transition(None, "b1", start, json!({}), 1, reset_cause(None));
    assert_eq!(record["reset_cause"], Value::Null);
}
