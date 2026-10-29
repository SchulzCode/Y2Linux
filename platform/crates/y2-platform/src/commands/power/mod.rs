//! Group `power` of the port (docs/planning/PLATFORM-RUST-PORT-PLAN.md, "Remaining port groups"):
//! `power-daemon` (battery, charger, low battery, shutdown coordinator) and its client `shutdown`.
//! Python: power.py, battery.py, power_journal.py (charger_state.py is `y2p_core::power_charger`).
// SPDX-License-Identifier: GPL-2.0-only
pub mod battery;
pub mod client;
pub mod coordinator;
pub mod daemon;
pub mod journal;
pub mod policy;
pub mod unix;

use super::Command;
use std::io::Write;

/// The Rust implementation of these exact arguments (the canonical form argparse.rs hands over: the
/// `--reason` option only when it was given), if this group has one.
pub fn route(args: &[&str]) -> Option<Command> {
    let action = |a: &str| ["poweroff", "reboot"].contains(&a);
    let reason = |r: &str| ["user", "service", "update"].contains(&r);
    let (a, r) = match args {
        ["power-daemon"] => return Some(Box::new(daemon::serve)),
        ["shutdown", a] if action(a) => (*a, "user"),
        ["shutdown", a, "--reason", r] if action(a) && reason(r) => (*a, *r),
        _ => return None,
    };
    let (a, r) = (a.to_owned(), r.to_owned());
    Some(Box::new(move |root| client::shutdown(root, &a, &r)))
}

/// A daemon's return: nothing printed (main.py prints nothing after `serve`), this exit code.
pub fn quiet_exit(code: i32) -> ! {
    let _ = std::io::stdout().flush();
    std::process::exit(code)
}

#[cfg(test)]
mod tests {
    use super::route;

    #[test]
    fn canonical_forms_run_here() {
        for args in [
            &["power-daemon"][..],
            &["shutdown", "poweroff"],
            &["shutdown", "reboot", "--reason", "update"],
        ] {
            assert!(route(args).is_some(), "{args:?}");
        }
        for args in [
            &["power-daemon", "--help"][..],
            &["shutdown"],
            &["shutdown", "halt"],
            &["shutdown", "poweroff", "--reason", "low_battery"],
            &["shutdown", "--reason", "user", "poweroff"],
        ] {
            assert!(route(args).is_none(), "{args:?}");
        }
    }
}
