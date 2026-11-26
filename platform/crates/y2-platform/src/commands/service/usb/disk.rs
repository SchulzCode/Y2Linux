//! The SD card as a USB drive (docs/planning/SD-DISK-MODE-PLAN.md): the mass-storage function's one
//! removable LUN, and the pass that lends the card to a computer and takes it back
//! (`storage::media::lend` owns the record and the mount).
// SPDX-License-Identifier: GPL-2.0-only
use super::{Failure, UsbService, GADGET};
use crate::commands::storage::media::lend::{self, Lend, Stage};
use serde::Serialize;
use y2p_core::mounts::mounts_at;

/// The mass-storage function in the MTP composition (between MTP and the network function).
pub(super) const FUNCTION: &str = "mass_storage.0";
const LUN: &str = "functions/mass_storage.0/lun.0";
/// A card the player still holds open this long after the lend began stays with the player.
const BUSY_SECONDS: f64 = 30.0;

/// `usb-transfer.json`'s `sd_disk`: the owner's switch and what became of the card.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct DiskRecord {
    pub enabled: bool,
    /// Off, Idle, Lending, Lent or Kept (the player keeps it until the cable is plugged in again).
    pub state: &'static str,
    pub reason: Option<&'static str>,
}

impl Default for DiskRecord {
    fn default() -> Self {
        DiskRecord {
            enabled: false,
            state: "Off",
            reason: None,
        }
    }
}

/// What the pass knows of the connection.
struct Link {
    /// A computer configured the gadget.
    hosted: bool,
    /// The cable is in (an unreadable controller state counts as in: nothing is changed on a guess).
    attached: bool,
    /// The bound MTP composition has the drive.
    ready: bool,
    enabled: bool,
}

impl UsbService {
    fn lun(&self, name: &str) -> String {
        format!("{LUN}/{name}")
    }

    /// The LUN as the gadget's setup leaves it: removable (the computer may eject it), writable, empty.
    pub(super) fn setup_lun(&self) -> Result<(), Failure> {
        self.eject();
        for (name, value) in [
            ("removable", "1"),
            ("ro", "0"),
            ("cdrom", "0"),
            ("nofua", "0"),
            // Vendor (8) and product (16): what the computer names the drive.
            ("inquiry_string", "Y2Linux SD Card"),
        ] {
            self.write(&self.lun(name), value)?;
        }
        Ok(())
    }

    /// The card the computer has (none: the LUN is empty, or the computer ejected it).
    fn backing(&self) -> Option<String> {
        self.root
            .read(&format!("{GADGET}/{}", self.lun("file")))
            .filter(|file| !file.is_empty())
    }

    /// Empty the LUN even while the computer holds the medium (`forced_eject` ignores its removal lock).
    fn eject(&self) {
        if self.backing().is_some() {
            // A LUN that cannot be emptied fails the next load, which gives the card back.
            let _ = self.write(&self.lun("forced_eject"), "1");
        }
    }

    /// The pass after a service start: a lend left by the previous service is ended and the card admitted.
    fn recover(&mut self) -> Result<(), Failure> {
        if lend::on_computer(&self.root) {
            let unmounted = lend::current(&self.root).is_none_or(|l| l.stage == Stage::Lent);
            // The previous service's LUN may still hold the card (none: nothing to empty).
            self.eject();
            lend::end(&self.root, unmounted)?;
        }
        Ok(())
    }

    /// One pass of the lend, before the views are read (a new lend takes the SD view away this pass).
    pub(super) fn lend_pass(&mut self, cable: Option<&str>) -> Result<(), Failure> {
        if !self.lend_recovered {
            self.lend_recovered = true;
            self.recover()?;
        }
        let link = Link {
            hosted: cable == Some("configured"),
            attached: cable != Some("not attached"),
            ready: self.disk && self.bound && self.mode == Some("mtp"),
            enabled: lend::enabled(&self.root),
        };
        if !link.attached {
            self.kept = None;
        }
        let reason = match lend::current(&self.root) {
            None if lend::on_computer(&self.root) => {
                // An unreadable record: the card is admitted again rather than left out.
                self.eject();
                lend::end(&self.root, true)?;
                Some("record_unreadable")
            }
            None => self.offer(&link)?,
            Some(lend) if lend.stage == Stage::Lending => self.release(&link, &lend)?,
            Some(lend) => self.lent(&link, &lend)?,
        };
        self.disk_record = self.describe(&link, reason);
        Ok(())
    }

