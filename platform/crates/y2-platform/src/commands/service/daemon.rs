//! `y2-platform service-daemon`: bounded readiness/maintenance work, separate from the
//! battery deadline owner. Every pass publishes `network.json` (Wi-Fi with the supplicant events, the
//! F52 self-heal and the DNS probe run in process), `coexistence.json` and `time-status.json`, ticks the
//! USB service and the Wi-Fi power policy, and starts the slow work in its own processes: `media
//! reconcile` and `media verify` (SD admission, F49), `firmware scan`, and on maintenance passes `space
//! --cleanup`, the hourly time floor (#53), the daily `update auto-check`, `report work`, `update
//! health-ack` and `boot-stage application_ready`.
//!
//! Below a fixture root the loop does not wait and ends after `Y2_DAEMON_TICKS` passes, and the
//! children are recorded, never started (process.rs; tests/platform_cases/service.py
//! `daemon_ticks`).
// SPDX-License-Identifier: GPL-2.0-only
use super::process::{self, Child, Start};
use super::{radiofw, update, usb::UsbService};
use crate::commands::connectivity::published::{Published, Record};
use crate::commands::connectivity::{
    observe_wifi::wifi,
    radio_policy::wifi_policy,
    wifi::{probe_dns, DnsProbeSchedule, EventMonitor, WifiRecovery},
};
use crate::commands::reports::state as reports;
use crate::commands::{
    storage::media::{self, inventory},
    time,
};
use crate::reply::At;
use crate::reply::{atomic_json, private_directory, Failure, Outcome};
use serde_json::{json, Map, Value};
use std::{env, sync::atomic::Ordering};
use y2p_core::{
    command,
    json::ObjectExt,
    py::{self, is_true, truthy},
    root::read,
    Root,
};

mod coexistence;
mod jobs;
mod signals;

// The error-report tests read the due time as the service does.
#[cfg(test)]
pub(crate) use self::jobs::{next_upload, Due};
use self::{coexistence::*, jobs::*, signals::*};

// Fix02 background attribution: every iteration used to start a complete Python interpreter for
// `media reconcile` (every 3 s) plus two more every 30 s. On the Y2 that recurring start-up work drove
// schedutil to 1040 MHz and >10% samples during screen-off idle. The reconcile subprocess is still the
// only owner of mount/unmount work; it is now started only when its own no-op condition (unchanged SD
// inventory, no retry) is false.
const INTERVAL_S: f64 = 3.0;
/// Reborn: network.json stale after 15 s (an Online record after 45 s).
const DARK_INTERVAL_S: f64 = 10.0;
const MAINTENANCE_S: f64 = 30.0;
const DARK_MAINTENANCE_S: f64 = 300.0;
// A card's read-only admission check alone may take 60 s (media.read_only_check); a run that is still
// busy after this is stopped, and the same card is tried again only after RECONCILE_BACKOFF_S.
const RECONCILE_LIMIT_S: f64 = 150.0;
const RECONCILE_BACKOFF_S: f64 = 600.0;
/// A background check (`media verify`, F49) that ended without a verdict (the player slept, the card
/// was busy) starts again after this.
const VERIFY_RETRY_S: f64 = 30.0;
// Error reports (ERROR-REPORTS-PLAN): look for new triggers this often, the first time shortly after
// start (a crash record of the previous boot); a run that builds and sends reports is stopped after
// REPORT_LIMIT_S.
const REPORT_FIRST_S: f64 = 120.0;
const REPORT_CHECK_S: f64 = 900.0;
const REPORT_LIMIT_S: f64 = 600.0;
/// A blocked upload (battery, clock) is not retried every pass.
const REPORT_SEND_GAP_S: f64 = 300.0;
const PLATFORM: &str = "/usr/sbin/y2-platform";

/// Same meaning as the kernel's backlight_is_blank(): Reborn blanks with bl_power=4
/// (FB_BLANK_POWERDOWN) and leaves brightness unchanged.
fn display_dark(root: &Root) -> bool {
    let devices = root.glob("/sys/class/backlight/*");
    let read = |path: &std::path::Path| read(path, y2p_core::root::READ_LIMIT);
    !devices.is_empty()
        && devices.iter().all(|device| {
            let power = read(&device.join("bl_power"));
            let brightness = read(&device.join("brightness"));
            !matches!(power.as_deref(), None | Some("0")) || brightness.as_deref() == Some("0")
        })
}

fn cadence(dark: bool, settled: bool) -> (f64, f64) {
    (
        if dark { DARK_INTERVAL_S } else { INTERVAL_S },
        if dark && settled {
            DARK_MAINTENANCE_S
        } else {
            MAINTENANCE_S
        },
    )
}

#[cfg(test)]
#[path = "daemon_connectivity_tests.rs"]
mod connectivity_tests;

