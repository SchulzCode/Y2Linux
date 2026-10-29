//! The command line of `/usr/sbin/y2-platform`: what each subcommand takes (`COMMANDS`), the parse that
//! turns a command line into the canonical form `commands::route` matches, and the help and usage texts.
//!
//! The canonical form is the subcommand, the positionals given, then each option given in definition
//! order under its full name (`--opt value` as two tokens, a flag bare, an append option once per value,
//! a repeated option once with its last value); ints as plain decimals, floats as Python's repr, so a
//! route matches one spelling. Options take `--opt value` or `--opt=value`, exact names only; `-h` or
//! `--help` anywhere before `--` prints that subcommand's help (exit 0); anything else that does not fit
//! is an error: usage and `y2-platform <cmd>: error: …` on stderr, nothing on stdout, exit 2 (the owner's
//! decision of 2026-10-29: CONTRACT.md "Arguments"). platform/contract.json's `arguments` table is this
//! table (`table`, printed by `y2-platform __arguments` for tools/development/platform-contract.py).
// SPDX-License-Identifier: GPL-2.0-only
use serde_json::{json, Value as Json};

const PROG: &str = "y2-platform";
const DESCRIPTION: &str = "Command line entry point for the versioned platform contract.";
/// The help texts' width: argparse's 80 columns less its 2-column margin, which the help kept.
const WIDTH: usize = 78;
/// Where an argument's help starts at most.
const HELP_COLUMN: usize = 24;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Type {
    Str,
    Int,
    Float,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// `--opt` alone (store_true).
    Flag,
    /// One value; given again, the last counts.
    One(Type),
    /// One value per use, all kept (append).
    Many(Type),
}

/// The value a command assumes when an argument is not given. Commands apply it themselves (the
/// canonical form lists only what was given); here it is for help readers and contract.json.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Default {
    None,
    False,
    Empty,
    Str(&'static str),
    Int(i64),
}

#[derive(Clone, Copy, Debug)]
pub struct Arg {
    /// `--name` for an option, the bare name for a positional.
    pub name: &'static str,
    pub kind: Kind,
    pub choices: &'static [&'static str],
    pub default: Default,
    pub required: bool,
    pub help: &'static str,
}

impl Arg {
    const fn new(name: &'static str, kind: Kind, required: bool) -> Arg {
        Arg {
            name,
            kind,
            choices: &[],
            default: Default::None,
            required,
            help: "",
        }
    }

    const fn choices(self, choices: &'static [&'static str]) -> Arg {
        Arg { choices, ..self }
    }

    const fn default(self, default: Default) -> Arg {
        Arg { default, ..self }
    }

    const fn required(self) -> Arg {
        Arg {
            required: true,
            ..self
        }
    }

    const fn help(self, help: &'static str) -> Arg {
        Arg { help, ..self }
    }

    pub fn positional(&self) -> bool {
        !self.name.starts_with('-')
    }

    fn value_type(&self) -> Option<Type> {
        match self.kind {
            Kind::Flag => None,
            Kind::One(ty) | Kind::Many(ty) => Some(ty),
        }
    }

    /// What usage and help show for the value: the choices, else the name (in capitals for an option).
    fn metavar(&self) -> String {
        if !self.choices.is_empty() {
            format!("{{{}}}", self.choices.join(","))
        } else if self.positional() {
            self.name.to_string()
        } else {
            self.name
                .trim_start_matches('-')
                .replace('-', "_")
                .to_uppercase()
        }
    }

    /// The argument in usage: `[--opt VALUE]`, `--required VALUE`, `[--flag]`, `name`, `[name]`.
    fn usage(&self) -> String {
        match self.required {
            true => self.invocation(),
            false => format!("[{}]", self.invocation()),
        }
    }

    /// The argument in a help row: `--opt VALUE`, `--flag`, `name`.
    fn invocation(&self) -> String {
        match (self.positional(), self.kind) {
            (true, _) => self.metavar(),
            (false, Kind::Flag) => self.name.to_string(),
            (false, _) => format!("{} {}", self.name, self.metavar()),
        }
    }
}

/// A positional, required unless `optional()` is called.
const fn positional(name: &'static str) -> Arg {
    Arg::new(name, Kind::One(Type::Str), true)
}

