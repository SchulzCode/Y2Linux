//! usb.py's `USBService` (and `media_views`, `config`, `expectations`): consumer MTP + advanced ECM;
//! Linux is the only block/filesystem owner. No block-export function is selectable. The responder
//! pins media roots per lifetime; mount identity changes require unbinding/stopping it before a
//! replacement view. `service-daemon` ticks it every pass and stops it when it ends.
// SPDX-License-Identifier: GPL-2.0-only
use super::process::{self, Child, Launch, Start};
use super::radiofw;
use crate::commands::storage::{
    internal, media,
    observe::{setup_status, volumes},
    sys::{mounts, read, Mount, Object},
};
use crate::reply::{atomic_json, mkdir_parents, write_text, Failure};
use serde_json::{json, Value};
use std::{fs, path::Path, thread, time::Duration};
use y2p_core::py::PyValue;
use y2p_core::{clock, py, Root};

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

fn name_of(path: &str) -> Option<&'static str> {
    NAMES.iter().find(|(p, _)| *p == path).map(|(_, n)| *n)
}

fn text<'a>(view: &'a Object, key: &str) -> Option<&'a str> {
    view.get(key).and_then(Value::as_str)
}

fn view(mount: &Mount, path: &str, uuid: Value, filesystem: &str, access: &str) -> Object {
    let mut value = Object::new();
    value.insert("path".into(), json!(path));
    value.insert("name".into(), json!(name_of(path)));
    value.insert("mount_id".into(), json!(mount.mount_id));
    value.insert("device_id".into(), json!(mount.device_id));
    value.insert("uuid".into(), uuid);
    value.insert("filesystem".into(), json!(filesystem));
    value.insert("access".into(), json!(access));
    value
}

/// `media_views(ctx)`: the host receives user music, never Y2DATA/private/system contents or a LUN.
fn media_views(root: &Root) -> Vec<Object> {
    let volumes = volumes(root);
    let mounts = mounts(root);
    let mut result = Vec::new();
    let at = |path: &str| mounts.iter().find(|m| m.path == path);
    // Internal Storage is the Y2Media partition's music folder (volumes[3]); the old /data/music folder
    // on Y2DATA is no longer exported.
    for (volume, path) in [
        (&volumes[3], "/media/internal/music"),
        (&volumes[2], "/media/sd"),
    ] {
        let mount = volume.get("path").and_then(Value::as_str).and_then(at);
        let folder = root.path(path);
        if path == "/media/internal/music" {
            let device = mount.map(|m| root.path(&format!("/sys/dev/block/{}", m.device_id)));
            let fits = device.is_some_and(|d| {
                read(&d.join("start")).as_deref() == Some("3742720")
                    && read(&d.join("size")).as_deref() == Some("11460608")
            });
            if !fits
                || volume.get("uuid").and_then(Value::as_str)
                    != Some("79324c69-6e75-4801-8000-000000000103")
                || mount.is_none_or(|m| m.filesystem != "ext4")
            {
                continue;
            }
        }
        // Existing validated internal partition and platform SD mount claim supply the identity.
        // Never create a view of an absent underlying mount.
        let release = root.json_object("/run/y2/usb-release-sd.json");
        if let Some(mount) = mount.filter(|_| path == "/media/sd") {
            if release
                .get("mount_id")
                .unwrap_or(&Value::Null)
                .py_eq(&json!(mount.mount_id))
            {
                continue;
            }
        }
        let state = volume.get("state").and_then(Value::as_str);
        let Some(mount) = mount else { continue };
        let nested = mounts.iter().any(|m| {
            m.path.starts_with(&format!("{path}/"))
                || (path == "/media/internal/music" && m.path == path)
        });
        let symlink = folder
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink());
        if matches!(state, Some("Ready" | "Degraded")) && folder.is_dir() && !symlink && !nested {
            let access = if mount.read_only() { "ro" } else { "rw" };
            let uuid = volume.get("uuid").cloned().unwrap_or(Value::Null);
            result.push(view(mount, path, uuid, &mount.filesystem, access));
        }
    }
    // STORAGE-SETUP case 3: the stock storage, read-only, until the listener sets up Internal Storage,
    // so the old files can be copied off first.
    let old = at(OLD);
    let entry = internal::partition(root);
    if let (Some(old), Some(entry)) = (old, entry) {
        let device = format!(
            "/dev/{}",
            entry.file_name().unwrap_or_default().to_string_lossy()
        );
        if old.source == device
            && old.filesystem == "vfat"
            && setup_status(root).get("state").and_then(Value::as_str) == Some("Waiting")
        {
            result.push(view(old, OLD, Value::Null, "vfat", "ro"));
        }
    }
    // RELEASE-49: while the radio firmware is missing, one empty folder of Y2DATA is shown as
    // "Firmware" for the owner's system.img copy. With Internal Storage the copy goes into its
    // Firmware folder instead (#39: Y2DATA is too small for an unpacked image).
    let data = at("/data");
    if !radiofw::present(root) && radiofw::internal(root) {
        // Internal Storage → Firmware, before the computer lists it.
        let _ = radiofw::inbox(root);
    } else if let Some(data) = data.filter(|d| !d.has_option("ro") && !radiofw::present(root)) {
        if radiofw::inbox(root).is_ok() {
            result.push(view(
                data,
                radiofw::INBOX,
                Value::Null,
                &data.filesystem,
                "rw",
            ));
        }
    }
    result
}