/// What `S43y2-time start` checks before it starts ntpd: `/run/y2/ntp.pid` names a live process
/// (`[ -s ntp.pid ] && kill -0 "$(cat ntp.pid)"`). Checked here, a running ntpd costs no shell.
fn ntp_running(root: &Root) -> bool {
    root.integer("/run/y2/ntp.pid")
        .is_some_and(|pid| pid > 1 && y2p_core::sys::alive(pid))
}

/// Reborn drew while the boot journal still waits for it: `boot-stage application_ready`. Whether the
/// journal is past that wait (nothing more to record this boot).
fn application_drawn(root: &Root) -> bool {
    if !update::ApplicationReady::read(root).drawn(root) {
        return false;
    }
    let waiting = matches!(
        root.json_object("/data/system/platform/boot.json")
            .get("last_stage")
            .and_then(Value::as_str),
        Some("platform_start" | "services_started")
    );
    if waiting {
        command::run(root, &[PLATFORM, "boot-stage", "application_ready"], 2.0);
    }
    !waiting
}

/// What the loop keeps between passes.
struct Service {
    root: Root,
    monitor: EventMonitor,
    recovery: WifiRecovery,
    usb: UsbService,
}

impl Service {
    fn serve(&mut self, ticks: Option<u64>) -> Result<(), Failure> {
        let root = self.root.clone();
        let root = &root;
        let runtime = root.path("/run/y2");
        let wake = SignalWake::new()?;
        let mut dns_schedule = DnsProbeSchedule::new();
        let (mut maintenance_timer, mut ntp_timer) = (Timer::default(), Timer::default());
        let mut associating_since: Option<f64> = None;
        let mut coexistence = Coexistence::default();
        let mut radio_policy_timer = Timer::default();
        let mut settled = false;
        let mut drawn_recorded = false;
        // It cannot change while the service runs.
        let boot_id = root.boot_id();
        let previous = Published::read(root);
        let mut query_failures = previous
            .status_query_failures
            .filter(|_| previous.is_this_boot(boot_id.as_deref()))
            .unwrap_or_default();
        let mut update_check: Option<Child> = None;
        let mut reports: Option<Reports> = None;
        let (mut reconcile, mut verify) = (Run::default(), Run::default());
        let (mut firmware_scan, mut firmware_ready): (Option<Child>, bool) = (None, false);
        let mut done = 0u64;
        while RUNNING.load(Ordering::SeqCst) {
            wake.drain();
            let before = root.clock().monotonic();
            let dark = display_dark(root);
            let (mut interval, maintenance) = cadence(dark, settled);
            self.monitor.poll();
            self.usb.tick()?;
            let mut value = wifi(root, None);
            if value.state == "Associating" {
                let since = *associating_since.get_or_insert(before);
                if before - since > 30.0 {
                    value.state = "Failed".into();
                    value.reason = Some("authentication_timeout".into());
                }
            } else {
                associating_since = None;
            }
            let epoch = root.read("/run/y2/wifi-epoch");
            if dns_schedule.due(&value, epoch.as_deref(), before) {
                let probed = probe_dns(root, &value)?;
                if probed.is_some_and(|p| is_true(p.get("ok"))) {
                    dns_schedule.confirmed(before);
                }
                value = wifi(root, Some(&value));
            }
            let observation_failed = value.supplicant_observation.ok == Some(false);
            if value.radio_present && observation_failed {
                query_failures = query_failures.saturating_add(1);
            }
            self.recovery.observe(&value, before, Some(&self.monitor));
            let record = Record {
                observation: &value,
                status_query_failures: query_failures,
                schema: 1,
                boot_id: boot_id.as_deref(),
                monotonic_s: root.clock().monotonic(),
                supplicant_events: self.monitor.summary(None).into_sorted(),
            };
            atomic_json(&runtime.join("network.json"), &record, false)?;
            let activity = coexistence.observe(root, &value, before)?;
            if value.association.as_deref() == Some("COMPLETED")
                && radio_policy_timer.due(before, 30.0)
            {
                wifi_policy(root, Some(&activity))?;
            }
            atomic_json(
                &runtime.join("time-status.json"),
                &super::time_status(root),
                false,
            )?;
            reconcile = media_reconcile(root, reconcile, before);
            verify = media_verify(root, verify, before);
            if value.state == "Online" && ntp_timer.due(before, 60.0) && !ntp_running(root) {
                command::run(root, &["/etc/init.d/S43y2-time", "start"], 1.0);
            }
            let due = maintenance_timer.due(before, maintenance);
            (firmware_scan, firmware_ready) =
                radio_firmware(root, firmware_scan, firmware_ready, due);
            if due {
                // The space policy and its bounded cleanup (64 files, 8 MiB) look at Y2DATA only, which
                // this daemon reads every pass anyway: in process, no y2-platform start each pass.
                // The SD card (whose statvfs can hang in D state) is not touched.
                let _ = crate::commands::storage::space_policy(root, true);
                // A restart without Wi-Fi resumes near this clock (#53); hourly.
                match remember_time(root) {
                    Err(Failure::Os { .. }) | Ok(()) => {}
                    Err(other) => return Err(other),
                }
                update_check = daily_update_check(root, &value, update_check);
                reports = Some(error_reports(root, &value, reports, before)?);
                settled = settled
                    || health_settled(&command::run(
                        root,
                        &[PLATFORM, "update", "health-ack"],
                        8.0,
                    ));
                if !settled {
                    application_drawn(root);
                }
            }
            // Once no update waits for health (whose acknowledgement records the stage itself), the
            // stage follows Reborn's first frame within a pass instead of on the next maintenance pass
            // up to 30 s later: it arms the runtime power features and lets a sleep enter.
            if settled && !drawn_recorded {
                drawn_recorded = application_drawn(root);
            }
            if matches!(
                value.state.as_str(),
                "Scanning" | "Associating" | "AcquiringIP"
            ) {
                interval = interval.min(1.0);
            }
            if ticks.is_none() {
                let left = interval - (root.clock().monotonic() - before);
                self.monitor.wait(left.max(0.05), Some(wake.fd()));
            }
            done += 1;
            if ticks.is_some_and(|n| done >= n) {
                break;
            }
        }
        drop(wake);
        Ok(())
    }
}