const fn int_positional(name: &'static str) -> Arg {
    Arg::new(name, Kind::One(Type::Int), true)
}

const fn optional(arg: Arg) -> Arg {
    Arg {
        required: false,
        ..arg
    }
}

const fn flag(name: &'static str) -> Arg {
    Arg::new(name, Kind::Flag, false).default(Default::False)
}

const fn option(name: &'static str) -> Arg {
    Arg::new(name, Kind::One(Type::Str), false)
}

const fn int_option(name: &'static str, default: i64) -> Arg {
    Arg::new(name, Kind::One(Type::Int), false).default(Default::Int(default))
}

pub struct Command {
    pub name: &'static str,
    /// The line under the subcommand list of `y2-platform --help` (none when empty).
    pub help: &'static str,
    pub args: &'static [Arg],
}

const fn command(name: &'static str, args: &'static [Arg]) -> Command {
    Command {
        name,
        help: "",
        args,
    }
}

impl Command {
    const fn help(self, help: &'static str) -> Command {
        Command { help, ..self }
    }
}

const VOLUMES: &[&str] = &["/data", "/media/sd", "/media/internal"];
const ON_OFF: &[&str] = &["on", "off"];

/// Every subcommand in the order `y2-platform --help` lists them, each argument in definition order
/// (the order of the options in the canonical form).
#[rustfmt::skip] // one argument per line
pub const COMMANDS: &[Command] = &[
    command("status", &[
        optional(positional("section")).choices(&[
            "cpu", "memory", "thermal", "power", "storage", "wifi", "bluetooth", "system", "audio", "readiness", "usb",
        ]),
        flag("--json").help("JSON is the default output"),
        Arg::new("--pid", Kind::Many(Type::Int), false).default(Default::Empty),
        flag("--pss"),
        flag("--reborn").help("observe current Reborn PID and bounded metrics"),
        Arg::new("--interval", Kind::One(Type::Float), false).default(Default::Int(0)),
    ]),
    command("health", &[flag("--json"), flag("--full")]),
    command("capabilities", &[flag("--json")]),
    command("boot-evidence", &[]),
    command("boot-stage", &[positional("stage")]),
    command("cpu-idle-policy", &[positional("action").choices(&["start", "stop"])]),
    command("bluetooth-power", &[positional("value").choices(ON_OFF)]),
    command("sleep", &[positional("action").choices(&["request", "status", "prepare"])]),
    command("power-daemon", &[]),
    command("service-daemon", &[]),
    command("connectivity-daemon", &[]),
    command("usb-mode", &[positional("mode").choices(&["mtp", "developer"])]),
    command("codec-runtime", &[flag("--record")]),
    command("codec-settings", &[
        option("--sbc-quality").choices(&["high", "xq", "xq+"]),
        option("--sbc-abr").choices(ON_OFF),
        option("--ldac-quality").choices(&["mobile", "standard", "high"]),
        option("--ldac-abr").choices(ON_OFF),
        option("--aac-quality").choices(&["standard", "high"]),
        option("--aac-abr").choices(ON_OFF),
    ]),
    command("wifi-power-policy", &[]),
    command("ssh-daemon", &[]),
    command("transfer", &[
        positional("action").choices(&["begin", "commit", "status", "discard"]),
        option("--id"),
        option("--name"),
        Arg::new("--bytes", Kind::One(Type::Int), false),
        option("--sha256"),
    ]),
    command("reset", &[
        positional("action").choices(&["plan", "execute", "resume", "purge"]),
        option("--scope").choices(&["settings", "network", "bluetooth-bonds", "library", "caches", "full-user"]),
        option("--confirm"),
        option("--id"),
        flag("--erase-user-music"),
    ]),
    command("export-diagnostics", &[]),
    command("report", &[
        positional("action").choices(&[
            "status", "enable", "disable", "new-id", "send", "save", "check", "upload", "work",
        ]),
        option("--to").choices(&["internal", "sd"]).help("save only: Internal Storage or the SD card"),
        option("--topic").choices(&["sound", "display", "battery", "bluetooth", "wifi", "crash", "other"]),
    ]).help("opt-in error reports (docs/planning/ERROR-REPORTS-PLAN.md)"),
    command("export-state", &[flag("--include-database"), flag("--include-network")]),
    command("firmware", &[
        positional("action").choices(&["status", "scan", "install"]),
        optional(positional("path")).help("install: stock system.img or a directory of the reviewed files"),
    ]).help("radio firmware from the owner's own copy"),
    command("update", &[
        positional("action").choices(&[
            "status", "check", "stage", "apply", "rollback", "cancel", "health-ack", "auto-check", "probe-recovery",
            "recovery-status",
        ]),
        option("--url").help("HTTPS manifest.json URL (default: the release channel)"),
        option("--package").help("owner-supplied local signed package directory"),
    ]),
    command("time-bootstrap", &[]),
    command("time", &[]),
    command("time-set", &[
        int_positional("epoch").help("UTC seconds since 1970, within the build-date range"),
    ]).help("the owner's date and time (#53); network time replaces it"),
    command("time-remember", &[]).help("write the durable clock floor now (orderly shutdown)"),
    command("rtc-alarm", &[
        int_positional("seconds").help("seconds from RTC now; zero disables; does not suspend"),
    ]),
    command("ntp-event", &[positional("event").choices(&["step", "stratum", "periodic", "unsync"])]),
    command("wifi-event", &[positional("event").choices(&["CONNECTED", "DISCONNECTED", "SERVICE_RESTART"])]),
    command("dhcp-event", &[positional("event").choices(&["deconfig", "leasefail", "nak", "bound", "renew"])]),
    command("network-check", &[
        option("--peer").required(),
        int_option("--seconds", 10),
        flag("--throughput"),
        option("--interface").choices(&["wlan0", "usb0"]).default(Default::Str("wlan0")),
    ]),
    command("shutdown", &[
        positional("action").choices(&["poweroff", "reboot"]),
        option("--reason").choices(&["user", "service", "update"]).default(Default::Str("user")),
    ]),
    command("media", &[
        positional("action").choices(&[
            "mount", "unmount", "check", "status", "reconcile", "verify", "format", "grow", "repair", "write",
        ]),
        option("--holder").choices(&["reborn", "report", "radiofw", "usb"]).default(Default::Str("reborn"))
            .help("write only: who holds the window"),
        int_option("--seconds", 60).help("write only: how long the card stays writable"),
        flag("--release").help("write only: end the holder's window"),
        flag("--json"),
        option("--confirm-cid").help("format only: the CID shown by `media status`; formats exactly that card"),
    ]),
    command("internal-media", &[
        positional("action").choices(&["status", "mount", "unmount", "provision", "setup"]),
        option("--confirm-serial")
            .help("provision/setup: the stock FAT serial from `status`; that filesystem is replaced"),
        flag("--automatic")
            .help("setup only: decide by itself at start (keeps a few files, waits for a large library)"),
        flag("--confirm-shown")
            .help("setup only: the listener confirmed the old storage this boot reported as waiting"),
        flag("--no-migrate").help("provision only: leave /data/music untouched"),
    ]),
    command("bench-storage", &[
        option("--volume").choices(VOLUMES).default(Default::Str("/data")),
        int_option("--size-mib", 16),
        int_option("--operations", 64),
        int_option("--seconds", 300),
    ]),
    command("space", &[flag("--cleanup")]),
    command("bench-library", &[
        option("--volume").choices(VOLUMES).default(Default::Str("/data")),
        int_option("--tracks", 1000).choices(&["1000", "10000", "20000"]),
        flag("--scan"),
    ]),
    command("collect", &[
        int_option("--seconds", 60),
        int_option("--interval", 5),
        int_option("--warmup", 0),
        option("--workload").default(Default::Str("idle")),
        Arg::new("--pid", Kind::Many(Type::Int), false).default(Default::Empty),
        flag("--pss"),
        flag("--reborn"),
    ]),
];

