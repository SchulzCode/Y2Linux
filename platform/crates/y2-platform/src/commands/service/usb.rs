//! The USB service: consumer MTP + advanced ECM;
//! Linux is the only block/filesystem owner. No block-export function is selectable. The responder
//! pins media roots per lifetime; mount identity changes require unbinding/stopping it before a
//! replacement view. `service-daemon` ticks it every pass and stops it when it ends.
// SPDX-License-Identifier: GPL-2.0-only
use super::process::{self, Child, Launch, Start};
use super::radiofw;
use crate::commands::storage::{
    internal, media,
    observe::{setup_status, volume_at},
    sys::{emmc_entry, mounts, read, Mount},
};
use crate::reply::{atomic_json, mkdir_parents, write_text, Failure};
use serde_json::{json, Value};
use std::{fs, path::Path, thread, time::Duration};
use y2p_core::py::PyValue;
use y2p_core::{py, Root};

mod gadget;
mod responder;
mod views;

use self::views::*;

const GADGET: &str = "/sys/kernel/config/usb_gadget/y2";
const SCHEMA: &str = "org.y2linux.usb-transfer/v1";
const STABLE_SECONDS: f64 = 60.0;
const OLD: &str = "/run/y2/old-storage";
/// The fixed MTP storages, in their slot order (stable MTP storage IDs).
const NAMES: [(&str, &str); 4] = [
    ("/media/internal/music", "Internal Storage"),
    ("/media/sd", "SD Card"),
    ("/data/firmware/inbox", "Firmware"),
    ("/run/y2/old-storage", "Old Storage"),
];
// A connected computer may write to the SD card at any time: the protected card (media.py) is
// writable while the host has the player enumerated, and read-only again once the cable is gone.
// Renewed every 10 s for 30 s.
const SD_WINDOW_RENEW: f64 = 10.0;
const SD_WINDOW_SECONDS: i64 = 30;

/// `selection(mode)`: the gadget functions of a mode.
fn selection(mode: &str) -> Result<&'static [&'static str], Failure> {
    match mode {
        "mtp" => Ok(&["ffs.mtp", "ecm.usb0"]),
        "developer" => Ok(&["acm.usb0", "ecm.usb0"]),
        _ => Err(Failure::value("unsupported_usb_mode")),
    }
}

/// /run/y2/usb-transfer.json (org.y2linux.usb-transfer/v1): what Reborn's USB screen and the
/// storage commands read of the connection.
#[derive(serde::Serialize)]
struct Transfer<'a> {
    schema: &'static str,
    boot_id: Option<String>,
    state: &'a str,
    reason: Option<&'a str>,
    mode: Option<&'static str>,
    cable: &'a Option<String>,
    functions: &'static [&'static str],
    views: &'a [View],
    filesystem_owner: &'static str,
    block_export: bool,
    refresh_generation: u64,
    active: bool,
    error: &'a Value,
    restarts: i64,
}

/// What the running responder reports in /run/y2/mtp-events.json (a record of another responder
/// counts as none): a copy in progress, its last error (28: a copy that did not fit) and how many
/// changes to the library it made (an integer; a bool counts as one).
#[derive(Default)]
struct Activity {
    active: bool,
    error: Value,
    generation: Option<i64>,
}

impl Activity {
    fn read(root: &Root, responder: Option<i32>) -> Self {
        let Some(pid) = responder else {
            return Self::default();
        };
        let record = root.json_object("/run/y2/mtp-events.json");
        if !record.get("pid").unwrap_or(&Value::Null).py_eq(&json!(pid)) {
            return Self::default();
        }
        Self {
            active: record.get("active").is_some_and(py::truthy),
            error: record.get("error").cloned().unwrap_or(Value::Null),
            generation: match record.get("generation") {
                Some(Value::Bool(b)) => Some(i64::from(*b)),
                other => other.and_then(Value::as_i64),
            },
        }
    }
}

/// What `tick` compares to reuse validated views until identity/options change.
#[derive(PartialEq)]
struct Signature {
    present: bool,
    files: Vec<Option<String>>,
    cards: Vec<(String, Option<String>, Option<String>)>,
}

pub struct UsbService {
    root: Root,
    child: Option<Child>,
    mode: Option<&'static str>,
    views: Option<Vec<View>>,
    bound: bool,
    failures: i64,
    started: f64,
    previous_cable: Option<String>,
    seen_generation: i64,
    refresh_generation: u64,
    last_record: Option<Value>,
    signature: Option<Signature>,
    cached_views: Vec<View>,
    terminal: bool,
    /// path -> identity, in insertion order (a dict).
    mounted: Vec<(String, Identity)>,
    sd_window: f64,
    /// This boot's id, read for the first record: it cannot change while the service runs.
    boot_id: Option<Option<String>>,
    /// What starts the responder and its IPC commands.
    launch: Launch,
}

