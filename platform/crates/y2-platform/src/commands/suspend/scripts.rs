//! The small programmes of the group: `suspend-record.py` (`/usr/sbin/y2-suspend-record`, used by
//! y2-suspend) and `y2-power-command` (installed as `/sbin/poweroff` and `/sbin/reboot`).
// SPDX-License-Identifier: GPL-2.0-only
use super::{exit, platform, receipt, traceback, E};
use crate::{argparse::python, reply::Outcome};
use std::io::Write;
use y2p_core::Root;

/// `suspend_record.main`: `record(Context(), argv[1], int(argv[2]) if given, argv[3] if given)`;
/// exit 1 when the receipt has an `error`, a traceback (exit 1) for any exception: a missing stage
/// (IndexError) and a result `int()` refuses (ValueError) before anything changed.
pub fn suspend_record(root: &Root, args: &[String]) -> Outcome {
    let Some(stage) = args.first() else {
        traceback(&E::Crash("list index out of range".into()), 1)
    };
    let result = match args.get(1) {
        None => None,
        Some(text) => match python::int(text) {
            // DEVIATION: an int beyond 64 bits (Python records it) is refused like a non-number;
            // y2-suspend passes exit codes.
            Some(value) => Some(value.parse::<i64>().unwrap_or_else(|_| {
                traceback(
                    &E::value(format!(
                        "invalid literal for int() with base 10: {}",
                        python::repr(text)
                    )),
                    1,
                )
            })),
            None => traceback(
                &E::value(format!(
                    "invalid literal for int() with base 10: {}",
                    python::repr(text)
                )),
                1,
            ),
        },
    };
    let mut record = receipt::Stage::new(stage, result);
    record.reason = args.get(2).cloned();
    match receipt::record(root, record) {
        Ok(value) => exit(i32::from(value.has_error())),
        Err(error) => traceback(&error, 1),
    }
}

/// `y2-power-command`: normal userspace requests go through the bounded platform coordinator.
pub fn power_command(root: &Root, args: &[String]) -> Outcome {
    let action = match args.first().map(String::as_str) {
        Some(name @ ("poweroff" | "reboot")) => name.to_owned(),
        _ => exit(2),
    };
    if args.len() > 1 {
        let _ = writeln!(
            std::io::stderr(),
            "Use y2-platform shutdown for normal shutdown; emergency flags are not supported."
        );
        exit(2)
    }
    platform(root, &["shutdown", &action, "--reason", "user"])
}
