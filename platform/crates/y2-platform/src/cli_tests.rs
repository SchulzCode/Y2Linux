use super::*;
use std::{ffi::OsString, os::unix::ffi::OsStringExt};

fn parsed(args: &[&str]) -> Vec<String> {
    match parse(args) {
        Outcome::Parsed(canonical) => canonical,
        other => panic!("{args:?}: {other:?}"),
    }
}

/// The error line of a refused command line (after its usage).
fn error(args: &[&str]) -> String {
    match parse(args) {
        Outcome::Error(text) => {
            assert!(text.starts_with("usage: y2-platform"), "{text}");
            assert!(text.ends_with('\n') && text.lines().count() >= 2, "{text}");
            text.lines().last().unwrap().to_string()
        }
        other => panic!("{args:?}: {other:?}"),
    }
}

fn command(name: &str) -> &'static Command {
    find(name).expect(name)
}

#[test]
fn canonical_forms() {
    for (args, canonical) in [
        // Options in definition order after the positionals, flags once, full names.
        (
            &["status", "--reborn", "--pss", "--reborn"][..],
            &["status", "--pss", "--reborn"][..],
        ),
        (
            &["status", "--reborn", "cpu", "--pid=7", "--pid", "+08"],
            &["status", "cpu", "--pid", "7", "--pid", "8", "--reborn"],
        ),
        // A repeated option: the last value counts.
        (
            &["status", "--interval", "1e-1", "--interval", "2"],
            &["status", "--interval", "2.0"],
        ),
        (
            &["status", "--interval=-.5"],
            &["status", "--interval", "-0.5"],
        ),
        (
            &["status", "--interval", "1E+01"],
            &["status", "--interval", "10.0"],
        ),
        (
            &["status", "--interval", "-0"],
            &["status", "--interval", "-0.0"],
        ),
        (&["capabilities", "--json"], &["capabilities", "--json"]),
        (&["capabilities"], &["capabilities"]),
        // Negative numbers are values; `--` makes anything one.
        (&["rtc-alarm", "-5"], &["rtc-alarm", "-5"]),
        (&["time-set", "-0"], &["time-set", "0"]),
        (&["collect", "--pid", "-1"], &["collect", "--pid", "-1"]),
        (&["boot-stage", "--", "-x"], &["boot-stage", "-x"]),
        (&["boot-stage", "x", "--"], &["boot-stage", "x"]),
        (&["sleep", "--", "status"], &["sleep", "status"]),
        (&["--", "time"], &["time"]),
        // Text with a space is a value, `--opt=` gives any text.
        (&["boot-stage", "-a b"], &["boot-stage", "-a b"]),
        (
            &["update", "check", "--url=-x"],
            &["update", "check", "--url", "-x"],
        ),
        (
            &["transfer", "commit", "--id=a b"],
            &["transfer", "commit", "--id", "a b"],
        ),
        (
            &["update", "check", "--url="],
            &["update", "check", "--url", ""],
        ),
        (
            &["network-check", "--seconds", "010", "--peer=h"],
            &["network-check", "--peer", "h", "--seconds", "10"],
        ),
        (
            &["firmware", "install", "/a b"],
            &["firmware", "install", "/a b"],
        ),
        (&["firmware", "status"], &["firmware", "status"]),
        (
            &["bench-library", "--tracks", "01000"],
            &["bench-library", "--tracks", "1000"],
        ),
        (
            &["media", "--json", "write", "--seconds", "5", "--release"],
            &["media", "write", "--seconds", "5", "--release", "--json"],
        ),
    ] {
        assert_eq!(parsed(args), canonical, "{args:?}");
    }
}

#[test]
fn what_callers_pass_parses_as_given() {
    // CONTRACT.md "Callers": Reborn's client.rs, the overlay scripts, the platform's own children. Each
    // reaches its route in the form it is written in, except Reborn's status (options in table order).
    assert_eq!(
        parsed(&["status", "--reborn", "--pss"]),
        ["status", "--pss", "--reborn"]
    );
    for line in [
        &["capabilities"][..],
        &["report", "status"],
        &["health"],
        &["status", "wifi"],
        &["update", "stage"],
        &["export-state", "--include-database"],
        &["codec-settings", "--ldac-quality", "high"],
        &["codec-settings", "--aac-abr", "off"],
        &["internal-media", "setup", "--confirm-shown"],
        &["media", "repair"],
        &[
            "media",
            "write",
            "--holder",
            "reborn",
            "--seconds",
            "60",
            "--release",
        ],
        &[
            "bench-storage",
            "--volume",
            "/data",
            "--size-mib",
            "16",
            "--operations",
            "64",
            "--seconds",
            "60",
        ],
        &["bench-library", "--volume", "/data", "--tracks", "1000"],
        &["report", "save", "--to", "sd"],
        &["time-set", "1761730000"],
        &["internal-media", "setup", "--automatic"],
        &["boot-stage", "platform_start"],
        &["cpu-idle-policy", "start"],
        &["codec-runtime", "--record"],
        &["wifi-event", "SERVICE_RESTART"],
        &["dhcp-event", "renew"],
        &["ntp-event", "step"],
        &["bluetooth-power", "on"],
        &["sleep", "prepare"],
        &["space", "--cleanup"],
        &["update", "health-ack"],
    ] {
        assert_eq!(parsed(line), line, "{line:?}");
        assert!(crate::commands::route(line).is_some(), "{line:?}");
    }
}

