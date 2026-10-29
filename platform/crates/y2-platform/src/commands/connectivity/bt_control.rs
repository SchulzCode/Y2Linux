//! `bt_control.py`: an explicit user radio action, serialized with the sole automatic reconnect owner
//! (`y2-bt-reconnect` holds `bt-operation.lock` while it reconnects and reads `bt-control.ini`,
//! tools/connectivity/reconnect.c: version 1, the same boot, a sequence in CLOCK_MONOTONIC µs not
//! after its own clock, a deadline at most 30 s later).
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::{private_directory, Failure, Outcome, Reply};
use serde_json::Value;
use std::{
    fs,
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
};
use y2p_core::{
    clock, command,
    json::{Build, Object},
    Root,
};

const OPERATIONS: [&str; 8] = [
    "connect",
    "disconnect",
    "pair",
    "forget",
    "power_on",
    "power_off",
    "uncertain",
    "power_pending",
];

/// `intent(ctx, operation)`: replace `/run/y2/bt-intent/bt-control.ini`.
pub fn intent(root: &Root, operation: &str) -> Result<(), Failure> {
    if !OPERATIONS.contains(&operation) {
        return Err(Failure::value("invalid_bluetooth_intent"));
    }
    let boot = root
        .read("/proc/sys/kernel/random/boot_id")
        .filter(|b| b.chars().count() == 36)
        .ok_or_else(|| Failure::value("boot_identity_unavailable"))?;
    let now = clock::monotonic_ns() / 1000;
    // The reconnect observer must not wake for unrelated power/USB status files.
    let directory = root.path("/run/y2/bt-intent");
    private_directory(&directory)?;
    // tempfile.mkstemp's name is random; this one is unique without the random pool (an early-boot
    // caller must not wait for the CRNG, CONTRACT.md "Randomness"). Same prefix, mode and O_EXCL.
    let temporary = directory.join(format!(
        ".bt-control.{}{}",
        std::process::id(),
        clock::monotonic_ns()
    ));
    let text = format!(
        "[intent]\nversion=1\nboot_id={boot}\nsequence={now}\noperation={operation}\ndeadline_us={}\n",
        now + 30_000_000
    );
    let result = (|| -> io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&temporary)?;
        file.write_all(text.as_bytes())?;
        drop(file);
        fs::rename(&temporary, directory.join("bt-control.ini"))
    })();
    let _ = fs::remove_file(&temporary);
    result.map_err(|e| Failure::os(e, &temporary))
}

/// `power(ctx, value)`: the command's result, or the busy refusal.
pub fn power(root: &Root, value: &str) -> Result<Object, Failure> {
    if value != "on" && value != "off" {
        return Err(Failure::value("invalid_bluetooth_power"));
    }
    // os.open without private_directory: a missing /run/y2 is Python's FileNotFoundError.
    let path = root.path("/run/y2/bt-operation.lock");
    let Some(_lock) = y2p_core::lock::try_exclusive(&path).map_err(|e| Failure::os(e, &path))?
    else {
        return Ok(Object::new()
            .with("ok", false)
            .with("reason", "platform_bluetooth_operation_in_progress"));
    };
    intent(
        root,
        if value == "on" {
            "power_pending"
        } else {
            "power_off"
        },
    )?;
    let result = command::run(root, &["bluetoothctl", "power", value], 15.0);
    intent(
        root,
        &if result.ok {
            format!("power_{value}")
        } else {
            "uncertain".into()
        },
    )?;
    Ok(outcome(&result))
}

/// A `ctx.command` result as Python's dict (`ok`, `reason`, `output`).
pub fn outcome(result: &command::Outcome) -> Object {
    Object::new()
        .with("ok", result.ok)
        .with(
            "reason",
            result.reason.clone().map_or(Value::Null, Value::from),
        )
        .with(
            "output",
            result.output.clone().map_or(Value::Null, Value::from),
        )
}

/// `y2-platform bluetooth-power on|off`: `print(json.dumps(result))`, exit 0 when ok.
pub fn command(root: &Root, value: &str) -> Outcome {
    let result = power(root, value)?;
    let ok = y2p_core::py::is_true(result.get("ok"));
    Reply::unsorted(&result, if ok { 0 } else { 1 })
}

#[cfg(test)]
#[path = "bt_control_tests.rs"]
mod bt_control_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_busy_operation_is_refused() {
        let dir = std::env::temp_dir().join(format!("y2-bt-{}", std::process::id()));
        fs::create_dir_all(dir.join("run/y2")).unwrap();
        let root = Root::new(&dir);
        let held = y2p_core::lock::exclusive(&dir.join("run/y2/bt-operation.lock")).unwrap();
        let reply = command(&root, "on").unwrap();
        assert_eq!(
            reply.stdout,
            "{\"ok\": false, \"reason\": \"platform_bluetooth_operation_in_progress\"}"
        );
        assert_eq!(reply.code, 1);
        drop(held);
        assert!(!dir.join("run/y2/bt-intent").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