    /// No lend: start one when a computer configured the drive; else why not.
    fn offer(&mut self, link: &Link) -> Result<Option<&'static str>, Failure> {
        if !(link.enabled && link.ready && link.hosted) || self.kept.is_some() {
            return Ok(None);
        }
        match lend::lendable(&self.root) {
            Ok(device) => {
                lend::begin(&self.root, device)?;
                Ok(None)
            }
            Err(reason) => Ok(Some(reason)),
        }
    }

    /// Lending: the player lets go of the card, then the computer gets it.
    fn release(&mut self, link: &Link, lend: &Lend) -> Result<Option<&'static str>, Failure> {
        if !(link.attached && link.enabled && link.ready) {
            lend::end(&self.root, false)?;
            return Ok(None);
        }
        // The responder drops its pinned root first (the views without the card, this pass).
        let pinned = self.mounted.iter().any(|(path, _)| path == "/media/sd");
        if !pinned && lend::detach(&self.root, lend)? {
            if self.write(&self.lun("file"), &lend.device).is_err() {
                lend::end(&self.root, true)?;
                self.kept = Some("drive_failed");
            }
            return Ok(None);
        }
        if self.root.clock().monotonic() - lend.since >= BUSY_SECONDS {
            // A song or a scan still reads the card: it stays with the player for this connection.
            lend::end(&self.root, false)?;
            self.kept = Some("sd_in_use");
        }
        Ok(None)
    }

    /// Lent: back to the player once the computer ejected it, the cable is out, the card is gone or the
    /// switch is off.
    fn lent(&mut self, link: &Link, lend: &Lend) -> Result<Option<&'static str>, Failure> {
        let ejected = self.backing().is_none();
        let gone = !self.root.path(&lend.device).exists();
        if link.attached && link.enabled && link.ready && !ejected && !gone {
            return Ok(None);
        }
        self.eject();
        lend::end(&self.root, true)?;
        if ejected && link.attached {
            self.kept = Some("computer_ejected");
        }
        Ok(None)
    }

    /// A step of the lend failed (a record that cannot be written, another card under the mount): the card
    /// goes back to the player, admitted again when it is no longer mounted, and stays there this connection.
    pub(super) fn give_up_lend(&mut self) {
        self.eject();
        let unmounted = mounts_at(&self.root, "/media/sd").is_empty();
        let _ = lend::end(&self.root, unmounted);
        self.kept = Some("lend_failed");
        self.disk_record = DiskRecord {
            enabled: lend::enabled(&self.root),
            state: "Kept",
            reason: self.kept,
        };
    }

    fn describe(&self, link: &Link, reason: Option<&'static str>) -> DiskRecord {
        let stage = lend::current(&self.root).map(|l| l.stage);
        let (state, reason) = match (stage, self.kept) {
            (Some(Stage::Lending), _) => ("Lending", None),
            (Some(Stage::Lent), _) => ("Lent", None),
            (None, Some(kept)) => ("Kept", Some(kept)),
            (None, None) if !link.enabled => ("Off", None),
            (None, None) if !link.hosted => ("Idle", Some("no_computer")),
            (None, None) if link.enabled && self.disk_failed => ("Kept", Some("drive_unavailable")),
            // Switched on while connected: the drive comes with the next connection.
            (None, None) if !self.disk => ("Idle", Some("reconnect")),
            (None, None) if !link.ready => ("Idle", Some("starting")),
            (None, None) => ("Idle", reason),
        };
        DiskRecord {
            enabled: link.enabled,
            state,
            reason,
        }
    }

    /// A lend is under way: the service runs its passes every second until the card moved.
    pub fn lending(&self) -> bool {
        self.disk_record.state == "Lending"
    }
}