/// `y2-platform service-daemon`: prints nothing and exits 0 when stopped (SIGTERM/SIGINT); a second
/// instance is refused (BlockingIOError) before it starts anything. An error escaping the loop ends
/// it (exit 1 with the error's record) after the USB gadget
/// and the responder are stopped; the supervisor restarts it.
pub fn serve(root: &Root) -> Outcome {
    let runtime = root.path("/run/y2");
    private_directory(&runtime)?;
    let path = runtime.join("service.lock");
    let lock = y2p_core::lock::try_exclusive(&path)
        .at(&path)?
        .ok_or_else(Failure::busy)?;
    let ticks = if root.is_real() {
        None
    } else {
        env::var("Y2_DAEMON_TICKS")
            .ok()
            .and_then(|t| t.parse().ok())
    };
    let mut service = Service {
        root: root.clone(),
        monitor: EventMonitor::new(root),
        recovery: WifiRecovery::new(root),
        usb: UsbService::new(root),
    };
    let mut result = service.serve(ticks);
    // finally: the USB service stops first; its error replaces the loop's.
    if let Err(error) = service.usb.stop() {
        result = Err(error);
    }
    service.monitor.close();
    drop(lock);
    match result {
        // A daemon prints nothing (main.rs would print a newline for a reply).
        Ok(()) => std::process::exit(0),
        // main.rs prints the error's record (exit 1); the supervisor starts the service again.
        Err(error) => Err(error),
    }
}

#[cfg(test)]
#[path = "daemon_tests.rs"]
mod daemon_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_ack_answers_settle_the_cadence() {
        let answer = |ok: bool, output: &str| command::Outcome {
            ok,
            reason: None,
            output: Some(output.into()),
        };
        assert!(health_settled(&answer(
            true,
            r#"{"state": "Acknowledged"}"#
        )));
        assert!(health_settled(&answer(true, r#"{"state": ["x"]}"#)));
        assert!(!health_settled(&answer(
            true,
            r#"{"state": "PendingHealth"}"#
        )));
        assert!(!health_settled(&answer(true, r#"{"state": null}"#)));
        assert!(!health_settled(&answer(true, r#"{"other": 1}"#)));
        assert!(!health_settled(&answer(true, "[1]")));
        assert!(!health_settled(&answer(false, r#"{"state": "Idle"}"#)));
        assert_eq!(cadence(true, true), (10.0, 300.0));
        assert_eq!(cadence(true, false), (10.0, 30.0));
        assert_eq!(cadence(false, true), (3.0, 30.0));
    }

    #[test]
    fn a_second_instance_is_refused() {
        let dir = std::env::temp_dir().join(format!("y2-service-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("run/y2")).unwrap();
        let held = y2p_core::lock::exclusive(&dir.join("run/y2/service.lock")).unwrap();
        let error = serve(&Root::new(&dir)).err().expect("refused");
        assert_eq!(
            error.message(),
            "[Errno 11] Resource temporarily unavailable"
        );
        assert!(matches!(
            error,
            Failure::Os {
                class: "BlockingIOError",
                ..
            }
        ));
        drop(held);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
#[path = "daemon_reports_tests.rs"]
mod daemon_reports_tests;

#[cfg(test)]
#[path = "daemon_wake_tests.rs"]
mod daemon_wake_tests;
