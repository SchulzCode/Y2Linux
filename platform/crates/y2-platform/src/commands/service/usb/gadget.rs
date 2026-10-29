//! The configfs USB gadget: its setup for a mode (with the responder for MTP) and its stop.
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    config, expectations, fs, identity, mkdir_parents, mounts, py, selection, text, write_text,
    Failure, Object, Path, Start, UsbService, Value, GADGET,
};

/// A symlink whose error names both paths (`[Errno 17] File exists: 'a' -> 'b'`).
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
    pub(super) fn write(&self, relative: &str, value: &str) -> Result<(), Failure> {
        write_text(
            &self.root.path(&format!("{GADGET}/{relative}")),
            &format!("{value}\n"),
        )
    }

    pub(super) fn mkdir(&self, relative: &str) -> Result<(), Failure> {
        mkdir_parents(&self.root.path(&format!("{GADGET}/{relative}")), 0o777)
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
                        // Not even SIGKILL ended it within a second (a process stuck in the
                        // kernel): the service gives up its USB part (tick) or ends (serve).
                        return Err(Failure::os_message("responder_stop_timeout"));
                    }
                }
            }
        }
        unbind_error.map_or(Ok(()), Err)
    }

    pub(super) fn setup(&mut self, mode: &'static str, views: Vec<Object>) -> Result<(), Failure> {
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
        self.started = self.root.clock().monotonic();
        Ok(())
    }
}
