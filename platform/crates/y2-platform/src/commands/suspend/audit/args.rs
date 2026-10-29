//! `y2-power-audit`'s arguments: the script's argparse parser (`--seconds`, `--settle`, `--state`,
//! `--output`, `--help`; abbreviations, `--x=y`, argparse's errors and texts at 80 columns, `prog`
//! y2-power-audit), then its `bounded windows only` refusal (exit 2).
// SPDX-License-Identifier: GPL-2.0-only
use crate::argparse::python;

pub(super) struct Args {
    pub(super) seconds: f64,
    pub(super) settle: f64,
    pub(super) state: String,
    pub(super) output: String,
}

pub(super) const USAGE: &str =
    "usage: y2-power-audit [-h] [--seconds SECONDS] [--settle SETTLE] --state STATE
                      --output OUTPUT
";
pub(super) const HELP: &str =
    "usage: y2-power-audit [-h] [--seconds SECONDS] [--settle SETTLE] --state STATE
                      --output OUTPUT

options:
  -h, --help         show this help message and exit
  --seconds SECONDS
  --settle SETTLE
  --state STATE
  --output OUTPUT
";

/// The script's parser: `-h/--help`, `--seconds` and `--settle` (float), `--state` and `--output`
/// (required), in argparse's lookup order (option string, action).
const OPTIONS: [(&str, usize); 6] = [
    ("-h", 0),
    ("--help", 0),
    ("--seconds", 1),
    ("--settle", 2),
    ("--state", 3),
    ("--output", 4),
];
const NAMES: [&str; 5] = ["-h/--help", "--seconds", "--settle", "--state", "--output"];

/// How argparse ends the script before it runs: `--help` (stdout, exit 0) or `p.error()` (exit 2).
pub(super) enum Stop {
    Help,
    Error(String),
}

/// An option string match: action (None: unknown), option string, separator, explicit argument.
#[derive(Clone)]
pub(super) struct Tuple {
    pub(super) action: Option<usize>,
    pub(super) option: String,
    pub(super) sep: Option<String>,
    pub(super) explicit: Option<String>,
}

pub(super) fn lookup(option: &str) -> Option<usize> {
    OPTIONS.iter().find(|(o, _)| *o == option).map(|(_, a)| *a)
}

pub(super) fn argument_error(action: usize, message: &str) -> Stop {
    Stop::Error(format!("argument {}: {message}", NAMES[action]))
}

/// `_parse_optional`: None for an argument, otherwise the option tuples.
pub(super) fn parse_optional(arg: &str) -> Option<Vec<Tuple>> {
    let tuple = |action, option: &str, sep: Option<&str>, explicit: Option<&str>| Tuple {
        action,
        option: option.into(),
        sep: sep.map(str::to_owned),
        explicit: explicit.map(str::to_owned),
    };
    if !arg.starts_with('-') {
        return None;
    }
    if let Some(action) = lookup(arg) {
        return Some(vec![tuple(Some(action), arg, None, None)]);
    }
    let text: Vec<char> = arg.chars().collect();
    if text.len() == 1 {
        return None;
    }
    if let Some((option, explicit)) = arg.split_once('=') {
        if let Some(action) = lookup(option) {
            return Some(vec![tuple(Some(action), option, Some("="), Some(explicit))]);
        }
    }
    // _get_option_tuples (allow_abbrev).
    let (prefix, sep, explicit) = match arg.split_once('=') {
        Some((prefix, explicit)) => (prefix, Some("="), Some(explicit)),
        None => (arg, None, None),
    };
    let mut found = Vec::new();
    let short: String = text[..2].iter().collect();
    let short_explicit: String = text[2..].iter().collect();
    for (option, action) in OPTIONS {
        if text[1] != '-' && option == short {
            found.push(tuple(Some(action), option, Some(""), Some(&short_explicit)));
        } else if option.starts_with(prefix) {
            found.push(tuple(Some(action), option, sep, explicit));
        }
    }
    if !found.is_empty() {
        return Some(found);
    }
    // `-\.?\d`: the parser has no option that looks like a negative number.
    let digit = |c: Option<&char>| c.is_some_and(|&c| python::decimal(c).is_some());
    if digit(text.get(1)) || (text[1] == '.' && digit(text.get(2))) || arg.contains(' ') {
        return None;
    }
    Some(vec![tuple(None, arg, None, None)])
}