impl UsbService {
    pub fn new(root: &Root) -> Self {
        Self {
            root: root.clone(),
            child: None,
            mode: None,
            views: None,
            bound: false,
            failures: 0,
            started: 0.0,
            previous_cable: None,
            seen_generation: 0,
            refresh_generation: 0,
            last_record: None,
            signature: None,
            cached_views: Vec::new(),
            terminal: false,
            mounted: Vec::new(),
            sd_window: 0.0,
            boot_id: None,
            launch: process::launcher(),
        }
    }

    fn child_running(&mut self) -> bool {
        self.child.as_mut().is_some_and(Child::running)
    }

    /// usb-transfer.json, written only when it changed; its value.
    fn record(
        &mut self,
        state: &str,
        reason: Option<&str>,
        cable: &Option<String>,
        activity: &Activity,
    ) -> Result<Value, Failure> {
        let root = &self.root;
        let boot_id = self.boot_id.get_or_insert_with(|| root.boot_id()).clone();
        let record = Transfer {
            schema: SCHEMA,
            boot_id,
            state,
            reason,
            mode: self.mode,
            cable,
            functions: self.mode.map_or(Ok(&[][..]), selection)?,
            views: self.views.as_deref().unwrap_or_default(),
            filesystem_owner: "linux",
            block_export: false,
            refresh_generation: self.refresh_generation,
            active: activity.active,
            error: &activity.error,
            restarts: self.failures,
        };
        let value = serde_json::to_value(&record).map_err(Failure::value)?;
        if !self.last_record.as_ref().is_some_and(|l| l.py_eq(&value)) {
            atomic_json(&self.root.path("/run/y2/usb-transfer.json"), &value, false)?;
            self.last_record = Some(value.clone());
        }
        Ok(value)
    }

    /// A connected computer may write to the SD card: its window is renewed while the host has the
    /// player enumerated and ended once the cable is gone. A window that cannot be had (no card, a
    /// check running, an unreadable record) never stops the service.
    fn sd_writable(&mut self, cable: &Option<String>) {
        let host = matches!(cable.as_deref(), Some("configured" | "suspended"));
        let now = self.root.clock().monotonic();
        if host && now - self.sd_window >= SD_WINDOW_RENEW {
            // Never wait for the card's lock here: a check or repair holding it is asked again next
            // tick (the window is renewed well before it ends).
            let answer =
                media::write_window_within(&self.root, "usb", SD_WINDOW_SECONDS, false, 0.0);
            if answer.is_ok_and(|answer| answer.get("holders").is_some_and(py::truthy)) {
                self.sd_window = now;
            }
        } else if !host
            && self.sd_window != 0.0
            && media::write_window_within(&self.root, "usb", 60, true, 0.0).is_ok_and(|answer| {
                answer.get("reason") != Some(&json!("media_operation_in_progress"))
            })
        {
            self.sd_window = 0.0;
        }
    }

    /// One pass: the record `/run/y2/usb-transfer.json` shows (written only when it changed).
    pub fn tick(&mut self) -> Result<Value, Failure> {
        mkdir_parents(&self.root.path("/run/y2"), 0o777)?;
        let udcs = self.root.glob("/sys/class/udc/*");
        if udcs.len() != 1 {
            return self.record(
                "Unavailable",
                Some("no_unique_udc"),
                &None,
                &Activity::default(),
            );
        }
        let udc = &udcs[0];
        let cable = read(&udc.join("state"));
        self.sd_writable(&cable);
        if self.terminal {
            return self.record(
                "Failed",
                Some("responder_restart_budget"),
                &cable,
                &Activity::default(),
            );
        }
        // Anything but a mode's name is refused by `attempt` (unsupported_usb_mode).
        let requested = self
            .root
            .json_object("/data/system/platform/usb-mode.json")
            .get("mode")
            .cloned()
            .unwrap_or_else(|| json!("mtp"));
        let name = udc
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match self.attempt(requested, &cable, &name) {
            Err(error) => {
                if self.stop().is_err() {
                    self.terminal = true;
                }
                if self.started != 0.0
                    && self.root.clock().monotonic() - self.started > STABLE_SECONDS
                {
                    self.failures = 0;
                }
                self.failures += 1;
                self.terminal = self.terminal || self.failures >= 5;
                if self.failures < 5 {
                    self.mode = None;
                }
                self.record(
                    "Failed",
                    Some(&error.message()),
                    &cable,
                    &Activity::default(),
                )
            }
            other => other,
        }
    }

