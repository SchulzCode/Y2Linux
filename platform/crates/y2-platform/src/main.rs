//! `/usr/sbin/y2-platform`, the platform service's command line (docs/planning/PLATFORM-RUST-PORT-PLAN.md).
//! The arguments are parsed first (cli.rs): `--help` prints that subcommand's help (exit 0), an argument
//! error its usage and message on stderr (exit 2). A parsed command goes to its Rust implementation in
//! its canonical spelling (`cli::Outcome::Parsed`); every command line the parser takes has a route (the
//! test below).
// SPDX-License-Identifier: GPL-2.0-only
mod argparse;
mod cli;
mod commands;
mod reply;
#[cfg(test)]
mod testing;

use reply::Failure;
use std::{env, ffi::OsString, io::Write, process::ExitCode};
use y2p_core::{
    json::{dumps, Separators},
    os_text::{backslashreplace, fsdecode_os},
};

fn main() -> ExitCode {
    let mut args: Vec<OsString> = env::args_os().skip(1).collect();
    // The programmes this binary also is, by the name it was started under (links to
    // /usr/sbin/y2-platform: y2-status, y2-health, y2-suspend-record, y2-power-audit, poweroff, reboot).
    let name = env::args_os()
        .next()
        .map(std::path::PathBuf::from)
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
    if let Some(mapped) = name
        .as_deref()
        .and_then(|n| commands::suspend::program(n, &args))
    {
        args = mapped;
    }
    // The internal programmes (the sleep helper, suspend records, the power audit and power command) read
    // their arguments as the Python scripts they replace did (`os.fsdecode`: a byte that is not UTF-8 is
    // an escape character, y2p_core::os_text, shown as Python shows a surrogate); each runs and exits
    // itself.
    let decoded: Vec<String> = args.iter().map(|a| fsdecode_os(a)).collect();
    let text: Vec<&str> = decoded.iter().map(String::as_str).collect();
    if let Some(programme) = commands::suspend::programme(&text) {
        let root = y2p_core::Root::from_env();
        return match programme(&root) {
            Ok(reply) => print(
                &mut std::io::stdout().lock(),
                &format!("{}\n", reply.stdout),
                reply.code,
            ),
            Err(error) => print(
                &mut std::io::stderr().lock(),
                &format!("{}: {}\n", text[0], error.message()),
                1,
            ),
        };
    }
    if text == ["__arguments"] {
        // The argument table, for tools/development/platform-contract.py (platform/contract.json).
        let table = cli::table_text();
        return emit(&mut std::io::stdout().lock(), &format!("{table}\n"), 0);
    }
    let canonical = match cli::parse_os(&args) {
        cli::Outcome::Help(help) => return emit(&mut std::io::stdout().lock(), &help, 0),
        cli::Outcome::Error(error) => return emit(&mut std::io::stderr().lock(), &error, 2),
        cli::Outcome::Parsed(canonical) => canonical,
    };
    if env::var_os("Y2_ROOT").is_some() && env::var_os("Y2_ARGPARSE_ECHO").is_some() {
        // tests/test_platform_argparse.py: the canonical form itself instead of running it.
        let echo = serde_json::json!({ "canonical": canonical });
        return emit(&mut std::io::stdout().lock(), &format!("{echo}\n"), 0);
    }
    let canonical: Vec<&str> = canonical.iter().map(String::as_str).collect();
    let Some(command) = commands::route(&canonical) else {
        // No build of this binary implements it (the route test below rules this out).
        let _ = writeln!(
            std::io::stderr(),
            "y2-platform: {}: no implementation",
            canonical[0]
        );
        return ExitCode::from(127);
    };
    let root = y2p_core::Root::from_env();
    let (stdout, code) = match command(&root) {
        Ok(reply) => (reply.stdout, reply.code),
        // An exception main.py does not catch: its traceback on stderr, nothing on stdout, exit 1.
        Err(error @ Failure::Crash { .. }) => {
            return print(&mut std::io::stderr().lock(), &error.traceback(), 1)
        }
        // main.py: an OSError, ValueError or RuntimeError no command caught.
        Err(error) => (
            dumps(&error.record(), Separators::Spaced).expect("record"),
            1,
        ),
    };
    let mut out = std::io::stdout().lock();
    // print(): a closed pipe after the work is done changes nothing (Python reports it on stderr).
    let _ = out
        .write_all(stdout.as_bytes())
        .and_then(|()| out.write_all(b"\n"))
        .and_then(|()| out.flush());
    ExitCode::from(code)
}

