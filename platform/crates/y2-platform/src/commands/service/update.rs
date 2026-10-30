//! `y2-platform update`: signed full-system OTA (OTA-35): check, download, install request, boot
//! health.
//!
//! The small native core (`/usr/sbin/y2-update-core`, C) is the shared authentication/policy
//! authority. It writes the RECOVERY image and the MISC request; the Y2 recovery image performs the
//! offline backup/write/readback of BOOTIMG and Y2ROOT. This module never opens a block device itself.
//! `/data/updates/state.json` is the core's journal; `download.json`, `last-check.json` and
//! `/run/y2/update-progress.json` are this module's records, read by Reborn (`client.rs`).
//!
//! Here: the routed command, the refusals and the shared limits. `source` (where a package comes
//! from), `staging` (check and stage), `journal` (the core's journal and answers), `apply` (install,
//! rollback, cancel), `check` (the daily check), `health` (the first boot's confirmation).
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    download::{self, Error as Net, Response, CANCELLED},
    storage,
};
use crate::reply::{atomic_json, private_directory, Failure, Outcome, Reply};
use serde_json::{json, Map, Value};
use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::Ordering,
    time::Duration,
};
use y2p_core::lock;
use y2p_core::sys::{set_signal, signal_disposition, Disposition, Handler};
use y2p_core::{
    clock, command,
    digest::Sha256,
    json::{sorted, Object, ObjectExt},
    py::{self, is_true, truthy},
    record, Root,
};

mod apply;
mod check;
mod health;
mod journal;
pub(crate) use journal::{state as journal_state, status as journal_status, JournalState};
mod source;
mod staging;

use self::{apply::*, check::*, health::*, journal::*, source::*, staging::*};
pub(super) use self::{
    check::{within, LastCheck},
    health::ApplicationReady,
};

const FILES: [&str; 5] = [
    "manifest.json",
    "manifest.sig",
    "rootfs.ext4.gz",
    "bootimg.img.gz",
    "recovery.img.gz",
];
/// `update check/stage` while another one holds .stage.lock.
const IN_PROGRESS: &str = "update_in_progress";
const RESERVE: i128 = 96 * 1024 * 1024;
const CHANNEL: &str = "/etc/y2linux/update-channel.json";
/// Owner gate before the device turns off to install (decision 2026-10-15).
const MINIMUM_PERCENT: i64 = 50;
/// A check that reached the server holds off the next for a day.
pub(super) const CHECK_INTERVAL: f64 = 24.0 * 3600.0;
/// The service tries a due check at most once an hour (any attempt is recorded, so an unready clock
/// or no Wi-Fi does not start one every pass).
pub(super) const ATTEMPT_INTERVAL: f64 = 3600.0;
const LAST_CHECK: &str = "/data/updates/last-check.json";
/// Wi-Fi needs 15-20 s to reconnect after the player wakes; a Check for Updates pressed meanwhile
/// failed at once ("name resolution") and hid Update Now.
const NETWORK_WAIT_S: f64 = 30.0;
const MANIFEST_ATTEMPTS: u32 = 3;
/// The pause before another manifest attempt.
const RETRY_PAUSE: Duration = Duration::from_secs(3);
const PROGRESS: &str = "/run/y2/update-progress.json";
const CHECK_PROGRESS: &str = "/run/y2/update-check-progress";
/// Rollback copies of the running system. The recovery deletes them right before it writes the new
/// ones, so they count as space the update can use.
const BACKUPS: [&str; 2] = ["previous.ext4.gz", "previous-bootimg.gz"];
/// Kept in /data/logs: the live system log and the last installer run.
const KEPT_LOGS: [&str; 4] = [
    "system.log",
    "recovery-last.log",
    "recovery-display.txt",
    "recovery-previous-boot.txt",
];
/// Owner decision (2026-10-28): the running system restarts 10 s after a kernel panic; init sets it
/// before the handover on every boot (the command line keeps panic=0 for the rescue).
const RUNNING_PANIC_SECONDS: &str = "10";

