//! The small programmes of the group: `y2-suspend-record` (`/usr/sbin/y2-suspend-record`, used by
//! y2-suspend) and `y2-power-command` (installed as `/sbin/poweroff` and `/sbin/reboot`).
// SPDX-License-Identifier: GPL-2.0-only
use super::{exit, fail, platform, receipt, E};
use crate::{argparse::python, reply::Outcome};
use std::io::Write;
use y2p_core::Root;

/// `y2-suspend-record <stage> [<result> [<reason>]]`: one receipt record for y2-suspend (the owner's
/// kernel qualification path). Exit 0, or 1 when the receipt now holds an `error` or nothing could be
/// recorded (no stage, a result that is no integer: nothing changed; a receipt that could not be
/// written). y2-suspend reads only the exit code.
pub fn suspend_record(root: &Root, args: &[String]) -> Outcome {
    const NAME: &str = "y2-suspend-record";
    let Some(stage) = args.first() else {
        fail(NAME, &E::value("stage_required"))
    };
    let result = match args.get(1) {
        None => None,
        // A result beyond 64 bits is refused like a non-number; y2-suspend passes exit codes.
        Some(text) => match python::int(text).and_then(|value| value.parse::<i64>().ok()) {
            Some(value) => Some(value),
            None => fail(
                NAME,
                &E::value(format!(
                    "invalid literal for int() with base 10: {}",
                    python::repr(text)
                )),
            ),
        },
    };
    let mut record = receipt::Stage::new(stage, result);
    record.reason = args.get(2).cloned();
    match receipt::record(root, record) {
        Ok(value) => exit(i32::from(value.has_error())),
        Err(error) => fail(NAME, &error),
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
