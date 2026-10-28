//! The subcommands implemented in Rust, each for exactly the argument forms listed in `route`; every
//! other form (help, abbreviations, invalid choices, extra arguments) is argparse's and goes to Python.
// SPDX-License-Identifier: GPL-2.0-only
pub mod boot;
pub mod capabilities;
pub mod codec;
pub mod cpu_idle;
pub mod network;
pub mod power_features;
pub mod sleep;
pub mod time;
// The remaining port groups, one module each, each with its own `route`.
pub mod connectivity;
pub mod devices;
pub mod observation;
pub mod power;
pub mod reports;
pub mod service;
pub mod storage;
pub mod suspend;

use crate::reply::Outcome;
use y2p_core::Root;

pub type Command = Box<dyn FnOnce(&Root) -> Outcome>;

/// The Rust implementation of these exact arguments, if there is one.
pub fn route(args: &[&str]) -> Option<Command> {
    let one = |choices: &[&str], value: &str| choices.contains(&value);
    Some(match args {
        ["capabilities"] | ["capabilities", "--json"] => Box::new(capabilities::run),
        ["sleep", "status"] => Box::new(sleep::status),
        ["time"] => Box::new(time::status),
        ["time-remember"] => Box::new(time::remember),
        ["boot-stage", stage] if !stage.starts_with('-') => {
            let stage = stage.to_string();
            Box::new(move |root| boot::mark(root, &stage))
        }
        ["codec-runtime"] => Box::new(|root| codec::runtime(root, false)),
        ["codec-runtime", "--record"] => Box::new(|root| codec::runtime(root, true)),
        ["wifi-event", event] if one(&["CONNECTED", "DISCONNECTED", "SERVICE_RESTART"], event) => {
            let event = event.to_string();
            Box::new(move |root| network::wifi_event(root, &event))
        }
        ["dhcp-event", event]
            if one(&["deconfig", "leasefail", "nak", "bound", "renew"], event) =>
        {
            let event = event.to_string();
            Box::new(move |root| network::dhcp_event(root, &event))
        }
        ["ntp-event", event] if one(&["step", "stratum", "periodic", "unsync"], event) => {
            let event = event.to_string();
            Box::new(move |root| time::ntp_event(root, &event))
        }
        ["cpu-idle-policy", action] if one(&["start", "stop"], action) => {
            let action = action.to_string();
            Box::new(move |root| cpu_idle::apply(root, &action))
        }
        _ => {
            return observation::route(args)
                .or_else(|| connectivity::route(args))
                .or_else(|| devices::route(args))
                .or_else(|| storage::route(args))
                .or_else(|| service::route(args))
                .or_else(|| reports::route(args))
                .or_else(|| power::route(args))
                .or_else(|| suspend::route(args))
        }
    })
}
