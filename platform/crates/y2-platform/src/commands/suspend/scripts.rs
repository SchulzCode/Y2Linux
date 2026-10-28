//! The small programmes of the group: `suspend-record.py` (`/usr/sbin/y2-suspend-record`, used by
//! y2-suspend) and `y2-power-command` (installed as `/sbin/poweroff` and `/sbin/reboot`).
// SPDX-License-Identifier: GPL-2.0-only
use super::{exit, platform, python_script, receipt, traceback};
use crate::reply::Outcome;
use std::io::Write;
use y2p_core::{py::truthy, root::python_int, Root};

/// `suspend_record.main`: `record(Context(), argv[1], int(argv[2]) if given, argv[3] if given)`;
/// exit 1 when the receipt has an `error`, a traceback (exit 1) for any exception. Argument forms it
/// does not read (missing stage, a result that is no 64-bit integer) and stored receipts Python
/// would read differently run the Python programme.
pub fn suspend_record(root: &Root, args: &[String]) -> Outcome {
    let fallback = || python_script("suspend-record.py", "__suspend-record", args);
    let Some(stage) = args.first() else { fallback() };
    let result = match args.get(1) {
        None => None,
        Some(text) => match python_int(text) {
            Some(value) => Some(value),
            None => fallback(),
        },
    };
    let battery = root
        .json_object("/run/y2/power.json")
        .get("battery")
        .is_some_and(|b| !b.is_object());
    if battery || !receipt::plain(&root.json_object(receipt::RECEIPT), stage) {
        fallback()
    }
    let mut record = receipt::Stage::new(stage, result);
    record.reason = args.get(2).cloned();
    match receipt::record(root, record) {
        Ok(value) => exit(i32::from(value.get("error").is_some_and(truthy))),
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