/// Why an update action is refused: its text is the record's `failure` (Reborn's client.rs matches
/// words of it).
#[derive(Debug)]
pub enum Raise {
    Value(String),
    Os(String),
    /// `update_data_space_reserve`, with the bytes the update needs.
    NoSpace(i128),
    Net(Net),
}

impl Raise {
    pub fn text(&self) -> String {
        match self {
            Raise::Value(text) | Raise::Os(text) => text.clone(),
            Raise::NoSpace(_) => "update_data_space_reserve".into(),
            Raise::Net(error) => error.text(),
        }
    }

    fn value(text: &str) -> Raise {
        Raise::Value(text.into())
    }

    fn os(error: std::io::Error, path: &Path) -> Raise {
        Raise::Os(py::os_error(&error, Some(&path.to_string_lossy())).0)
    }

    fn cancelled(&self) -> bool {
        matches!(self, Raise::Net(Net::Cancelled))
    }
}

/// An `OSError` naming no file.
impl From<std::io::Error> for Raise {
    fn from(error: std::io::Error) -> Raise {
        Raise::Os(y2p_core::py::os_error(&error, None).0)
    }
}

impl From<Failure> for Raise {
    fn from(failure: Failure) -> Raise {
        match failure {
            Failure::Value(text) => Raise::Value(text),
            Failure::Os { text, .. } => Raise::Os(text),
            // A record another group's helper cannot read: refused like the others, naming what it
            // found.
            other => Raise::Value(format!("record_unreadable: {}", other.message())),
        }
    }
}

type Result<T> = std::result::Result<T, Raise>;

/// A record's string field (any other value is no string).
fn string(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str)
}