/// `p.parse_args()` of the script's parser, then its window bounds.
pub(super) fn parse(args: &[String]) -> Result<Args, Stop> {
    let mut tuples = Vec::new();
    let mut pattern = Vec::new();
    let mut after_dashes = false;
    for arg in args {
        let found = if after_dashes || arg == "--" {
            None
        } else {
            parse_optional(arg)
        };
        pattern.push(match (&found, after_dashes || arg != "--") {
            (Some(_), _) => b'O',
            (None, true) => b'A',
            (None, false) => b'-',
        });
        after_dashes |= arg == "--";
        tuples.push(found);
    }
    let mut values: [Option<f64>; 2] = [Some(60.0), Some(10.0)];
    let mut texts: [Option<String>; 2] = [None, None];
    let mut seen = [false; 5];
    let mut extras: Vec<String> = Vec::new();
    // take_action: the help action ends the parse; a store converts its one string.
    let mut take = |action: usize, strings: &[String], seen: &mut [bool; 5]| -> Result<(), Stop> {
        seen[action] = true;
        match action {
            0 => Err(Stop::Help),
            1 | 2 => {
                let text = &strings[0];
                let value = python::float(text).ok_or_else(|| {
                    argument_error(
                        action,
                        &format!("invalid float value: {}", python::repr(text)),
                    )
                })?;
                values[action - 1] = Some(value);
                Ok(())
            }
            _ => {
                texts[action - 3] = Some(strings[0].clone());
                Ok(())
            }
        }
    };
    let max_option = tuples.iter().rposition(Option::is_some);
    let mut start = 0;
    while let Some(max) = max_option.filter(|&max| start <= max) {
        let mut next = start;
        while next <= max && tuples[next].is_none() {
            next += 1;
        }
        if tuples[start].is_none() {
            extras.extend(args[start..next].iter().cloned());
            start = next;
        }
        // consume_optional
        let found = tuples[start].clone().expect("option");
        if found.len() > 1 {
            let matches: Vec<&str> = found.iter().map(|t| t.option.as_str()).collect();
            return Err(Stop::Error(format!(
                "ambiguous option: {} could match {}",
                args[start],
                matches.join(", ")
            )));
        }
        let Tuple {
            mut action,
            mut option,
            mut sep,
            mut explicit,
        } = found.into_iter().next().expect("tuple");
        let mut actions: Vec<(usize, Vec<String>)> = Vec::new();
        let stop;
        loop {
            let Some(a) = action else {
                extras.push(args[start].clone());
                stop = start + 1;
                break;
            };
            // nargs 0 for the help action, one argument for the others.
            let wants = usize::from(a != 0);
            if let Some(argument) = explicit.clone() {
                if wants == 0 && !option.starts_with("--") && !argument.is_empty() {
                    if sep.as_deref().is_some_and(|s| !s.is_empty()) || argument.starts_with('-') {
                        return Err(argument_error(
                            a,
                            &format!("ignored explicit argument {}", python::repr(&argument)),
                        ));
                    }
                    actions.push((a, Vec::new()));
                    let mut rest = argument.chars();
                    let first = rest.next().expect("argument");
                    let rest = rest.as_str();
                    option = format!("-{first}");
                    if let Some(next) = lookup(&option) {
                        action = Some(next);
                        if rest.is_empty() {
                            (sep, explicit) = (None, None);
                        } else if let Some(value) = rest.strip_prefix('=') {
                            (sep, explicit) = (Some("=".into()), Some(value.into()));
                        } else {
                            (sep, explicit) = (Some(String::new()), Some(rest.into()));
                        }
                    } else {
                        extras.push(format!("-{argument}"));
                        stop = start + 1;
                        break;
                    }
                } else if wants == 1 {
                    stop = start + 1;
                    actions.push((a, vec![argument]));
                    break;
                } else {
                    return Err(argument_error(
                        a,
                        &format!("ignored explicit argument {}", python::repr(&argument)),
                    ));
                }
            } else {
                let from = start + 1;
                if wants == 1 && pattern.get(from) != Some(&b'A') {
                    return Err(argument_error(a, "expected one argument"));
                }
                stop = from + wants;
                actions.push((a, args[from..stop].to_vec()));
                break;
            }
        }
        for (a, strings) in actions {
            take(a, &strings, &mut seen)?;
        }
        start = stop;
    }
    extras.extend(args[start..].iter().cloned());
    let required: Vec<&str> = [3, 4]
        .into_iter()
        .filter(|&a| !seen[a])
        .map(|a| NAMES[a])
        .collect();
    if !required.is_empty() {
        return Err(Stop::Error(format!(
            "the following arguments are required: {}",
            required.join(", ")
        )));
    }
    if !extras.is_empty() {
        return Err(Stop::Error(format!(
            "unrecognized arguments: {}",
            extras.join(" ")
        )));
    }
    let [Some(seconds), Some(settle)] = values else {
        unreachable!("defaults")
    };
    // `not 1 <= seconds <= 7200 or not 0 <= settle <= 120` (NaN fails both).
    if !((1.0..=7200.0).contains(&seconds) && (0.0..=120.0).contains(&settle)) {
        return Err(Stop::Error("bounded windows only".into()));
    }
    let [Some(state), Some(output)] = texts else {
        unreachable!("required")
    };
    Ok(Args {
        seconds,
        settle,
        state,
        output,
    })
}
