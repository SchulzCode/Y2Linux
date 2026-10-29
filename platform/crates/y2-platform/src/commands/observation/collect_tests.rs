//! Growth series of `collect` (was tests/test_platform_measurement.py; the run with a reused PID is in
//! tests/observation.rs).
// SPDX-License-Identifier: GPL-2.0-only
use super::*;

#[test]
fn growth_window() {
    let mut growth = Growth::default();
    for t in 0..10 {
        growth.add(f64::from(t), Some(100 + i64::from(t) * 2));
    }
    let result = growth.result();
    assert_eq!(result["slope_kib_per_hour"].as_f64(), Some(7200.0));
    assert_eq!(result["samples"], 10);
    assert_eq!(
        (
            &result["first_kib"],
            &result["last_kib"],
            &result["maximum_kib"]
        ),
        (&json!(100), &json!(118), &json!(118))
    );
}