/// The first `limit` characters of `text`: a failure kept in a record stays short.
fn shortened(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// The routed form: `update <action> [--url URL] [--package DIR]`.
pub fn run(root: &Root, action: &str, url: Option<&str>, package: Option<&str>) -> Outcome {
    let result = (|| -> Result<Value> {
        match action {
            "check" | "stage" => {
                let url = match url.filter(|u| !u.is_empty()) {
                    Some(url) => Some(url.to_owned()),
                    None if package.is_none_or(str::is_empty) => channel_url(root)?,
                    None => None,
                };
                stage(root, &Player, package, url.as_deref(), action == "check")
            }
            "auto-check" => automatic_check(root, &Player),
            "probe-recovery" => probe(root),
            "recovery-status" => Ok(Value::Object(core(root, "misc", None, 15.0)?)),
            "apply" | "rollback" => apply(root, action == "rollback"),
            "cancel" => cancel_update(root),
            "health-ack" => health_ack(root),
            _ => Ok(Value::Object(status(root))),
        }
    })();
    match result {
        Ok(value) => Reply::sorted(&value, 0),
        Err(error) => Reply::unsorted(
            &Object::new()
                .with("state", "Failed")
                .with("failure", error.text()),
            1,
        ),
    }
}

#[cfg(test)]
#[path = "update_tests.rs"]
mod update_tests;

/// `stage` keeps process-wide state (CANCELLED, the SIGTERM disposition): tests that run it, here
/// and in update_tests.rs, take the process-wide signal lock.
#[cfg(test)]
pub(super) fn test_serial() -> std::sync::MutexGuard<'static, ()> {
    crate::testing::process_signals()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{ok, Fixture};

    /// A player whose verified /data is mounted (the transfer volume `stage` needs).
    fn data_ready() -> Fixture {
        let fixture = Fixture::new();
        fixture
            .put("/proc/sys/kernel/random/boot_id", "boot-a\n")
            .put(
                "/proc/self/mountinfo",
                "30 22 179:9 / /data rw,noatime - ext4 /dev/mmcblk0p9 rw\n",
            )
            .put(
                "/sys/devices/platform/11230000.mmc/mmc_host/mmc0/mmc0:0001/block/mmcblk0/mmcblk0p9/dev",
                "179:9\n",
            )
            .mkdir("/sys/dev/block")
            .mkdir("/data/updates");
        std::os::unix::fs::symlink(
            "../../devices/platform/11230000.mmc/mmc_host/mmc0/mmc0:0001/block/mmcblk0/mmcblk0p9",
            fixture.path("/sys/dev/block/179:9"),
        )
        .unwrap();
        fixture
    }

    fn data_uuid() -> Vec<(Vec<String>, command::Outcome)> {
        vec![ok(
            &["blkid", "-p", "-s", "UUID", "-o", "value", "/dev/mmcblk0p9"],
            "79324c69-6e75-4801-8000-000000000102",
        )]
    }

    #[test]
    fn a_busy_stage_lock_leaves_the_running_download_alone() {
        let _serial = test_serial();
        let fixture = data_ready();
        let staging = r#"{"schema":1,"state":"Staging","id":".incoming-first"}"#;
        fixture.put_mode("/data/updates/download.json", staging, 0o600);
        fixture.put_mode("/data/updates/.stage.lock", "", 0o600);
        // The first download holds the lock on its own open file.
        let first = File::open(fixture.path("/data/updates/.stage.lock")).unwrap();
        first.try_lock().unwrap();
        let root = fixture.root.clone().with_answers(data_uuid());
        let result = stage(
            &root,
            &Player,
            None,
            Some("https://updates.example/m.json"),
            true,
        );
        assert_eq!(result.err().map(|e| e.text()).as_deref(), Some(IN_PROGRESS));
        assert_eq!(fixture.read("/data/updates/download.json"), staging);
    }

    #[test]
    fn auto_check_says_why_it_did_not_check() {
        let _serial = test_serial();
        let fixture = Fixture::new();
        let now = Player.now();
        fixture.put_json(
            "/data/updates/last-check.json",
            &json!({"schema": 1, "result": "checked", "time": now - 60.0}),
        );
        assert_eq!(
            automatic_check(&fixture.root, &Player).ok(),
            Some(json!({"schema": 1, "result": "skipped", "reason": "not_due"}))
        );
        // Due, but nothing to check against: the attempt is recorded and the reason given.
        fixture.remove("/data/updates/last-check.json");
        let answer = automatic_check(&fixture.root, &Player).ok().unwrap();
        assert_eq!(answer["result"], json!("skipped"));
        assert!(answer["reason"].is_string());
        assert!(fixture.json("/data/updates/last-check.json")["attempt"].is_number());
    }

    #[test]
    fn a_rollback_on_a_low_battery_never_reaches_the_core() {
        let _serial = test_serial();
        let fixture = Fixture::new().with_answers(vec![]);
        fixture.put_json(
            "/run/y2/power.json",
            &json!({"battery": {"percent": 20, "charging": false, "status": "Discharging"}}),
        );
        let result = apply(&fixture.root, true);
        assert_eq!(
            result.err().map(|e| e.text()).as_deref(),
            Some("update_needs_charger_or_half_battery")
        );
        assert!(fixture.calls().is_empty(), "{:?}", fixture.calls());
    }

    #[test]
    fn health_ack_asks_for_the_rollback_once_its_deadline_passed() {
        let clock = std::sync::Arc::new(y2p_core::clock::Manual::at(50_000_000_000, 0));
        let core = |action: &str| {
            ok(
                &["/usr/sbin/y2-update-core", action],
                r#"{"state": "RollbackPending"}"#,
            )
        };
        let fixture = Fixture::new()
            .with_answers(vec![core("rollback"), core("prepare")])
            .with_clock(clock.clone());
        fixture
            .put("/proc/sys/kernel/random/boot_id", "boot-a\n")
            .put_json(
                "/data/updates/state.json",
                &json!({"schema": 2, "state": "PendingHealth"}),
            );
        // This boot's first pass sets the deadline 180 s on.
        let waiting = health_ack(&fixture.root).ok().unwrap();
        assert_eq!(waiting["reason"], json!("application_first_frame_missing"));
        assert_eq!(
            fixture.json("/run/y2/update-health.json")["deadline_monotonic_s"],
            json!(230.0)
        );
        clock.advance(179_000_000_000);
        assert_eq!(
            health_ack(&fixture.root).ok().unwrap()["state"],
            json!("PendingHealth")
        );
        assert!(fixture.calls().is_empty());
        clock.advance(1_000_000_000);
        let asked = health_ack(&fixture.root).ok().unwrap();
        assert_eq!(asked["state"], json!("RollbackPending"));
        let actions: Vec<String> = fixture
            .calls()
            .into_iter()
            .map(|(argv, _)| argv[1].clone())
            .collect();
        assert_eq!(actions, ["rollback", "prepare"]);
    }

    #[test]
    fn maintenance_renews_the_workload_lease_until_it_ends() {
        let fixture = Fixture::new();
        fixture.put("/dev/y2-workload", "");
        let result = maintenance(&fixture.root, || {
            std::thread::sleep(Duration::from_millis(1500));
            Ok(7)
        });
        assert_eq!(result.ok(), Some(7));
        // Acquired, renewed when the worker starts and again a second later; none after the end.
        let writes = || {
            fixture
                .read("/dev/y2-workload")
                .matches("Maintenance 3000\n")
                .count()
        };
        let count = writes();
        assert!((3..=4).contains(&count), "{count}");
        std::thread::sleep(Duration::from_millis(1100));
        assert_eq!(writes(), count);
    }

    #[test]
    fn health_debug_values_are_numbers_only() {
        assert!(numbers_only("4\t4\t1\t7"));
        assert!(numbers_only("10"));
        assert!(!numbers_only(""));
        assert!(!numbers_only("4 x"));
        assert!(!numbers_only("-1"));
    }

    #[test]
    fn reborns_verdict_wins() {
        assert!(application_healthy(&json!({"healthy": true})));
        assert!(!application_healthy(
            &json!({"healthy": 1, "graphics": {"available": true}})
        ));
        assert!(application_healthy(
            &json!({"graphics": {"available": true}, "uptime_seconds": 10})
        ));
        assert!(!application_healthy(
            &json!({"graphics": {"available": true}, "uptime_seconds": 9.5})
        ));
        assert!(!application_healthy(&json!([])));
    }

    #[test]
    fn pathlib_spells_paths() {
        assert_eq!(pathlib(Path::new("/a//b/./"), "x"), PathBuf::from("/a/b/x"));
        assert_eq!(pathlib(Path::new("./pkg"), "x"), PathBuf::from("pkg/x"));
    }

    #[test]
    fn lenient_json_takes_raw_tabs() {
        assert_eq!(
            loads_lenient("{\"printk\": \"4\t4\t1\t7\"}"),
            Some(json!({"printk": "4\t4\t1\t7"}))
        );
        assert_eq!(loads_lenient("nope"), None);
    }

    #[test]
    fn payloads_keep_the_manifests_order() {
        // The map keeps the document's order; a repeated key keeps its first place and its last value,
        // as a Python dict does.
        let text = r#"{"payloads": {"rootfs": 1, "bootimg": 2, "recovery": 3, "bootimg": 4}}"#;
        let manifest: Map<String, Value> = serde_json::from_str(text).unwrap();
        let payloads: Vec<(&String, &Value)> =
            manifest["payloads"].as_object().unwrap().iter().collect();
        assert_eq!(
            payloads,
            [
                (&"rootfs".to_owned(), &json!(1)),
                (&"bootimg".to_owned(), &json!(4)),
                (&"recovery".to_owned(), &json!(3))
            ]
        );
    }

    #[test]
    fn json_errors_read_as_python() {
        let error = serde_json::from_str::<Value>("").unwrap_err();
        assert_eq!(
            json_error("", &error),
            "Expecting value: line 1 column 1 (char 0)"
        );
        let error = serde_json::from_str::<Value>("{} x").unwrap_err();
        assert_eq!(
            json_error("{} x", &error),
            "Extra data: line 1 column 4 (char 3)"
        );
    }
}
