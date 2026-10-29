//! Group `devices` of the port (docs/planning/PLATFORM-RUST-PORT-PLAN.md, "Remaining port groups"):
//! `ssh-daemon` (ssh.py), `usb-mode` (usb.request_mode), `transfer` (transfer.py), `codec-settings`
//! (codec_controls.configure/status) and `time-set`, `time-bootstrap`, `rtc-alarm` (timekeeping.py).
//! Rust takes only the argument forms below; every other one (`--help`, abbreviations, an invalid
//! choice or integer, a value that starts with `-`, a repeated option, options before the positional)
//! is argparse's and runs in Python.
// SPDX-License-Identifier: GPL-2.0-only
use super::Command;
use std::collections::BTreeMap;
use y2p_core::root::python_int;

mod codec_settings;
mod ssh;
pub(crate) mod storage;
mod timekeeping;
mod transfer;
mod usb;

/// The Rust implementation of these exact arguments, if this group has one.
pub fn route(args: &[&str]) -> Option<Command> {
    Some(match args {
        ["ssh-daemon"] => Box::new(ssh::serve),
        ["usb-mode", mode] if ["mtp", "developer"].contains(mode) => {
            let mode = mode.to_string();
            Box::new(move |root| usb::request_mode(root, &mode))
        }
        ["time-bootstrap"] => Box::new(timekeeping::bootstrap),
        ["time-set", epoch] => {
            let epoch = integer(epoch)?;
            Box::new(move |root| timekeeping::set_manual(root, epoch))
        }
        ["rtc-alarm", seconds] => {
            let seconds = integer(seconds)?;
            Box::new(move |root| timekeeping::alarm(root, seconds))
        }
        ["codec-settings", rest @ ..] => {
            let mut given = options(rest, &CODEC_OPTIONS)?;
            for (name, choices) in CODEC_OPTIONS.iter().zip(CODEC_CHOICES) {
                if given
                    .get(*name)
                    .is_some_and(|v| !choices.contains(&v.as_str()))
                {
                    return None;
                }
            }
            let mut take = |name: &str| given.remove(name);
            let changes = codec_settings::Changes {
                sbc_quality: take("--sbc-quality"),
                sbc_abr: take("--sbc-abr").map(|v| v == "on"),
                ldac_quality: take("--ldac-quality"),
                ldac_abr: take("--ldac-abr").map(|v| v == "on"),
                aac_quality: take("--aac-quality"),
                aac_abr: take("--aac-abr").map(|v| v == "on"),
            };
            Box::new(move |root| codec_settings::configure(root, changes))
        }
        ["transfer", action, rest @ ..]
            if ["begin", "commit", "status", "discard"].contains(action) =>
        {
            let mut given = options(rest, &["--id", "--name", "--bytes", "--sha256"])?;
            // `type=int`: a value int() refuses is argparse's error (exit 2).
            let bytes = match given.remove("--bytes") {
                Some(text) => Some(python_int(&text)?),
                None => None,
            };
            let request = transfer::Request {
                id: given.remove("--id"),
                name: given.remove("--name"),
                bytes,
                sha256: given.remove("--sha256"),
            };
            let action = action.to_string();
            Box::new(move |root| transfer::run(root, &action, request))
        }
        _ => return None,
    })
}

const CODEC_OPTIONS: [&str; 6] = [
    "--sbc-quality",
    "--sbc-abr",
    "--ldac-quality",
    "--ldac-abr",
    "--aac-quality",
    "--aac-abr",
];
const CODEC_CHOICES: [&[&str]; 6] = [
    &["high", "xq", "xq+"],
    &["on", "off"],
    &["mobile", "standard", "high"],
    &["on", "off"],
    &["standard", "high"],
    &["on", "off"],
];

/// A positional `type=int` argument: Python `int()` of it. argparse takes a leading `-` only for a
/// plain negative number (`-5`); anything else starting with `-` is an option to it.
fn integer(text: &str) -> Option<i64> {
    if let Some(digits) = text.strip_prefix('-') {
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    python_int(text)
}

/// Options given as `--name value` or `--name=value`, each at most once and spelled in full; `None`
/// for anything else (an abbreviation, an unknown option, a stray positional, a value that starts
/// with `-`, which argparse may read as an option).
fn options(args: &[&str], names: &[&str]) -> Option<BTreeMap<String, String>> {
    let mut given = BTreeMap::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let (name, value) = match arg.split_once('=') {
            Some((name, value)) if names.contains(&name) => (name, value.to_string()),
            _ if names.contains(arg) => {
                let value = rest.next()?;
                if value.starts_with('-') {
                    return None;
                }
                (*arg, value.to_string())
            }
            _ => return None,
        };
        if given.insert(name.to_string(), value).is_some() {
            return None;
        }
    }
    Some(given)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_full_well_formed_forms_are_taken() {
        assert!(route(&["ssh-daemon"]).is_some());
        assert!(route(&["ssh-daemon", "x"]).is_none());
        assert!(route(&["usb-mode", "mtp"]).is_some());
        assert!(route(&["usb-mode", "mass-storage"]).is_none());
        assert!(route(&["time-set", "1788000000"]).is_some());
        assert!(route(&["time-set", "-5"]).is_some());
        assert!(route(&["time-set", "-x"]).is_none());
        assert!(route(&["time-set", "17.5"]).is_none());
        assert!(route(&["rtc-alarm", "1_0"]).is_some());
        assert!(route(&["codec-settings"]).is_some());
        assert!(route(&["codec-settings", "--sbc-quality", "xq+"]).is_some());
        assert!(route(&["codec-settings", "--sbc-quality=xq"]).is_some());
        assert!(route(&["codec-settings", "--sbc-quality", "max"]).is_none());
        assert!(route(&["codec-settings", "--sbc", "xq"]).is_none());
        assert!(route(&["codec-settings", "--aac-abr", "on", "--aac-abr", "off"]).is_none());
        assert!(route(&["transfer", "status", "--id", "x"]).is_some());
        assert!(route(&["transfer", "begin", "--bytes", "x"]).is_none());
        assert!(route(&["transfer", "begin", "--name", "-a"]).is_none());
        assert!(route(&["transfer", "begin", "--name"]).is_none());
        assert!(route(&["transfer", "--id", "x", "status"]).is_none());
        assert!(route(&["transfer", "list"]).is_none());
        assert_eq!(integer(" 7 "), Some(7));
        assert_eq!(integer("-"), None);
    }
}
