//! `y2-platform usb-mode mtp|developer`: queue the USB gadget mode; `usb-sd-disk on|off`: the SD card as a
//! USB drive.
// SPDX-License-Identifier: GPL-2.0-only
use crate::commands::storage::{media::lend, sys::wake_service};
use crate::reply::{atomic_json, Outcome, Reply};
use serde::Serialize;
use y2p_core::Root;

/// /data/system/platform/usb-mode.json: the mode the service daemon sets up next.
#[derive(Serialize)]
struct Request<'a> {
    mode: &'a str,
}

/// What the command prints, in this key order.
#[derive(Serialize)]
struct Queued<'a> {
    state: &'static str,
    mode: &'a str,
    apply: &'static str,
}

/// Queue the gadget mode the service daemon sets up next (its USB service applies it only while no
/// host is attached). The gadget must request MaxPower 500 bus-powered when it is set up, or a PC port
/// charges at 0 mA (SDP 500 mA); that is the service's part, not this command's.
pub fn request_mode(root: &Root, mode: &str) -> Outcome {
    // Avoid disconnecting a host with an open session. Changes take effect after unplug/replug;
    // advanced users may choose before attaching the cable. The directory is not created: without
    // /data/system/platform (no verified data volume) the request fails.
    atomic_json(
        &root.path("/data/system/platform/usb-mode.json"),
        &Request { mode },
        true,
    )?;
    Reply::unsorted(
        &Queued {
            state: "Queued",
            mode,
            apply: "when_disconnected",
        },
        0,
    )
}

/// What `usb-sd-disk` prints.
#[derive(Serialize)]
struct Switched {
    enabled: bool,
    apply: &'static str,
}

/// The owner's switch: with a computer connected it applies from the next connection (the computer keeps
/// the composition it enumerated). The service is asked to report it at once.
pub fn sd_disk(root: &Root, enabled: bool) -> Outcome {
    atomic_json(&root.path(lend::SETTING), &lend::Setting { enabled }, true)?;
    wake_service(root);
    Reply::unsorted(
        &Switched {
            enabled,
            apply: "next_connection",
        },
        0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Fixture;

    #[test]
    fn the_sd_disk_switch_is_saved_where_the_service_reads_it() {
        let fixture = Fixture::new();
        fixture.mkdir("/data/system/platform");
        assert!(!lend::enabled(&fixture.root));
        sd_disk(&fixture.root, true).unwrap();
        assert!(lend::enabled(&fixture.root));
        sd_disk(&fixture.root, false).unwrap();
        assert!(!lend::enabled(&fixture.root));
    }
}
