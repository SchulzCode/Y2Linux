//! Timer slack for the platform's background daemons (power plan 2026-11, phase 6a). The kernel may
//! fire a timer of a task up to its slack late, so timers of different tasks that fall due close
//! together are served by one CPU wake instead of several. The setting is per task and inherited by
//! every thread and child created afterwards (umtprd, dropbear, the Wi-Fi tools), so each daemon sets
//! it for itself before it starts anything. Reborn, the audio path (bluealsad, real-time anyway) and
//! the Bluetooth daemons keep the kernel's 50 µs.
// SPDX-License-Identifier: GPL-2.0-only
use std::{fs::OpenOptions, io::Write};
use y2p_core::Root;

/// 50 ms: far below any deadline these daemons keep (the battery sample every second, a 2 s safety
/// tick, network and USB retries in seconds), far above the default 50 µs.
pub const SLACK_NS: u64 = 50_000_000;

/// The daemons that take the slack (their canonical command lines).
const DAEMONS: [&str; 4] = [
    "power-daemon",
    "service-daemon",
    "connectivity-daemon",
    "ssh-daemon",
];

/// Whether `command` is one of the background daemons.
pub fn applies(command: &[&str]) -> bool {
    matches!(command, [name] if DAEMONS.contains(name))
}

/// Set this task's slack (`/proc/self/timerslack_ns`, writable by the task itself). Best effort: a
/// tree without the file (an older kernel, a test fixture) is left as it is, never created, and the
/// daemon runs the same either way.
pub fn apply(root: &Root, command: &[&str]) {
    if !applies(command) {
        return;
    }
    if let Ok(mut file) = OpenOptions::new()
        .write(true)
        .open(root.path("/proc/self/timerslack_ns"))
    {
        let _ = file.write_all(format!("{SLACK_NS}\n").as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Fixture;

    #[test]
    fn only_the_background_daemons_take_the_slack() {
        for daemon in DAEMONS {
            assert!(applies(&[daemon]));
        }
        for other in [
            &["power-daemon", "x"][..],
            &["status"],
            &["sleep", "request"],
            &[],
        ] {
            assert!(!applies(other), "{other:?}");
        }
    }

    #[test]
    fn the_daemon_writes_its_own_slack_only_where_the_kernel_offers_it() {
        let fixture = Fixture::new();
        fixture.put("/proc/self/timerslack_ns", "50000\n");
        apply(&fixture.root, &["service-daemon"]);
        let slack = || fixture.root.read("/proc/self/timerslack_ns");
        assert_eq!(slack().as_deref(), Some("50000000"));
        fixture.put("/proc/self/timerslack_ns", "50000\n");
        apply(&fixture.root, &["status"]);
        assert_eq!(slack().as_deref(), Some("50000"));
        let bare = Fixture::new();
        apply(&bare.root, &["power-daemon"]);
        assert!(!bare.root.path("/proc/self/timerslack_ns").exists());
    }
}
