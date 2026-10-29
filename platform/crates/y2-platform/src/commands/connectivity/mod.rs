//! Group `connectivity`: `connectivity-daemon` (the radio helpers' supervisor), `wifi-power-policy`
//! and `bluetooth-power on|off`, with what the service daemon runs in process (supplicant events, the
//! F52 password latch and self-heal, the DNS probe, the Wi-Fi journal). Callers in the production
//! overlay: `usr/libexec/y2/connectivity` (`exec y2-platform connectivity-daemon`),
//! `usr/libexec/y2/wpa-event` (`wifi-event`), `usr/sbin/y2-radio` (`bluetooth-power on|off`).
// SPDX-License-Identifier: GPL-2.0-only
#[cfg(test)]
mod bluetooth_tests;
pub mod bt_control;
#[cfg(test)]
pub(crate) mod crosscheck;
pub mod daemon;
pub mod observe_wifi;
pub mod radio_policy;
pub(crate) mod record;
#[cfg(test)]
mod scenarios;
#[cfg(test)]
pub(crate) mod steps;
pub mod wifi;

use super::Command;

/// The Rust implementation of these exact arguments, if this group has one.
pub fn route(args: &[&str]) -> Option<Command> {
    Some(match args {
        ["connectivity-daemon"] => Box::new(daemon::serve),
        ["wifi-power-policy"] => Box::new(radio_policy::command),
        ["bluetooth-power", value] if *value == "on" || *value == "off" => {
            let value = value.to_string();
            Box::new(move |root| bt_control::command(root, &value))
        }
        _ => return None,
    })
}