/// `identity(view)`.
fn identity(view: &Object) -> Value {
    json!([
        view.get("mount_id"),
        view.get("device_id"),
        view.get("uuid"),
        view.get("access")
    ])
}

/// `str(value)` of a record's field (`'%s'`), a dict's keys sorted as the records are written.
fn py_text(value: Option<&Value>) -> String {
    y2p_core::json::sorted(value.cloned().unwrap_or_default()).py_str()
}

/// `expectations(views)`: the mount identity the responder must find when it pins a root.
fn expectations(views: &[Object]) -> String {
    views
        .iter()
        .filter(|v| {
            v.get("device_id").is_some_and(py::truthy)
                && !v.get("mount_id").is_none_or(Value::is_null)
        })
        .map(|v| {
            format!(
                "{} {} {}\n",
                py_text(v.get("path")),
                py_text(v.get("device_id")),
                py_text(v.get("mount_id"))
            )
        })
        .collect()
}

/// `config(views)`: only fixed paths and labels reach the responder parser. Both views are always
/// declared, in fixed slots (stable MTP storage IDs); a view that is not currently available is
/// declared `notmounted` and joins or leaves the live session later without a USB re-enumeration.
fn config(views: &[Object]) -> Result<String, Failure> {
    let mut lines: Vec<String> = [
        "loop_on_disconnect 1",
        "manufacturer \"Y2Linux\"",
        "product \"Y2\"",
        "serial \"Y2LINUX-DEV-01\"",
        "interface \"MTP\"",
        "firmware_version \"Platform 1.0\"",
        "mtp_extensions \"\"",
        "usb_class 0x6",
        "usb_subclass 0x1",
        "usb_protocol 0x1",
        "perceived_device_type 0x2",
        "usb_functionfs_mode 1",
        "usb_dev_path \"/dev/ffs-mtp/ep0\"",
        "usb_epin_path \"/dev/ffs-mtp/ep1\"",
        "usb_epout_path \"/dev/ffs-mtp/ep2\"",
        "usb_epint_path \"/dev/ffs-mtp/ep3\"",
        "usb_max_packet_size 0x200",
        "usb_max_rd_buffer_size 0x10000",
        "usb_max_wr_buffer_size 0x10000",
        "read_buffer_cache_size 0x100000",
        "sync_when_close 0",
        "no_inotify 1",
        "umask 077",
    ]
    .map(str::to_owned)
    .to_vec();
    let mut present: Vec<(&str, &Object)> = Vec::new();
    for v in views {
        let path = text(v, "path").unwrap_or_default();
        let named = name_of(path).is_some_and(|n| text(v, "name") == Some(n));
        if !named || !matches!(text(v, "access"), Some("ro" | "rw")) {
            return Err(Failure::value("unsafe_media_view"));
        }
        match present.iter_mut().find(|(p, _)| *p == path) {
            Some(entry) => entry.1 = v,
            None => present.push((path, v)),
        }
    }
    for (path, name) in NAMES {
        let v = present.iter().find(|(p, _)| *p == path).map(|(_, v)| *v);
        let mut options = vec![if v.is_some_and(|v| text(v, "access") == Some("ro")) {
            "ro"
        } else {
            "rw"
        }];
        if path == "/media/sd" {
            options.push("removable");
        }
        if v.is_none() {
            options.push("notmounted");
        }
        lines.push(format!(
            "storage \"{path}\" \"{name}\" \"{}\"",
            options.join(",")
        ));
    }
    Ok(lines.join("\n") + "\n")
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
    views: Option<Vec<Object>>,
    bound: bool,
    failures: i64,
    started: f64,
    previous_cable: Option<String>,
    seen_generation: i64,
    refresh_generation: u64,
    last_record: Option<Value>,
    signature: Option<Signature>,
    cached_views: Vec<Object>,
    terminal: bool,
    /// path -> identity, in insertion order (a dict).
    mounted: Vec<(String, Value)>,
    sd_window: f64,
    /// What starts the responder and its IPC commands.
    launch: Launch,
}

