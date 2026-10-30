//! The running responder's IPC: a view joins or leaves the live MTP session without a USB
//! re-enumeration, acknowledged by the roots it pins (`mtp-roots.json`).
// SPDX-License-Identifier: GPL-2.0-only
use super::{
    expectations, name_of, thread, write_text, Child, Duration, Failure, Start, UsbService, View,
};
use crate::commands::storage::sys::MtpRoots;

impl UsbService {
    /// `command(message)`: one IPC command to the running responder.
    pub(super) fn command(&mut self, message: &str) -> bool {
        let argv = ["/usr/sbin/umtprd", &format!("-cmd:{message}")];
        (self.launch)(&self.root, &argv, Start::Command)
            .is_ok_and(|child| child.succeeds_within(3.0))
    }

    /// `pinned()`: the roots the running responder has pinned, if its record is this responder's.
    pub(super) fn pinned(&mut self) -> Option<Vec<String>> {
        let state = MtpRoots::read(&self.root);
        let pid = self.child.as_ref().map(Child::pid)?;
        if state.pid != Some(i64::from(pid)) {
            return None;
        }
        state.roots
    }

    pub(super) fn await_root(&mut self, path: &str, present: bool) -> bool {
        let deadline = self.root.clock().monotonic() + 3.0;
        while self.root.clock().monotonic() < deadline {
            if let Some(roots) = self.pinned() {
                if roots.iter().any(|r| r == path) == present {
                    return true;
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
        false
    }

    /// `reconcile(views)`: change the live MTP view without touching the USB gadget.
    pub(super) fn reconcile(&mut self, views: Vec<View>) -> Result<(), Failure> {
        let mut desired: Vec<(String, &View)> = Vec::new();
        for v in &views {
            let path = v.path.to_owned();
            match desired.iter_mut().find(|(p, _)| *p == path) {
                Some(entry) => entry.1 = v,
                None => desired.push((path, v)),
            }
        }
        for (path, mounted) in self.mounted.clone() {
            let view = desired.iter().find(|(p, _)| *p == path).map(|(_, v)| *v);
            if view.is_some_and(|v| mounted == v.identity()) {
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
            let verb = if view.access == "ro" {
                "mountro:"
            } else {
                "mount:"
            };
            let name = name_of(path).unwrap_or_default();
            if !(self.command(&format!("{verb}{name}")) && self.await_root(path, true)) {
                return Err(Failure::os_message("responder_mount_timeout"));
            }
            self.mounted.push((path.clone(), view.identity()));
        }
        self.views = Some(views);
        self.refresh_generation = self.root.clock().monotonic_ns();
        Ok(())
    }
}
