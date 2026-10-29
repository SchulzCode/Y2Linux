//! `usb.request_mode`: `y2-platform usb-mode mtp|developer`.
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::{atomic_json, Outcome, Reply};
use serde_json::json;
use y2p_core::{
    json::{Object, ObjectExt},
    Root,
};

/// Queue the gadget mode the service daemon sets up next (USBService.tick applies it only while no
/// host is attached). The gadget must request MaxPower 500 bus-powered when it is set up, or a PC port
/// charges at 0 mA (SDP 500 mA, usb.py); that is the service's part, not this command's.
pub fn request_mode(root: &Root, mode: &str) -> Outcome {
    // Avoid disconnecting a host with an open session. Changes take effect after unplug/replug;
    // advanced users may choose before attaching the cable. No private_directory: a missing
    // /data/system/platform is the record's OSError.
    atomic_json(
        &root.path("/data/system/platform/usb-mode.json"),
        &json!({"mode": mode}),
        true,
    )?;
    Reply::unsorted(
        &Object::new()
            .with("state", "Queued")
            .with("mode", mode)
            .with("apply", "when_disconnected"),
        0,
    )
}