/// A symlink's OSError: Python names both paths.
fn symlink(target: &Path, link: &Path) -> Result<(), Failure> {
    std::os::unix::fs::symlink(target, link).map_err(|error| {
        let (text, class) = py::os_error(&error, Some(&target.to_string_lossy()));
        Failure::Os {
            text: format!("{text} -> {}", py::repr(&link.to_string_lossy())),
            class,
        }
    })
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
            launch: process::launcher(),
        }
    }

    fn write(&self, relative: &str, value: &str) -> Result<(), Failure> {
        write_text(
            &self.root.path(&format!("{GADGET}/{relative}")),
            &format!("{value}\n"),
        )
    }

    fn mkdir(&self, relative: &str) -> Result<(), Failure> {
        mkdir_parents(&self.root.path(&format!("{GADGET}/{relative}")), 0o777)
    }

    fn child_running(&mut self) -> bool {
        self.child.as_mut().is_some_and(Child::running)
    }

    /// `stop()`: unbind first (endpoint reads return, then join/abort staged uploads), then end the
    /// responder. An unbind error is raised after the responder is stopped.
    pub fn stop(&mut self) -> Result<(), Failure> {
        let udc = self.root.path(&format!("{GADGET}/UDC"));
        let mut unbind_error = None;
        let bound = self.bound
            || self
                .root
                .read(&format!("{GADGET}/UDC"))
                .is_some_and(|t| !t.is_empty());
        if udc.exists() && bound {
            if let Err(error) = self.write("UDC", "") {
                unbind_error = Some(error);
            }
        }
        self.bound = false;
        if let Some(mut child) = self.child.take() {
            if child.running() {
                child.terminate();
                if !child.wait(2.0) {
                    child.kill();
                    if !child.wait(1.0) {
                        // subprocess.TimeoutExpired: not an OSError, the daemon ends (a traceback).
                        return Err(Failure::crash(
                            "subprocess.TimeoutExpired",
                            "Command '['/usr/sbin/umtprd']' timed out after 1.0 seconds",
                        ));
                    }
                }
            }
        }
        unbind_error.map_or(Ok(()), Err)
    }

    fn setup(&mut self, mode: &'static str, views: Vec<Object>) -> Result<(), Failure> {
        let functions = selection(mode)?;
        self.stop()?;
        let root = self.root.path(GADGET);
        if !self.root.path("/sys/kernel/config/usb_gadget").exists() {
            let answer = y2p_core::command::run(
                &self.root,
                &["mount", "-t", "configfs", "none", "/sys/kernel/config"],
                y2p_core::command::TIMEOUT,
            );
            if !answer.ok {
                return Err(Failure::os_message("configfs_mount_failed"));
            }
        }
        self.mkdir("")?;
        // Keep the existing development VID/PID identity (owner-local build), power request and
        // serial. No fabricated production USB allocation.
        for (name, value) in [
            ("idVendor", "0x0525"),
            ("idProduct", "0xa4aa"),
            ("bcdUSB", "0x0200"),
            ("bcdDevice", "0x0100"),
            ("bDeviceClass", "0"),
            ("bDeviceSubClass", "0"),
            ("bDeviceProtocol", "0"),
        ] {
            self.write(name, value)?;
        }
        self.mkdir("configs/c.1")?;
        self.mkdir("os_desc")?;
        self.write("os_desc/use", if mode == "mtp" { "1" } else { "0" })?;
        self.write("os_desc/b_vendor_code", "0x20")?;
        self.write("os_desc/qw_sign", "MSFT100")?;
        let os_link = root.join("os_desc/c.1");
        if os_link
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())
        {
            fs::remove_file(&os_link).map_err(|e| Failure::os(e, &os_link))?;
        }
        if mode == "mtp" {
            symlink(&root.join("configs/c.1"), &os_link)?;
        }
        self.mkdir("strings/0x409")?;
        for (name, value) in [
            ("manufacturer", "Y2Linux"),
            ("product", "Y2"),
            ("serialnumber", "Y2LINUX-DEV-01"),
        ] {
            self.write(&format!("strings/0x409/{name}"), value)?;
        }
        self.mkdir("configs/c.1/strings/0x409")?;
        self.write(
            "configs/c.1/strings/0x409/configuration",
            if mode == "mtp" {
                "Music Transfer"
            } else {
                "Developer"
            },
        )?;
        // The charger grants the validated SDP ceiling (450 mA) only for a configured 500 mA
        // bus-powered request (kernel/platform/source-policy.h). A self-powered, 0 mA configuration
        // made the allocation 0 and left the PC port "Not charging".
        self.write("configs/c.1/MaxPower", "500")?;
        self.write("configs/c.1/bmAttributes", "0x80")?;
        let config_dir = root.join("configs/c.1");
        for entry in fs::read_dir(&config_dir).map_err(|e| Failure::os(e, &config_dir))? {
            let link = entry.map_err(|e| Failure::os(e, &config_dir))?.path();
            if link
                .symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink())
            {
                fs::remove_file(&link).map_err(|e| Failure::os(e, &link))?;
            }
        }
        for function in functions {
            self.mkdir(&format!("functions/{function}"))?;
            if *function == "ecm.usb0" {
                self.write("functions/ecm.usb0/host_addr", "02:42:00:00:00:02")?;
                self.write("functions/ecm.usb0/dev_addr", "02:42:00:00:00:01")?;
            }
            symlink(
                &root.join("functions").join(function),
                &root.join("configs/c.1").join(function),
            )?;
        }
        // FFS must be mounted and have its descriptors written BEFORE UDC bind.
        if mode == "mtp" {
            mkdir_parents(&self.root.path("/dev/ffs-mtp"), 0o777)?;
            let mounted = mounts(&self.root)
                .iter()
                .any(|m| m.path == "/dev/ffs-mtp" && m.filesystem == "functionfs");
            if !mounted {
                let result = y2p_core::command::run(
                    &self.root,
                    &["mount", "-t", "functionfs", "mtp", "/dev/ffs-mtp"],
                    y2p_core::command::TIMEOUT,
                );
                if !result.ok {
                    return Err(Failure::os_message("functionfs_mount_failed"));
                }
            }
            write_text(&self.root.path("/run/y2/umtprd.conf"), &config(&views)?)?;
            write_text(
                &self.root.path("/run/y2/mtp-expect.txt"),
                &expectations(&views),
            )?;
            self.child = Some((self.launch)(
                &self.root,
                &["/usr/sbin/umtprd", "-conf", "/run/y2/umtprd.conf"],
                Start::Responder,
            )?);
        }
        self.mounted = if mode == "mtp" {
            let mut mounted: Vec<(String, Value)> = Vec::new();
            for v in &views {
                let path = text(v, "path").unwrap_or_default().to_owned();
                match mounted.iter_mut().find(|(p, _)| *p == path) {
                    Some(entry) => entry.1 = identity(v),
                    None => mounted.push((path, identity(v))),
                }
            }
            mounted
        } else {
            Vec::new()
        };
        self.mode = Some(mode);
        self.views = Some(views);
        self.started = clock::monotonic();
        Ok(())
    }

    /// `command(message)`: one IPC command to the running responder.
    fn command(&mut self, message: &str) -> bool {
        let argv = ["/usr/sbin/umtprd", &format!("-cmd:{message}")];
        (self.launch)(&self.root, &argv, Start::Command)
            .is_ok_and(|child| child.succeeds_within(3.0))
    }

    /// `pinned()`: the roots the running responder has pinned, if its record is this responder's.
    fn pinned(&mut self) -> Option<Vec<Value>> {
        let state = self.root.json_object("/run/y2/mtp-roots.json");
        let pid = self.child.as_ref().map(Child::pid)?;
        if !state.get("pid").unwrap_or(&Value::Null).py_eq(&json!(pid)) {
            return None;
        }
        state.get("roots").and_then(Value::as_array).cloned()
    }

    fn await_root(&mut self, path: &str, present: bool) -> bool {
        let deadline = clock::monotonic() + 3.0;
        while clock::monotonic() < deadline {
            if let Some(roots) = self.pinned() {
                if roots.iter().any(|r| r.as_str() == Some(path)) == present {
                    return true;
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
        false
    }

    /// `reconcile(views)`: change the live MTP view without touching the USB gadget.
    fn reconcile(&mut self, views: Vec<Object>) -> Result<(), Failure> {
        let mut desired: Vec<(String, &Object)> = Vec::new();
        for v in &views {
            let path = text(v, "path").unwrap_or_default().to_owned();
            match desired.iter_mut().find(|(p, _)| *p == path) {
                Some(entry) => entry.1 = v,
                None => desired.push((path, v)),
            }
        }
        for (path, mounted) in self.mounted.clone() {
            let view = desired.iter().find(|(p, _)| *p == path).map(|(_, v)| *v);
            if view.is_some_and(|v| mounted.py_eq(&identity(v))) {
                continue;
            }
            // Leaving view: the responder drops its pinned root and open files before the platform
            // may unmount or replace the filesystem.
            let name = name_of(&path).unwrap_or_default();
            if !(self.command(&format!("unmount:{name}")) && self.await_root(&path, false)) {
                return Err(Failure::os_message("responder_unmount_timeout"));
            }
            self.mounted.retain(|(p, _)| *p != path);
        }
        write_text(
            &self.root.path("/run/y2/mtp-expect.txt"),
            &expectations(&views),
        )?;
        for (path, view) in &desired {
            if self.mounted.iter().any(|(p, _)| p == path) {
                continue;
            }
            let verb = if text(view, "access") == Some("ro") {
                "mountro:"
            } else {
                "mount:"
            };
            let name = name_of(path).unwrap_or_default();
            if !(self.command(&format!("{verb}{name}")) && self.await_root(path, true)) {
                return Err(Failure::os_message("responder_mount_timeout"));
            }
            self.mounted.push((path.clone(), identity(view)));
        }
        self.views = Some(views);
        self.refresh_generation = clock::monotonic_ns();
        Ok(())
    }

    fn record(
        &mut self,
        state: &str,
        reason: Option<&str>,
        cable: &Option<String>,
        activity: Option<&Object>,
    ) -> Result<Value, Failure> {
        let functions = self.mode.map_or(Ok(&[][..]), selection)?;
        let value = json!({
            "schema": SCHEMA,
            "boot_id": self.root.boot_id(),
            "state": state,
            "reason": reason,
            "mode": self.mode,
            "cable": cable,
            "functions": functions,
            "views": self.views.clone().unwrap_or_default(),
            "filesystem_owner": "linux",
            "block_export": false,
            "refresh_generation": self.refresh_generation,
            "active": activity.and_then(|a| a.get("active")).is_some_and(py::truthy),
            "error": activity.and_then(|a| a.get("error")).cloned().unwrap_or(Value::Null),
            "restarts": self.failures,
        });
        if !self.last_record.as_ref().is_some_and(|l| l.py_eq(&value)) {
            atomic_json(&self.root.path("/run/y2/usb-transfer.json"), &value, false)?;
            self.last_record = Some(value.clone());
        }
        Ok(value)
    }

    fn sd_writable(&mut self, cable: &Option<String>) -> Result<(), Failure> {
        let host = matches!(cable.as_deref(), Some("configured" | "suspended"));
        let now = clock::monotonic();
        let result = if host && now - self.sd_window >= SD_WINDOW_RENEW {
            media::write_window(&self.root, "usb", SD_WINDOW_SECONDS, false).map(|answer| {
                if answer.get("holders").is_some_and(py::truthy) {
                    self.sd_window = now;
                }
            })
        } else if !host && self.sd_window != 0.0 {
            media::write_window(&self.root, "usb", 60, true).map(|_| self.sd_window = 0.0)
        } else {
            Ok(())
        };
        match result {
            Err(error @ Failure::Crash { .. }) => Err(error),
            _ => Ok(()),
        }
    }

    /// One pass: the record `/run/y2/usb-transfer.json` shows (written only when it changed).
    pub fn tick(&mut self) -> Result<Value, Failure> {
        mkdir_parents(&self.root.path("/run/y2"), 0o777)?;
        let udcs = self.root.glob("/sys/class/udc/*");
        if udcs.len() != 1 {
            return self.record("Unavailable", Some("no_unique_udc"), &None, None);
        }
        let udc = &udcs[0];
        let cable = read(&udc.join("state"));
        self.sd_writable(&cable)?;
        if self.terminal {
            return self.record("Failed", Some("responder_restart_budget"), &cable, None);
        }
        let requested = match self
            .root
            .json_object("/data/system/platform/usb-mode.json")
            .get("mode")
        {
            None => json!("mtp"),
            // `mode not in FUNCTIONS` of a list or dict: TypeError, the daemon ends.
            Some(Value::Array(_)) => {
                return Err(Failure::crash("TypeError", "unhashable type: 'list'"))
            }
            Some(Value::Object(_)) => {
                return Err(Failure::crash("TypeError", "unhashable type: 'dict'"))
            }
            Some(value) => value.clone(),
        };
        let name = udc
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match self.attempt(requested, &cable, &name) {
            Err(error @ (Failure::Os { .. } | Failure::Value(_))) => {
                match self.stop() {
                    Err(error @ Failure::Crash { .. }) => return Err(error),
                    Err(_) => self.terminal = true,
                    Ok(()) => {}
                }
                if self.started != 0.0 && clock::monotonic() - self.started > STABLE_SECONDS {
                    self.failures = 0;
                }
                self.failures += 1;
                self.terminal = self.terminal || self.failures >= 5;
                if self.failures < 5 {
                    self.mode = None;
                }
                self.record("Failed", Some(&error.message()), &cable, None)
            }
            other => other,
        }
    }

    /// The `try:` part of usb.py's tick.
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
            cards: root
                .glob("/sys/class/block/mmcblk*")
                .iter()
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
                self.refresh_generation = clock::monotonic_ns();
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
            if clock::monotonic() - self.started > STABLE_SECONDS {
                self.failures = 0;
            }
            self.failures += 1;
            if self.failures >= 5 {
                self.terminal = true;
                return self.record("Failed", Some("responder_restart_budget"), cable, None);
            }
            self.mode = None;
            return self.record("Starting", Some("responder_restarting"), cable, None);
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
                if clock::monotonic() - self.started > 15.0 {
                    return Err(Failure::os_message("responder_descriptor_timeout"));
                }
                return self.record("Starting", Some("waiting_for_descriptors"), cable, None);
            }
            self.write("UDC", udc)?;
            self.bound = true;
        }
        let mut activity = self.root.json_object("/run/y2/mtp-events.json");
        let pid = self.child.as_ref().map(Child::pid);
        if pid.is_none_or(|pid| {
            !activity
                .get("pid")
                .unwrap_or(&Value::Null)
                .py_eq(&json!(pid))
        }) {
            activity = Object::new();
        }
        // isinstance(generation, int): a bool is an int too.
        let generation = match activity.get("generation") {
            None => Some(0),
            Some(Value::Bool(b)) => Some(i64::from(*b)),
            Some(value) => value.as_i64(),
        };
        if let Some(generation) = generation.filter(|g| *g > self.seen_generation) {
            self.seen_generation = generation;
            self.refresh_generation = clock::monotonic_ns();
        }
        let hosted = matches!(cable.as_deref(), Some("configured" | "suspended"));
        if self.previous_cable.as_deref() == Some("configured") && !hosted {
            self.refresh_generation = clock::monotonic_ns();
        }
        self.previous_cable = cable.clone();
        let mut state = match cable.as_deref() {
            Some("not attached") => "Disconnected",
            Some("configured") => "Ready",
            _ => "Starting",
        };
        if state == "Ready" && activity.get("active").is_some_and(py::truthy) {
            state = "Copying";
        }
        if state == "Ready"
            && self.mode == Some("mtp")
            && self.views.as_ref().is_none_or(Vec::is_empty)
        {
            state = "Unavailable";
        }
        self.record(state, None, cable, Some(&activity))
    }
}

#[cfg(test)]
#[path = "usb_tests.rs"]
mod usb_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn view_of(path: &str, access: &str) -> Object {
        json!({"path": path, "name": name_of(path), "mount_id": 31, "device_id": "179:8",
               "uuid": null, "access": access})
        .as_object()
        .cloned()
        .unwrap()
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
        odd.insert("name".into(), json!("Card"));
        assert_eq!(config(&[odd]).err().unwrap().message(), "unsafe_media_view");
    }
}