/// What a command line asks for.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// `-h`/`--help`: the text for stdout, exit 0.
    Help(String),
    /// An argument error: the text for stderr, exit 2.
    Error(String),
    /// The canonical form for `commands::route`.
    Parsed(Vec<String>),
}

fn find(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.name == name)
}

/// `-h`/`--help` (exact: no abbreviations, no `-hx`).
fn is_help(token: &str) -> bool {
    token == "-h" || token == "--help"
}

/// Whether a token is meant as an option: it starts with `-` and is not `-` alone, a negative number
/// (`rtc-alarm -5`, `--pid -1`: no option looks like one) or text with a space (a value such as `-a b`).
fn looks_like_option(token: &str) -> bool {
    let mut rest = token.chars().skip(1);
    let number = match rest.next() {
        Some('.') => rest.next().is_some_and(|c| c.is_ascii_digit()),
        Some(c) => c.is_ascii_digit(),
        None => return false,
    };
    token.starts_with('-') && !number && !token.contains(' ')
}

/// A plain decimal integer with an optional sign, in its canonical spelling (`+07` → `7`, `-0` → `0`).
fn int(text: &str) -> Option<String> {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<i64>().ok().map(|n| n.to_string())
}

/// A plain finite decimal float (`1`, `-.5`, `5.`, `1e-3`) in its canonical spelling, Python's repr
/// (`1.0`, `-0.5`, `0.001`), which the routes read.
fn float(text: &str) -> Option<String> {
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(at) => (&unsigned[..at], Some(&unsigned[at + 1..])),
        None => (unsigned, None),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    let plain = !(whole.is_empty() && fraction.is_empty())
        && digits(whole)
        && digits(fraction)
        && exponent.is_none_or(|e| {
            let e = e.strip_prefix(['+', '-']).unwrap_or(e);
            !e.is_empty() && digits(e)
        });
    let value: f64 = text.parse().ok().filter(|_| plain)?;
    value.is_finite().then(|| y2p_core::json::float_repr(value))
}