/// Text that can hold an escaped byte of a programme's argument, shown as Python's `backslashreplace`
/// shows a surrogate (`\udcff`), then `emit`.
fn print(out: &mut dyn Write, text: &str, code: u8) -> ExitCode {
    emit(out, &backslashreplace(text), code)
}

/// Write the text and exit with `code`; a failed write is ignored (a caller that closed the pipe gets
/// no text, the exit code still tells).
fn emit(out: &mut dyn Write, text: &str, code: u8) -> ExitCode {
    let _ = out.write_all(text.as_bytes()).and_then(|()| out.flush());
    ExitCode::from(code)
}

#[cfg(test)]
mod tests {
    use crate::cli::{Arg, Kind, Type, COMMANDS};

    /// One sample value the parser takes for an argument (its first choice, a number, or a word).
    fn sample(arg: &Arg) -> String {
        if let Some(choice) = arg.choices.first() {
            return choice.to_string();
        }
        match arg.kind {
            Kind::One(Type::Int) | Kind::Many(Type::Int) => "1".into(),
            Kind::One(Type::Float) | Kind::Many(Type::Float) => "1.5".into(),
            _ => "x".into(),
        }
    }

    /// Every command line of the table, as far as routes tell them apart: each subcommand, each choice
    /// of its first positional with choices (the action), the other positionals with one sample (an
    /// optional positional also left out), alone and with each of its options once.
    fn command_lines() -> Vec<(String, Vec<String>)> {
        let mut lines = Vec::new();
        for command in COMMANDS {
            let name = command.name.to_string();
            let positionals: Vec<&Arg> = command.args.iter().filter(|a| a.positional()).collect();
            let optionals: Vec<&Arg> = command.args.iter().filter(|a| !a.positional()).collect();
            let chooser = positionals.iter().position(|a| !a.choices.is_empty());
            let choices: Vec<Option<&str>> = match chooser {
                Some(i) => positionals[i].choices.iter().map(|c| Some(*c)).collect(),
                None => vec![None],
            };
            for choice in choices {
                let mut full = vec![name.clone()];
                let mut short = vec![name.clone()];
                for (i, positional) in positionals.iter().enumerate() {
                    let value = match (Some(i) == chooser, choice) {
                        (true, Some(c)) => c.to_string(),
                        _ => sample(positional),
                    };
                    full.push(value.clone());
                    if positional.required || Some(i) == chooser {
                        short.push(value);
                    }
                }
                let label = match choice {
                    Some(c) => format!("{name} {c}"),
                    None => name.clone(),
                };
                for base in [&full, &short] {
                    lines.push((label.clone(), base.clone()));
                    for optional in &optionals {
                        let mut line = base.clone();
                        line.push(optional.name.to_string());
                        if optional.kind != Kind::Flag {
                            line.push(sample(optional));
                        }
                        lines.push((label.clone(), line));
                    }
                }
            }
        }
        lines
    }

    #[test]
    fn every_command_line_the_parser_takes_has_a_route() {
        let mut unrouted = std::collections::BTreeSet::new();
        let mut missing = Vec::new();
        let mut parsed = 0;
        for (label, line) in command_lines() {
            let args: Vec<&str> = line.iter().map(String::as_str).collect();
            let crate::cli::Outcome::Parsed(canonical) = crate::cli::parse(&args) else {
                continue; // a combination the parser refuses (a required option missing)
            };
            parsed += 1;
            let canonical: Vec<&str> = canonical.iter().map(String::as_str).collect();
            if crate::commands::route(&canonical).is_none() {
                missing.push(format!("{canonical:?}"));
                unrouted.insert(label);
            }
        }
        assert!(parsed > 300, "only {parsed} command lines parsed");
        assert!(missing.is_empty(), "no route for {missing:#?}");
        assert!(unrouted.is_empty(), "no route for {unrouted:?}");
    }
}
