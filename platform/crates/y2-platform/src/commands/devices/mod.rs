//! The devices commands: `ssh-daemon` (owner SSH over USB), `usb-mode` (the gadget mode), `transfer`
//! (verified uploads), `codec-settings` (Bluetooth codec quality) and the clock commands `time-set`,
//! `time-bootstrap`, `rtc-alarm`. `route` sees the parser's canonical form (cli.rs); it takes exactly
//! the forms below and nothing else (`None`: no implementation).
// SPDX-License-Identifier: GPL-2.0-only
use super::Command;
use y2p_core::root::parse_int;

mod codec_settings;
pub(crate) mod ssh;
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
            let given = crate::cli::given("codec-settings", rest)?;
            let take = |name: &str| given.value(name).map(str::to_owned);
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
            let given = crate::cli::given("transfer", rest)?;
            // An integer option; the parser has already refused any other value (exit 2).
            let bytes = match given.value("--bytes") {
                Some(text) => Some(text.parse::<i64>().ok()?),
                None => None,
            };
            let take = |name: &str| given.value(name).map(str::to_owned);
            let request = transfer::Request {
                id: take("--id"),
                name: take("--name"),
                bytes,
                sha256: take("--sha256"),
            };
            let action = action.to_string();
            Box::new(move |root| transfer::run(root, &action, request))
        }
        _ => return None,
    })
}

/// A positional integer argument (sign, digits, `_` between digits). A leading `-` is taken only for a
/// plain negative number (`-5`); anything else starting with `-` would be an option.
fn integer(text: &str) -> Option<i64> {
    if let Some(digits) = text.strip_prefix('-') {
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    parse_int(text)
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
        assert!(route(&["rtc-alarm", "10"]).is_some());
        assert!(route(&["rtc-alarm", "1_0"]).is_none());
        // Options reach a route in the canonical form cli::parse gives; what it refuses (exit 2)
        // never runs.
        let runs = |args: &[&str]| match crate::cli::parse(args) {
            crate::cli::Outcome::Parsed(canonical) => {
                route(&canonical.iter().map(String::as_str).collect::<Vec<_>>()).is_some()
            }
            _ => false,
        };
        assert!(runs(&["codec-settings"]));
        assert!(runs(&["codec-settings", "--sbc-quality", "xq+"]));
        assert!(runs(&["codec-settings", "--sbc-quality=xq"]));
        assert!(!runs(&["codec-settings", "--sbc-quality", "max"]));
        assert!(!runs(&["codec-settings", "--sbc", "xq"]));
        assert!(runs(&["transfer", "status", "--id", "x"]));
        assert!(!runs(&["transfer", "begin", "--bytes", "x"]));
        assert!(!runs(&["transfer", "begin", "--name"]));
        assert!(!runs(&["transfer", "list"]));
        assert_eq!(integer(" 7 "), Some(7));
        assert_eq!(integer("-"), None);
    }
}
