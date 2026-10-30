//! Persistent sleep and USB diagnostics (`sleep-diagnostics.jsonl`) that survive a hold-Power restart.
//!
//! POWER-FINAL-14. 13's counters (VPROC sleep, PHY save-current, wheel sleep, INFRA power-down, the
//! golden capture at sleep entry) live only in kernel memory, and the USB-after-wake defect is only
//! recoverable by a restart, which erased them twice. Each row here is read-only sysfs/procfs plus a
//! filtered kernel-log tail, appended to a bounded private file on Y2DATA. A failure is counted and
//! never blocks, delays or changes sleep.
// SPDX-License-Identifier: GPL-2.0-only
use super::E;
use y2p_core::mounts::data_ready;
use y2p_core::Root;

#[cfg(test)]
pub const PATH: &str = "/data/system/platform/sleep-diagnostics.jsonl";

/// `Diagnostics(ctx).record(stage, golden=stage == 'kernel_resumed')`: one row for a suspend
/// stage. `Ok(false)` when the data mount is not verified or a failure was counted.
pub fn record(root: &Root, stage: &str) -> Result<bool, E> {
    match row(root, stage, stage == "kernel_resumed") {
        Ok(written) => Ok(written),
        Err(error) if error.is_recorded_failure() => Ok(false),
        Err(error) => Err(error),
    }
}

fn row(root: &Root, stage: &str, golden: bool) -> Result<bool, E> {
    if !data_ready(root) {
        return Ok(false);
    }
    // A row that cannot be made or written is counted and dropped, as an OS error is.
    crate::commands::power::journal::diagnostics::write_row(
        root,
        stage,
        None,
        super::monotonic(),
        golden,
    )
    .map(|()| true)
    .map_err(|_| E::value("sleep_diagnostics_row_dropped"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::power::journal::usb;
    use crate::commands::suspend::tests::Dir;
    use std::fs;

    fn put(dir: &Dir, name: &str, text: &str) {
        let path = dir.path().join(name.trim_start_matches('/'));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn addresses_come_from_the_fib_trie() {
        let dir = Dir::new();
        put(
            &dir,
            "/proc/net/route",
            "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\n\
             usb0\t0002A8C0\t00000000\t0001\t0\t0\t0\t00FFFFFF\n",
        );
        put(
            &dir,
            "/proc/net/fib_trie",
            "Main:\n  +-- 0.0.0.0/0 3 0 5\n     |-- 192.168.2.0\n        /24 link UNICAST\n     \
             |-- 192.168.2.1\n        /32 host LOCAL\n     |-- 10.0.0.1\n        /32 host LOCAL\n",
        );
        let root = Root::new(dir.path());
        assert_eq!(usb::ipv4(&root, "usb0").unwrap(), ["192.168.2.1"]);
        assert!(usb::ipv4(&root, "wlan0").unwrap().is_empty());
        assert_eq!(
            usb::key_values(Some("stage=a error=0 x=1"), &["stage", "irqs"]),
            serde_json::json!({"stage": "a", "irqs": null})
        );
    }
}

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod diagnostics_tests;
