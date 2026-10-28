//! The dispatcher hands every command it does not run itself to Python exactly as the shell wrapper did,
//! and parses, prints help and refuses arguments itself as main.py's argparse parser does.
// SPDX-License-Identifier: GPL-2.0-only
// Built without the Python platform, the tests of its hand-off are left out.
#![cfg_attr(not(feature = "python-fallback"), allow(unused_imports))]
use std::{
    ffi::OsStr,
    fs,
    io::Write,
    os::unix::{ffi::OsStrExt, fs::PermissionsExt},
    path::PathBuf,
    process::{Command, Stdio},
};

const BINARY: &str = env!("CARGO_BIN_EXE_y2-platform");

#[cfg(feature = "python-fallback")]
/// A stand-in interpreter: prints its PID, arguments, the two Python variables and stdin, exits 3.
fn fake_python(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("y2-dispatch-{}-{name}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("python3");
    fs::write(
        &path,
        "#!/bin/sh\necho \"pid=$$\"\nfor a in \"$@\"; do echo \"arg=$a\"; done\n\
         echo \"PYTHONPATH=${PYTHONPATH-unset}\"\necho \"PYTHONDONTWRITEBYTECODE=${PYTHONDONTWRITEBYTECODE-unset}\"\n\
         echo \"KEPT=${KEPT-unset}\"\necho \"stdin=$(cat)\"\nexit 3\n",
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(feature = "python-fallback")]
#[test]
fn python_replaces_the_dispatcher_with_the_same_arguments() {
    let python = fake_python("same");
    let mut child = Command::new(BINARY)
        // A firmware path that is not UTF-8: the service group's commands do not read such bytes yet, so
        // the arguments go to Python as given.
        .args(["firmware", "--", "install"])
        .arg(OsStr::from_bytes(b"\xff"))
        .env("Y2_ROOT", "/nonexistent")
        .env("Y2_PLATFORM_PYTHON", &python)
        .env("Y2_PLATFORM_MAIN", "/fixture/main.py")
        .env("KEPT", "inherited")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"input").unwrap();
    let pid = child.id();
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(3),
        "Python's exit code is the command's"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!(
            "pid={pid}\narg=-I\narg=/fixture/main.py\narg=firmware\narg=--\narg=install\narg=\u{fffd}\n\
             PYTHONPATH=/usr/lib/y2-platform\nPYTHONDONTWRITEBYTECODE=1\nKEPT=inherited\nstdin=input\n"
        ),
        "exec in the same process (callers signal this PID), argv, environment and stdin unchanged"
    );
}

#[cfg(feature = "python-fallback")]
#[test]
fn only_a_fixture_root_may_name_another_interpreter() {
    let python = fake_python("real");
    let output = Command::new(BINARY)
        .args(["service-daemon"])
        .env_remove("Y2_ROOT")
        .env("Y2_PLATFORM_PYTHON", &python)
        .env("Y2_PLATFORM_MAIN", "/fixture/main.py")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&output.stdout).contains("arg=/fixture/main.py"));
}

#[cfg(feature = "python-fallback")]
#[test]
fn a_missing_interpreter_fails_like_the_shell() {
    let output = Command::new(BINARY)
        .args(["service-daemon"])
        .env("Y2_ROOT", "/nonexistent")
        .env("Y2_PLATFORM_PYTHON", "/nonexistent/python3")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(127));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).starts_with("y2-platform: /nonexistent/python3: ")
    );
}

#[cfg(feature = "python-fallback")]
#[test]
fn a_parsed_command_no_route_takes_goes_to_python_as_given() {
    let python = fake_python("given");
    let output = Command::new(BINARY)
        .args(["service-daemon"])
        .env("Y2_ROOT", "/nonexistent")
        .env("Y2_PLATFORM_PYTHON", &python)
        .env("Y2_PLATFORM_MAIN", "/fixture/main.py")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("arg=/fixture/main.py\narg=service-daemon\nPYTHONPATH"));
}

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
