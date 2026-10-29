//! The dispatcher parses, prints help and refuses arguments itself as main.py's argparse parser did.
// SPDX-License-Identifier: GPL-2.0-only
use std::{
    ffi::OsStr,
    os::unix::ffi::OsStrExt,
    process::{Command, Stdio},
};

const BINARY: &str = env!("CARGO_BIN_EXE_y2-platform");

#[test]
fn help_and_argument_errors_are_printed_without_python() {
    let run = |args: &[&str]| {
        Command::new(BINARY)
            .args(args)
            .env("Y2_ROOT", "/nonexistent")
            .env("Y2_PLATFORM_PYTHON", "/nonexistent/python3")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let help = run(&["capabilities", "--help"]);
    assert_eq!(help.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(help.stdout).unwrap(),
        "usage: main.py capabilities [-h] [--json]\n\noptions:\n  -h, --help  show this help message and exit\n  --json\n"
    );
    assert!(help.stderr.is_empty());
    let error = run(&["sleep", "nap"]);
    assert_eq!(error.status.code(), Some(2));
    assert!(error.stdout.is_empty());
    assert_eq!(
        String::from_utf8(error.stderr).unwrap(),
        "usage: main.py sleep [-h] {request,status,prepare}\nmain.py sleep: error: argument action: invalid choice: 'nap' (choose from 'request', 'status', 'prepare')\n"
    );
}

#[test]
fn arguments_that_are_not_utf8_are_parsed_as_python_sees_them() {
    // os.fsdecode: each byte that is no UTF-8 is a lone surrogate, shown as \udcXX.
    let run = |args: &[&[u8]]| {
        Command::new(BINARY)
            .args(args.iter().map(|a| OsStr::from_bytes(a)))
            .env("Y2_ROOT", "/nonexistent")
            .env("Y2_PLATFORM_PYTHON", "/nonexistent/python3")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let error = run(&[b"sleep", b"n\xffp"]);
    assert_eq!(error.status.code(), Some(2));
    assert!(error.stdout.is_empty());
    assert_eq!(
        String::from_utf8(error.stderr).unwrap(),
        "usage: main.py sleep [-h] {request,status,prepare}\nmain.py sleep: error: argument action: invalid choice: \
         'n\\udcffp' (choose from 'request', 'status', 'prepare')\n"
    );
    let int = run(&[b"time-set", b"\xe2\x82"]);
    assert_eq!(int.status.code(), Some(2));
    assert!(String::from_utf8(int.stderr)
        .unwrap()
        .ends_with("error: argument epoch: invalid int value: '\\udce2\\udc82'\n"));
    // A routed command runs in Rust: boot.mark refuses the stage before it reads anything.
    let stage = run(&[b"boot-stage", b"a\xffb"]);
    assert_eq!(stage.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(stage.stdout).unwrap(),
        "{\"schema\": \"org.y2linux.error/v1\", \"state\": \"Failed\", \"reason\": \"unknown_boot_stage\", \
         \"type\": \"ValueError\"}\n"
    );
    // The parse holds Python's surrogates (ASCII JSON: \udcff).
    let echo = Command::new(BINARY)
        .arg("boot-stage")
        .arg(OsStr::from_bytes(b"\xff"))
        .env("Y2_ROOT", "/nonexistent")
        .env("Y2_ARGPARSE_ECHO", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(String::from_utf8(echo.stdout)
        .unwrap()
        .contains("\"canonical\": [\"boot-stage\", \"\\udcff\"]"));
}

#[test]
fn the_parse_is_echoed_only_below_a_fixture_root() {
    let echo = |root: bool| {
        let mut command = Command::new(BINARY);
        command
            .args(["status", "--reb", "--pid=3"])
            .env("Y2_ARGPARSE_ECHO", "1")
            .env("Y2_PLATFORM_PYTHON", "/nonexistent/python3")
            .stdin(Stdio::null());
        if root {
            command.env("Y2_ROOT", "/nonexistent");
        } else {
            command.env_remove("Y2_ROOT");
        }
        command.output().unwrap()
    };
    let output = echo(true);
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["canonical"],
        serde_json::json!(["status", "--pid", "3", "--reborn"])
    );
    assert_eq!(value["namespace"]["pid"], serde_json::json!([{"int": "3"}]));
    assert_eq!(
        value["namespace"]["interval"],
        serde_json::json!({"int": "0"})
    );
    assert!(!String::from_utf8_lossy(&echo(false).stdout).contains("canonical"));
}
