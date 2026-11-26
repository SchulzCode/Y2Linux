//! The dispatcher parses, prints help and refuses arguments itself (src/cli.rs), before any command runs.
// SPDX-License-Identifier: GPL-2.0-only
use std::{
    ffi::OsStr,
    os::unix::ffi::OsStrExt,
    process::{Command, Output, Stdio},
};

const BINARY: &str = env!("CARGO_BIN_EXE_y2-platform");

fn run(args: &[&[u8]]) -> Output {
    Command::new(BINARY)
        .args(args.iter().map(|a| OsStr::from_bytes(a)))
        .env("Y2_ROOT", "/nonexistent")
        .env_remove("Y2_ARGPARSE_ECHO")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn help_goes_to_stdout_and_argument_errors_to_stderr_with_exit_2() {
    let help = run(&[b"capabilities", b"--help"]);
    assert_eq!(help.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(help.stdout).unwrap(),
        "usage: y2-platform capabilities [-h] [--json]\n\noptions:\n  -h, --help  show this help message and exit\n  --json\n"
    );
    assert!(help.stderr.is_empty());
    let error = run(&[b"sleep", b"nap"]);
    assert_eq!(error.status.code(), Some(2));
    assert!(error.stdout.is_empty());
    assert_eq!(
        String::from_utf8(error.stderr).unwrap(),
        "usage: y2-platform sleep [-h] {request,status,prepare}\ny2-platform sleep: error: argument action: \
         invalid choice: \"nap\" (choose from request, status, prepare)\n"
    );
    // An abbreviation is no option any more.
    let abbreviated = run(&[b"capabilities", b"--js"]);
    assert_eq!(abbreviated.status.code(), Some(2));
    assert!(abbreviated.stdout.is_empty());
    assert!(String::from_utf8(abbreviated.stderr)
        .unwrap()
        .ends_with("y2-platform capabilities: error: unrecognized arguments: \"--js\"\n"));
}

#[test]
fn arguments_that_are_not_utf8_are_refused_before_a_command_runs() {
    for args in [
        &[&b"sleep"[..], b"n\xffp"][..],
        &[b"time-set", b"\xe2\x82"],
        // boot.mark would refuse the stage with exit 1: the parser does first, with 2.
        &[b"boot-stage", b"a\xffb"],
    ] {
        let output = run(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr
                .lines()
                .last()
                .unwrap()
                .contains(": error: argument is not UTF-8: "),
            "{stderr}"
        );
    }
}

#[test]
fn the_parse_is_echoed_only_below_a_fixture_root() {
    let echo = |root: bool| {
        let mut command = Command::new(BINARY);
        command
            .args(["status", "--reborn", "--pid=3", "--interval", "1"])
            .env("Y2_ARGPARSE_ECHO", "1")
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
        value,
        serde_json::json!({"canonical": ["status", "--pid", "3", "--reborn", "--interval", "1.0"]})
    );
    assert!(!String::from_utf8_lossy(&echo(false).stdout).contains("canonical"));
}

#[test]
fn the_argument_table_is_printed_for_the_contract_check() {
    let output = run(&[b"__arguments"]);
    assert_eq!(output.status.code(), Some(0));
    let table: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(table["time-set"][0]["name"], "epoch");
    assert_eq!(table["time-set"][0]["type"], "int");
    assert_eq!(table.as_object().unwrap().len(), 39);
}