/// Shown in an error: the text as typed, quoted and escaped.
fn shown(text: &str) -> String {
    format!("{text:?}")
}

/// The parse of one subcommand's arguments.
struct Parse<'a> {
    command: &'static Command,
    /// Per positional (in definition order), its canonical value once given.
    positionals: Vec<String>,
    /// Per option (argument index), its canonical values in the order given.
    options: Vec<(usize, String)>,
    /// Flags given (argument index).
    flags: Vec<usize>,
    unknown: Vec<&'a str>,
}

impl<'a> Parse<'a> {
    fn error(&self, message: String) -> Outcome {
        Outcome::Error(error_text(Some(self.command), &message))
    }

    /// The value as the argument's type and choices take it, canonical.
    fn value(&self, arg: &Arg, text: &str) -> Result<String, Outcome> {
        let value = match arg.value_type() {
            Some(Type::Int) => int(text).ok_or_else(|| {
                format!("argument {}: invalid int value: {}", arg.name, shown(text))
            }),
            Some(Type::Float) => float(text).ok_or_else(|| {
                format!(
                    "argument {}: invalid float value: {}",
                    arg.name,
                    shown(text)
                )
            }),
            _ => Ok(text.to_string()),
        }
        .map_err(|message| self.error(message))?;
        if !arg.choices.is_empty() && !arg.choices.contains(&value.as_str()) {
            return Err(self.error(format!(
                "argument {}: invalid choice: {} (choose from {})",
                arg.name,
                shown(text),
                arg.choices.join(", ")
            )));
        }
        Ok(value)
    }

    /// Whether a token is an option: one of this subcommand's by its exact name (`--id=a b` too),
    /// else by its looks.
    fn is_option(&self, token: &str) -> bool {
        let name = token.split_once('=').map_or(token, |(name, _)| name);
        let named = is_help(name)
            || self
                .command
                .args
                .iter()
                .any(|a| !a.positional() && a.name == name);
        (token.starts_with('-') && named) || looks_like_option(token)
    }

