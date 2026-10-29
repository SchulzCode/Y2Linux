//! Group `connectivity` of the port (docs/planning/PLATFORM-RUST-PORT-PLAN.md, "Remaining port groups"):
//! `connectivity-daemon` (connectivity.py), `wifi-power-policy` (radio_policy.wifi_policy) and
//! `bluetooth-power on|off` (bt_control.py), with the rest of network.py the service daemon runs in
//! process (supplicant events, the F52 password latch and self-heal, the DNS probe, the Wi-Fi journal)
//! and bluetooth.normalize for `status bluetooth`. Callers in the production overlay:
//! `usr/libexec/y2/connectivity` (`exec y2-platform connectivity-daemon`), `usr/libexec/y2/wpa-event`
//! (`wifi-event`), `usr/sbin/y2-radio` (`bluetooth-power on|off`).
// SPDX-License-Identifier: GPL-2.0-only
// The service daemon (EventMonitor, WifiRecovery, the DNS probe, wifi_policy with activity) and
// `status bluetooth` (normalize) call these in process once their groups are ported.
#[allow(dead_code)]
pub mod bluetooth;
pub mod bt_control;
#[cfg(test)]
pub(crate) mod crosscheck;
pub mod daemon;
#[allow(dead_code)]
pub mod observe_wifi;
pub mod radio_policy;
#[cfg(test)]
mod scenarios;
#[cfg(test)]
pub(crate) mod steps;
#[allow(dead_code)]
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