#[test]
fn help_anywhere_before_the_end_of_options() {
    let status = help(Some(command("status")));
    for args in [
        &["status", "--pss", "-h"][..],
        &["status", "cpu", "--help", "--pid", "x"],
        &["status", "--nope", "-h"],
        &["-x", "status", "-h"],
    ] {
        assert_eq!(parse(args), Outcome::Help(status.clone()), "{args:?}");
    }
    assert_eq!(parse(&["-h", "nonsense"]), Outcome::Help(help(None)));
    assert_eq!(parse(&["--help"]), Outcome::Help(help(None)));
    // After `--` it is a value; an error before it comes first.
    assert!(matches!(parse(&["sleep", "--", "-h"]), Outcome::Error(_)));
    assert!(matches!(
        parse(&["status", "--pid", "x", "-h"]),
        Outcome::Error(_)
    ));
    assert!(matches!(
        parse(&["sleep", "bogus", "-h"]),
        Outcome::Error(_)
    ));
}

#[test]
fn each_error() {
    for (args, message) in [
        (&[][..], "y2-platform: error: a command is required"),
        (&["nope"], "y2-platform: error: unknown command \"nope\""),
        (&["-5"], "y2-platform: error: unknown command \"-5\""),
        (
            &["sleep", "nap"],
            "y2-platform sleep: error: argument action: invalid choice: \"nap\" (choose from request, status, prepare)",
        ),
        (
            &["bench-library", "--tracks", "5"],
            "y2-platform bench-library: error: argument --tracks: invalid choice: \"5\" (choose from 1000, 10000, 20000)",
        ),
        (
            &["status", "--pid", "x"],
            "y2-platform status: error: argument --pid: invalid int value: \"x\"",
        ),
        (
            &["status", "--interval", "inf"],
            "y2-platform status: error: argument --interval: invalid float value: \"inf\"",
        ),
        (
            &["status", "--pss=1"],
            "y2-platform status: error: argument --pss: takes no value",
        ),
        (
            &["status", "--help="],
            "y2-platform status: error: argument --help: takes no value",
        ),
        (
            &["status", "--pid"],
            "y2-platform status: error: argument --pid: expected a value",
        ),
        (
            &["network-check", "--peer", "-h"],
            "y2-platform network-check: error: argument --peer: expected a value",
        ),
        (
            &["network-check"],
            "y2-platform network-check: error: the following arguments are required: --peer",
        ),
        (
            &["boot-stage", "-x"],
            "y2-platform boot-stage: error: the following arguments are required: stage",
        ),
        (
            &["time", "x", "--js"],
            "y2-platform time: error: unrecognized arguments: \"x\" \"--js\"",
        ),
        (
            &["time", "--"],
            "y2-platform time: error: unrecognized arguments: \"--\"",
        ),
        (
            &["firmware", "install", "p", "q"],
            "y2-platform firmware: error: unrecognized arguments: \"q\"",
        ),
        (
            &["-x", "time"],
            "y2-platform time: error: unrecognized arguments: \"-x\"",
        ),
    ] {
        assert_eq!(error(args), message, "{args:?}");
    }
    // What the owner's decision dropped: abbreviations, clusters, Python's number spellings.
    for args in [
        &["status", "--reb"][..],
        &["internal-media", "setup", "--confirm"],
        &["status", "--he"],
        &["status", "-hx"],
        &["status", "--pid", " 7"],
        &["status", "--pid", "1_0"],
        &["status", "--pid", "\u{0661}"],
        &["status", "--pid", "99999999999999999999"],
        &["time-set", "-1_0"],
        &["status", "--interval", "nan"],
        &["status", "--interval", "1e400"],
        &["status", "--interval", "1_0.5"],
        &["bench-library", "--tracks", " 1000"],
    ] {
        assert!(matches!(parse(args), Outcome::Error(_)), "{args:?}");
    }
}