    fn positional_slots(&self) -> impl Iterator<Item = &'static Arg> {
        self.command.args.iter().filter(|a| a.positional())
    }

    /// The next positional not given yet.
    fn next_positional(&self) -> Option<&'static Arg> {
        self.positional_slots().nth(self.positionals.len())
    }

    fn positional(&mut self, token: &'a str) -> Result<(), Outcome> {
        match self.next_positional() {
            Some(arg) => {
                let value = self.value(arg, token)?;
                self.positionals.push(value);
            }
            None => self.unknown.push(token),
        }
        Ok(())
    }

    /// An option token at `tokens[0]`; returns how many tokens it used.
    fn option(&mut self, tokens: &[&'a str]) -> Result<usize, Outcome> {
        let token = tokens[0];
        let (name, explicit) = match token.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (token, None),
        };
        if is_help(name) {
            return Err(self.error(format!("argument {name}: takes no value")));
        }
        let Some(index) = self.command.args.iter().position(|a| a.name == name) else {
            self.unknown.push(token);
            return Ok(1);
        };
        let arg = &self.command.args[index];
        if arg.kind == Kind::Flag {
            if explicit.is_some() {
                return Err(self.error(format!("argument {name}: takes no value")));
            }
            if !self.flags.contains(&index) {
                self.flags.push(index);
            }
            return Ok(1);
        }
        let (text, used) = match explicit {
            Some(text) => (text, 1),
            None => match tokens.get(1) {
                Some(next) if *next != "--" && !self.is_option(next) => (*next, 2),
                _ => return Err(self.error(format!("argument {name}: expected a value"))),
            },
        };
        let value = self.value(arg, text)?;
        if let Kind::One(_) = arg.kind {
            self.options.retain(|(i, _)| *i != index);
        }
        self.options.push((index, value));
        Ok(used)
    }

    fn run(mut self, tokens: &[&'a str]) -> Outcome {
        let mut at = 0;
        let mut options_end = false;
        while let Some(&token) = tokens.get(at) {
            let step = if !options_end && token == "--" {
                // The rest are values, `-h` too; for a subcommand without positionals the `--`
                // itself is an argument too many. A later `--` is a value.
                options_end = true;
                if self.positional_slots().next().is_none() {
                    self.unknown.push(token);
                }
                Ok(1)
            } else if options_end || !self.is_option(token) {
                self.positional(token).map(|()| 1)
            } else if is_help(token) {
                return Outcome::Help(help(Some(self.command)));
            } else {
                self.option(&tokens[at..])
            };
            match step {
                Ok(used) => at += used,
                Err(outcome) => return outcome,
            }
        }
        let missing: Vec<String> = self
            .positional_slots()
            .skip(self.positionals.len())
            .filter(|a| a.required)
            .map(|a| a.name.to_string())
            .chain(
                self.command
                    .args
                    .iter()
                    .enumerate()
                    .filter(|(i, a)| {
                        !a.positional() && a.required && !self.options.iter().any(|(o, _)| o == i)
                    })
                    .map(|(_, a)| a.name.to_string()),
            )
            .collect();
        if !missing.is_empty() {
            return self.error(format!(
                "the following arguments are required: {}",
                missing.join(", ")
            ));
        }
        if !self.unknown.is_empty() {
            let unknown: Vec<String> = self.unknown.iter().map(|u| shown(u)).collect();
            return self.error(format!("unrecognized arguments: {}", unknown.join(" ")));
        }
        let mut canonical = vec![self.command.name.to_string()];
        canonical.append(&mut self.positionals);
        for (index, arg) in self.command.args.iter().enumerate() {
            if self.flags.contains(&index) {
                canonical.push(arg.name.to_string());
            }
            for (_, value) in self.options.iter().filter(|(i, _)| *i == index) {
                canonical.push(arg.name.to_string());
                canonical.push(value.clone());
            }
        }
        Outcome::Parsed(canonical)
    }
}

/// Parse `y2-platform`'s arguments (argv without argv[0]).
pub fn parse(args: &[&str]) -> Outcome {
    let mut unknown = Vec::new();
    let mut options_end = false;
    for (at, &token) in args.iter().enumerate() {
        if !options_end && is_help(token) {
            return Outcome::Help(help(None));
        }
        if !options_end && token == "--" {
            options_end = true;
            continue;
        }
        if !options_end && looks_like_option(token) {
            unknown.push(token);
            continue;
        }
        let Some(command) = find(token) else {
            let message = format!("unknown command {}", shown(token));
            return Outcome::Error(error_text(None, &message));
        };
        let parse = Parse {
            command,
            positionals: Vec::new(),
            options: Vec::new(),
            flags: Vec::new(),
            unknown,
        };
        return parse.run(&args[at + 1..]);
    }
    Outcome::Error(error_text(None, "a command is required"))
}

/// Parse argv as the process got it: y2-platform's arguments are text, so one that is not UTF-8 is an
/// error (no caller passes one; the hidden programmes read their own arguments).
pub fn parse_os(args: &[std::ffi::OsString]) -> Outcome {
    let text: Option<Vec<&str>> = args.iter().map(|a| a.to_str()).collect();
    match text {
        Some(text) => parse(&text),
        None => {
            let command = args.first().and_then(|a| a.to_str()).and_then(find);
            let bad = args.iter().find(|a| a.to_str().is_none()).expect("one");
            let message = format!("argument is not UTF-8: {}", shown(&bad.to_string_lossy()));
            Outcome::Error(error_text(command, &message))
        }
    }
}

