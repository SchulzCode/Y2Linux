//! `y2-platform usb-mode mtp|developer`: queue the USB gadget mode.
// SPDX-License-Identifier: GPL-2.0-only
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
