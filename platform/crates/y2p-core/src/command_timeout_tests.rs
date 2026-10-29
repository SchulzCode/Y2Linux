//! tests/test_platform_update.py `test_a_timed_out_command_also_kills_what_its_child_started` (V1 code
//! review B5: a filesystem check outlived the `media reconcile` that started it).
// SPDX-License-Identifier: GPL-2.0-only
use super::*;

#[test]
fn a_timed_out_command_also_kills_what_its_child_started() {
    let directory = env::temp_dir().join(format!("y2-command-timeout-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let marker = directory.join("grandchild.pid");
    // The child starts a grandchild in a session of its own, then waits for it.
    let script = format!("setsid sleep 30 & echo $! > '{}'; wait", marker.display());
    let result = run(&Root::new("/"), &["/bin/sh", "-c", &script], 1.5);
    assert_eq!(result.reason.as_deref(), Some("command_timeout"));
    let pid: i32 = fs::read_to_string(&marker).unwrap().trim().parse().unwrap();
    fs::remove_dir_all(&directory).unwrap();
    for _ in 0..50 {
        match fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) if stat.rsplit_once(')').unwrap().1.split_whitespace().next() == Some("Z") => {
                return
            }
            Ok(_) => thread::sleep(Duration::from_millis(20)),
            Err(_) => return,
        }
    }
    // SAFETY: the test's own grandchild, still running.
    unsafe { libc::kill(pid, libc::SIGKILL) };
    panic!("grandchild survived the timeout");
}