/// An argument error of a subcommand (or of y2-platform itself): its usage, then
/// `y2-platform[ <cmd>]: error: <message>`. Also a command's own refusal after parsing (`status
/// --interval`), which names y2-platform itself.
pub fn error_text(command: Option<&Command>, message: &str) -> String {
    format!("{}{}: error: {message}\n", usage(command), prog(command))
}

fn prog(command: Option<&Command>) -> String {
    match command {
        Some(c) => format!("{PROG} {}", c.name),
        None => PROG.to_string(),
    }
}

fn command_choices() -> String {
    let names: Vec<&str> = COMMANDS.iter().map(|c| c.name).collect();
    format!("{{{}}}", names.join(","))
}

/// `usage: y2-platform <cmd> [-h] [options] positionals`, wrapped at the width with the parts aligned
/// after the programme name (options first, the positionals on lines of their own).
fn usage(command: Option<&Command>) -> String {
    const PREFIX: &str = "usage: ";
    let prog = prog(command);
    // The programme and its options, then the positionals.
    let mut head = vec![prog.clone(), "[-h]".to_string()];
    let mut tail = Vec::new();
    match command {
        Some(c) => {
            for arg in c.args {
                match arg.positional() {
                    true => tail.push(arg.usage()),
                    false => head.push(arg.usage()),
                }
            }
        }
        None => tail.push(format!("{} ...", command_choices())),
    }
    let one_line = [head.join(" "), tail.join(" ")].join(" ");
    if PREFIX.len() + one_line.trim_end().len() <= WIDTH {
        return format!("{PREFIX}{}\n", one_line.trim_end());
    }
    let indent = PREFIX.len() + prog.len() + 1;
    // Parts on lines of at most WIDTH columns, the first line starting at column `first`.
    let lines = |parts: &[String], first: usize| -> Vec<String> {
        let mut lines = Vec::new();
        let mut line: Vec<&str> = Vec::new();
        let mut length = first - 1;
        for part in parts {
            if length + 1 + part.len() > WIDTH && !line.is_empty() {
                lines.push(line.join(" "));
                line.clear();
                length = indent - 1;
            }
            line.push(part);
            length += part.len() + 1;
        }
        if !line.is_empty() {
            lines.push(line.join(" "));
        }
        lines
    };
    let mut text = lines(&head, PREFIX.len());
    text.extend(lines(&tail, indent));
    let pad = " ".repeat(indent);
    format!("{PREFIX}{}\n", text.join(&format!("\n{pad}")))
}