    /// The part of a tick whose refusal counts against the responder's restart budget.
    fn attempt(
        &mut self,
        requested: Value,
        cable: &Option<String>,
        udc: &str,
    ) -> Result<Value, Failure> {
        let requested_text = requested.as_str().unwrap_or_default();
        selection(requested_text).map_err(|e| {
            if requested.is_string() {
                e
            } else {
                Failure::value("unsupported_usb_mode")
            }
        })?;
        let mut requested: &'static str = if requested_text == "mtp" {
            "mtp"
        } else {
            "developer"
        };
        // Reuse validated views until identity/options change. This adds no recurring blkid
        // process, storage read, or polling wakeup.
        let root = &self.root;
        let signature = Signature {
            present: radiofw::present(root),
            files: [
                "/proc/self/mountinfo",
                "/run/y2/internal-media-setup.json",
                "/run/y2/internal-media.json",
                "/run/y2/media-mount.json",
                "/run/y2/usb-release-sd.json",
            ]
            .iter()
            .map(|f| root.read(f))
            .collect(),
            // The SD card's entries: the eMMC's never change while the player runs.
            cards: root
                .glob("/sys/class/block/mmcblk*")
                .iter()
                .filter(|p| !emmc_entry(p))
                .map(|p| {
                    (
                        p.file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        read(&p.join("dev")),
                        read(&p.join("device/cid")),
                    )
                })
                .collect(),
        };
        if self.signature.as_ref() != Some(&signature) {
            self.cached_views = media_views(root);
            self.signature = Some(signature);
        }
        let mut views = if requested == "mtp" {
            self.cached_views.clone()
        } else {
            Vec::new()
        };
        if let Some(mode) = self.mode {
            if requested != mode && cable.as_deref() != Some("not attached") {
                requested = mode;
                views = if mode == "mtp" {
                    self.cached_views.clone()
                } else {
                    Vec::new()
                };
            }
        }
        if self.mode != Some(requested) {
            if self.mode.is_some() {
                self.refresh_generation = self.root.clock().monotonic_ns();
            }
            self.setup(requested, views)?;
        } else if self.mode == Some("mtp")
            && self.views.as_ref() != Some(&views)
            && self.child_running()
        {
            self.reconcile(views)?;
        }
        if self.child.is_some() && !self.child_running() {
            self.stop()?;
            // A responder that served for a minute before dying is a new incident, not another step
            // toward the permanent-failure budget.
            if self.root.clock().monotonic() - self.started > STABLE_SECONDS {
                self.failures = 0;
            }
            self.failures += 1;
            if self.failures >= 5 {
                self.terminal = true;
                return self.record(
                    "Failed",
                    Some("responder_restart_budget"),
                    cable,
                    &Activity::default(),
                );
            }
            self.mode = None;
            return self.record(
                "Starting",
                Some("responder_restarting"),
                cable,
                &Activity::default(),
            );
        }
        if !self.bound {
            let ready = self.mode == Some("developer")
                || (self
                    .root
                    .read(&format!("{GADGET}/functions/ffs.mtp/ready"))
                    .as_deref()
                    == Some("1")
                    && (1..=3).all(|i| self.root.path(&format!("/dev/ffs-mtp/ep{i}")).exists()));
            if !ready {
                if self.root.clock().monotonic() - self.started > 15.0 {
                    return Err(Failure::os_message("responder_descriptor_timeout"));
                }
                return self.record(
                    "Starting",
                    Some("waiting_for_descriptors"),
                    cable,
                    &Activity::default(),
                );
            }
            self.write("UDC", udc)?;
            self.bound = true;
        }
        let activity = Activity::read(&self.root, self.child.as_ref().map(Child::pid));
        if let Some(generation) = activity.generation.filter(|g| *g > self.seen_generation) {
            self.seen_generation = generation;
            self.refresh_generation = self.root.clock().monotonic_ns();
        }
        let hosted = matches!(cable.as_deref(), Some("configured" | "suspended"));
        if self.previous_cable.as_deref() == Some("configured") && !hosted {
            self.refresh_generation = self.root.clock().monotonic_ns();
        }
        self.previous_cable = cable.clone();
        let mut state = match cable.as_deref() {
            Some("not attached") => "Disconnected",
            Some("configured") => "Ready",
            _ => "Starting",
        };
        if state == "Ready" && activity.active {
            state = "Copying";
        }
        if state == "Ready"
            && self.mode == Some("mtp")
            && self.views.as_ref().is_none_or(Vec::is_empty)
        {
            state = "Unavailable";
        }
        self.record(state, None, cable, &activity)
    }
}

#[cfg(test)]
#[path = "usb_tests.rs"]
mod usb_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn view_of(path: &'static str, access: &'static str) -> View {
        View {
            path,
            name: name_of(path),
            mount_id: 31,
            device_id: "179:8".into(),
            uuid: None,
            filesystem: "vfat".into(),
            access,
        }
    }

    #[test]
    fn the_responder_configuration_declares_every_slot() {
        let views = [view_of("/media/sd", "ro")];
        let text = config(&views).unwrap();
        assert!(text.ends_with(
            "storage \"/media/internal/music\" \"Internal Storage\" \"rw,notmounted\"\n\
             storage \"/media/sd\" \"SD Card\" \"ro,removable\"\n\
             storage \"/data/firmware/inbox\" \"Firmware\" \"rw,notmounted\"\n\
             storage \"/run/y2/old-storage\" \"Old Storage\" \"rw,notmounted\"\n"
        ));
        assert_eq!(expectations(&views), "/media/sd 179:8 31\n");
        let mut odd = view_of("/media/sd", "rw");
        odd.name = Some("Card");
        assert_eq!(config(&[odd]).err().unwrap().message(), "unsafe_media_view");
    }
}
