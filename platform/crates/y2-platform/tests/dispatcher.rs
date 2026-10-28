//! The dispatcher hands every subcommand to Python exactly as the shell wrapper did.
// SPDX-License-Identifier: GPL-2.0-only
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Stdio},
};

const BINARY: &str = env!("CARGO_BIN_EXE_y2-platform");

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

#[test]
fn python_replaces_the_dispatcher_with_the_same_arguments() {
    let python = fake_python("same");
    let mut child = Command::new(BINARY)
        .args(["status", "--reborn", "--pss", "a b", ""])
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
        String::from_utf8(output.stdout).unwrap(),
        format!(
            "pid={pid}\narg=-I\narg=/fixture/main.py\narg=status\narg=--reborn\narg=--pss\narg=a b\narg=\n\
             PYTHONPATH=/usr/lib/y2-platform\nPYTHONDONTWRITEBYTECODE=1\nKEPT=inherited\nstdin=input\n"
        ),
        "exec in the same process (callers signal this PID), argv, environment and stdin unchanged"
    );
}

#[test]
fn only_a_fixture_root_may_name_another_interpreter() {
    let python = fake_python("real");
    let output = Command::new(BINARY)
        .arg("capabilities")
        .env_remove("Y2_ROOT")
        .env("Y2_PLATFORM_PYTHON", &python)
        .env("Y2_PLATFORM_MAIN", "/fixture/main.py")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&output.stdout).contains("arg=/fixture/main.py"));
}

#[test]
fn a_missing_interpreter_fails_like_the_shell() {
    let output = Command::new(BINARY)
        .arg("capabilities")
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