/// `text` in lines of at most `width` columns, broken at spaces and after the hyphen of a hyphenated
/// word (`ERROR-` `REPORTS-`), as the help texts were wrapped.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let chars: Vec<char> = word.chars().collect();
        let mut pieces = Vec::new();
        let mut start = 0;
        for i in 0..chars.len() {
            let letter = |j: usize| chars.get(j).is_some_and(|c| c.is_alphabetic());
            let hyphenated = chars[i] == '-'
                && i >= 2
                && letter(i - 1)
                && letter(i - 2)
                && letter(i + 1)
                && (letter(i + 2) || (chars.get(i + 2) == Some(&'-') && letter(i + 3)));
            if hyphenated {
                pieces.push(chars[start..=i].iter().collect::<String>());
                start = i + 1;
            }
        }
        pieces.push(chars[start..].iter().collect());
        for (n, piece) in pieces.iter().enumerate() {
            let space = usize::from(n == 0 && !line.is_empty());
            if !line.is_empty() && line.chars().count() + space + piece.chars().count() > width {
                lines.push(std::mem::take(&mut line));
            } else if space == 1 {
                line.push(' ');
            }
            line.push_str(piece);
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// `--help` of a subcommand (or of y2-platform itself): usage, the positionals and the options, each
/// with its help in a column after the longest (at most column 24).
pub fn help(command: Option<&Command>) -> String {
    // (indent, invocation, help) per row.
    let mut positionals: Vec<(usize, String, &str)> = Vec::new();
    let mut options: Vec<(usize, String, &str)> =
        vec![(2, "-h, --help".into(), "show this help message and exit")];
    match command {
        Some(c) => {
            for arg in c.args {
                let row = (2, arg.invocation(), arg.help);
                match arg.positional() {
                    true => positionals.push(row),
                    false => options.push(row),
                }
            }
        }
        None => {
            positionals.push((2, command_choices(), ""));
            positionals.extend(
                COMMANDS
                    .iter()
                    .filter(|c| !c.help.is_empty())
                    .map(|c| (4, c.name.to_string(), c.help)),
            );
        }
    }
    let longest = positionals
        .iter()
        .chain(&options)
        .map(|(indent, invocation, _)| indent + invocation.len())
        .max()
        .unwrap_or(0);
    let column = (longest + 2).min(HELP_COLUMN);
    let rows = |rows: &[(usize, String, &str)]| -> String {
        let mut out = String::new();
        for (indent, invocation, help) in rows {
            let pad = " ".repeat(*indent);
            if help.is_empty() {
                out.push_str(&format!("{pad}{invocation}\n"));
                continue;
            }
            let lines = wrap(help, WIDTH - column);
            let fits = invocation.len() + indent + 2 <= column;
            let mut lines = lines.iter();
            if fits {
                let width = column - indent - 2;
                let first = lines.next().map(String::as_str).unwrap_or("");
                out.push_str(&format!("{pad}{invocation:<width$}  {first}\n"));
            } else {
                out.push_str(&format!("{pad}{invocation}\n"));
            }
            for line in lines {
                out.push_str(&format!("{:column$}{line}\n", ""));
            }
        }
        out
    };
    let mut out = usage(command);
    out.push('\n');
    if command.is_none() {
        out.push_str(&format!("{DESCRIPTION}\n\n"));
    }
    if !positionals.is_empty() {
        out.push_str(&format!("positional arguments:\n{}\n", rows(&positionals)));
    }
    out.push_str(&format!("options:\n{}", rows(&options)));
    out
}

/// `table` as JSON text with the subcommands in `COMMANDS` order (contract.json keeps that order).
pub fn table_text() -> String {
    let table = table();
    let entries: Vec<String> = COMMANDS
        .iter()
        .map(|c| format!("{}:{}", Json::from(c.name), table[c.name]))
        .collect();
    format!("{{{}}}", entries.join(","))
}

/// The table as platform/contract.json's `arguments` holds it: per subcommand, each argument's name,
/// whether it is positional, type (`str`, `int`, `float`, `flag`, `append`), choices, default,
/// whether required, nargs (argparse's: `?` for an optional positional, 0 for a flag) and help.
pub fn table() -> Json {
    let mut out = serde_json::Map::new();
    for c in COMMANDS {
        let args: Vec<Json> = c
            .args
            .iter()
            .map(|a| {
                let ty = match a.kind {
                    Kind::Flag => "flag",
                    Kind::Many(_) => "append",
                    Kind::One(Type::Int) => "int",
                    Kind::One(Type::Float) => "float",
                    Kind::One(Type::Str) => "str",
                };
                let choices: Option<Vec<Json>> = (!a.choices.is_empty()).then(|| {
                    a.choices
                        .iter()
                        .map(|c| match a.value_type() {
                            Some(Type::Int) => json!(c.parse::<i64>().expect("an int choice")),
                            _ => json!(c),
                        })
                        .collect()
                });
                let default = match a.default {
                    Default::None => Json::Null,
                    Default::False => json!(false),
                    Default::Empty => json!([]),
                    Default::Str(s) => json!(s),
                    Default::Int(n) => json!(n),
                };
                let nargs = match (a.kind, a.positional() && !a.required) {
                    (Kind::Flag, _) => json!(0),
                    (_, true) => json!("?"),
                    _ => Json::Null,
                };
                json!({
                    "name": a.name,
                    "positional": a.positional(),
                    "type": ty,
                    "choices": choices,
                    "default": default,
                    "required": a.required,
                    "nargs": nargs,
                    "help": (!a.help.is_empty()).then_some(a.help),
                })
            })
            .collect();
        out.insert(c.name.to_string(), Json::Array(args));
    }
    Json::Object(out)
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