#[test]
fn arguments_that_are_not_utf8_are_refused() {
    let args = [
        OsString::from("sleep"),
        OsString::from_vec(b"n\xffp".to_vec()),
    ];
    match parse_os(&args) {
        Outcome::Error(text) => assert!(
            text.starts_with("usage: y2-platform sleep")
                && text.ends_with("error: argument is not UTF-8: \"n\u{fffd}p\"\n"),
            "{text}"
        ),
        other => panic!("{other:?}"),
    }
    let args = [OsString::from("status")];
    assert_eq!(parse_os(&args), parse(&["status"]));
}

#[test]
fn numbers_are_plain() {
    assert_eq!(int("+007").as_deref(), Some("7"));
    assert_eq!(int("-0").as_deref(), Some("0"));
    assert_eq!(
        int("-9223372036854775808").as_deref(),
        Some("-9223372036854775808")
    );
    for bad in [
        "",
        "+",
        "1.0",
        " 1",
        "1_0",
        "0x10",
        "\u{0661}",
        "9223372036854775808",
    ] {
        assert_eq!(int(bad), None, "{bad:?}");
    }
    for (text, repr) in [
        ("1", "1.0"),
        ("5.", "5.0"),
        (".5", "0.5"),
        ("-1e-5", "-1e-05"),
        ("1e16", "1e+16"),
        ("0.1", "0.1"),
    ] {
        assert_eq!(float(text).as_deref(), Some(repr), "{text:?}");
    }
    for bad in [
        "", ".", "e5", "1e", "1e+", "inf", "nan", "1_0", " 1", "1e400", "0x1", "1.2.3",
    ] {
        assert_eq!(float(bad), None, "{bad:?}");
    }
}

#[test]
fn help_texts() {
    assert_eq!(
        help(Some(command("capabilities"))),
        "usage: y2-platform capabilities [-h] [--json]\n\noptions:\n  -h, --help  show this help message and exit\n  --json\n"
    );
    assert_eq!(
        help(Some(command("media"))),
        "usage: y2-platform media [-h] [--holder {reborn,report,radiofw,usb}]\n\
         \x20                        [--seconds SECONDS] [--release] [--json]\n\
         \x20                        [--confirm-cid CONFIRM_CID]\n\
         \x20                        {mount,unmount,check,status,reconcile,verify,format,grow,repair,write}\n\
         \n\
         positional arguments:\n\
         \x20 {mount,unmount,check,status,reconcile,verify,format,grow,repair,write}\n\
         \n\
         options:\n\
         \x20 -h, --help            show this help message and exit\n\
         \x20 --holder {reborn,report,radiofw,usb}\n\
         \x20                       write only: who holds the window\n\
         \x20 --seconds SECONDS     write only: how long the card stays writable\n\
         \x20 --release             write only: end the holder's window\n\
         \x20 --json\n\
         \x20 --confirm-cid CONFIRM_CID\n\
         \x20                       format only: the CID shown by `media status`; formats\n\
         \x20                       exactly that card\n"
    );
    let top = help(None);
    assert!(top.starts_with("usage: y2-platform [-h]\n                   {status,health,"));
    assert!(top.contains("\nCommand line entry point for the versioned platform contract.\n\n"));
    assert!(top.contains(
        "\n    report              opt-in error reports (docs/planning/ERROR-REPORTS-\n                        PLAN.md)\n"
    ));
    // Every subcommand's help starts with the usage its errors print.
    for c in COMMANDS {
        assert!(help(Some(c)).starts_with(&usage(Some(c))), "{}", c.name);
    }
    assert_eq!(
        crate::argparse::error_text("--interval must be positive"),
        format!(
            "{}y2-platform: error: --interval must be positive\n",
            usage(None)
        )
    );
}

#[test]
fn the_table_is_well_formed() {
    for c in COMMANDS {
        let mut names: Vec<&str> = c.args.iter().map(|a| a.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), c.args.len(), "{}: a name twice", c.name);
        // An optional positional only last, so positionals fill in order.
        let positionals: Vec<&Arg> = c.args.iter().filter(|a| a.positional()).collect();
        assert!(
            positionals
                .iter()
                .skip_while(|a| a.required)
                .all(|a| !a.required),
            "{}",
            c.name
        );
        for a in c.args {
            assert!(
                !a.positional() || a.kind != Kind::Flag,
                "{} {}",
                c.name,
                a.name
            );
            assert!(
                a.positional() || a.name.starts_with("--"),
                "{} {}",
                c.name,
                a.name
            );
            for choice in a.choices {
                let canonical = match a.value_type() {
                    Some(Type::Int) => int(choice),
                    Some(Type::Float) => float(choice),
                    _ => Some(choice.to_string()),
                };
                assert_eq!(canonical.as_deref(), Some(*choice), "{} {}", c.name, a.name);
            }
        }
    }
}

#[test]
fn contract_json_holds_this_table() {
    // tools/development/platform-contract.py --write copies `y2-platform __arguments` there.
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../contract.json");
    let contract: Json = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(contract["arguments"], table());
}
