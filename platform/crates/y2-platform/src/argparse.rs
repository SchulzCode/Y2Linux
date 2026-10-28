//! `main.py`'s argument parsing (cli.py's argparse parser, Python 3.15 on the player) in Rust, so that the
//! dispatcher parses, prints help and refuses arguments exactly as Python, and a ported command sees one
//! canonical spelling of what it was given (PLATFORM-RUST-PORT-PLAN.md §3, §5a; phase 6 has no Python).
//!
//! The parser is data: `argparse.json`, made from the real parser by tools/development/platform-argparse.py
//! (actions, option strings in lookup order, help and usage texts at 80 columns). The algorithm below is
//! argparse's `_parse_known_args` for the features that parser uses: one level of subcommands, store /
//! store_true / append / help actions, nargs None, '?' and 0 (the others for completeness), int and float
//! types, choices, long-option abbreviations, `--opt=value`, `--`, negative-number positionals, the error
//! order and texts (`usage…` then `<prog>: error: <message>` on stderr, exit 2) and `-h` anywhere (that
//! parser's help on stdout, exit 0).
// SPDX-License-Identifier: GPL-2.0-only
pub(crate) mod python;
#[rustfmt::skip]
mod unicode;

use serde::Deserialize;
use serde_json::Value;
use std::{cell::OnceCell, collections::BTreeSet};

/// `{"top":<main.py's parser>,` / `"commands":[` / one `["<name>",<parser>],` line per subcommand / `]}`: a
/// call reads only the lines of main.py and of the subcommand it names.
const SPEC: &str = include_str!("argparse.json");

/// main.py's parser.
fn load_top() -> Parser<'static> {
    let line = SPEC.lines().next().expect("argparse.json");
    let json = line
        .strip_prefix("{\"top\":")
        .and_then(|l| l.strip_suffix(','))
        .expect("argparse.json: the top line");
    Parser::from(serde_json::from_str(json).expect("argparse.json: main.py"))
}

/// The parser of subcommand `name`.
fn load_command(name: &str) -> Parser<'static> {
    let prefix = format!("[\"{name}\",");
    let line = SPEC
        .lines()
        .find(|l| l.starts_with(&prefix))
        .expect("argparse.json: a subcommand line");
    let (_, spec): (&str, ParserSpec) =
        serde_json::from_str(line.strip_suffix(',').unwrap_or(line)).expect("argparse.json");
    Parser::from(spec)
}

#[derive(Deserialize)]
struct ParserSpec<'a> {
    prog: String,
    usage: String,
    help: String,
    suggest_on_error: bool,
    #[serde(borrow)]
    options: Vec<(&'a str, usize)>,
    #[serde(borrow)]
    actions: Vec<ActionSpec<'a>>,
}

#[derive(Deserialize)]
struct ActionSpec<'a> {
    kind: Kind,
    dest: Option<&'a str>,
    #[serde(borrow)]
    options: Vec<&'a str>,
    nargs: Value,
    #[serde(rename = "type")]
    ty: Option<Type>,
    choices: Option<Vec<Value>>,
    default: Value,
    #[serde(rename = "const")]
    constant: Value,
    required: bool,
    name: Option<&'a str>,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Help,
    Store,
    StoreTrue,
    Append,
    Parsers,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
enum Type {
    Int,
    Float,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Nargs {
    One,
    Optional,
    ZeroOrMore,
    OneOrMore,
    Remainder,
    Parser,
    Exactly(usize),
}

impl Nargs {
    fn from(value: &Value) -> Nargs {
        match value {
            Value::Null => Nargs::One,
            Value::Number(n) => Nargs::Exactly(n.as_u64().expect("nargs") as usize),
            Value::String(s) => match s.as_str() {
                "?" => Nargs::Optional,
                "*" => Nargs::ZeroOrMore,
                "+" => Nargs::OneOrMore,
                "..." => Nargs::Remainder,
                "A..." => Nargs::Parser,
                other => panic!("nargs {other}"),
            },
            other => panic!("nargs {other}"),
        }
    }
}

/// A value in the namespace, with Python's types.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    None,
    Bool(bool),
    Str(String),
    /// `str(int)`: arbitrary size, as Python's.
    Int(String),
    Float(f64),
    List(Vec<Val>),
    Suppress,
}

impl Val {
    fn from_spec(value: &Value) -> Val {
        match value {
            Value::Null => Val::None,
            Value::Bool(b) => Val::Bool(*b),
            Value::String(s) => Val::Str(s.clone()),
            Value::Array(items) => Val::List(items.iter().map(Val::from_spec).collect()),
            Value::Object(o) => match (o.get("int"), o.get("float"), o.get("suppress")) {
                (Some(Value::String(i)), _, _) => Val::Int(i.clone()),
                (_, Some(Value::String(f)), _) => {
                    Val::Float(python::float(f).expect("float default"))
                }
                (_, _, Some(_)) => Val::Suppress,
                _ => panic!("value {o:?}"),
            },
            Value::Number(_) => panic!("untagged number"),
        }
    }

    /// `str(value)`.
    fn text(&self) -> String {
        match self {
            Val::Str(s) | Val::Int(s) => s.clone(),
            Val::Float(f) => python::float_repr(*f),
            Val::Bool(b) => if *b { "True" } else { "False" }.into(),
            Val::None => "None".into(),
            Val::List(_) | Val::Suppress => panic!("str() of {self:?}"),
        }
    }

    /// `self == other` for the choice types (str, int, float).
    fn equals(&self, other: &Val) -> bool {
        match (self, other) {
            (Val::Str(a), Val::Str(b)) | (Val::Int(a), Val::Int(b)) => a == b,
            (Val::Float(a), Val::Float(b)) => a == b,
            (Val::Int(i), Val::Float(f)) | (Val::Float(f), Val::Int(i)) => {
                // Exact for every int a float can equal that a command line spells.
                i.parse::<f64>().ok() == Some(*f) && f.fract() == 0.0 && i.len() < 300
            }
            _ => false,
        }
    }

    /// The value as JSON with ints and floats tagged (tools/development/platform-argparse.py `typed`).
    pub fn typed(&self) -> Value {
        match self {
            Val::None => Value::Null,
            Val::Bool(b) => Value::Bool(*b),
            Val::Str(s) => Value::String(s.clone()),
            Val::Int(i) => serde_json::json!({ "int": i }),
            Val::Float(f) => serde_json::json!({ "float": python::float_repr(*f) }),
            Val::List(items) => Value::Array(items.iter().map(Val::typed).collect()),
            Val::Suppress => serde_json::json!({ "suppress": true }),
        }
    }
}

struct Action<'a> {
    kind: Kind,
    dest: Option<&'a str>,
    options: Vec<&'a str>,
    nargs: Nargs,
    ty: Option<Type>,
    choices: Option<Vec<Val>>,
    default: Val,
    constant: Val,
    required: bool,
    name: Option<&'a str>,
}

struct Parser<'a> {
    prog: String,
    usage: String,
    help: String,
    suggest_on_error: bool,
    options: Vec<(&'a str, usize)>,
    actions: Vec<Action<'a>>,
}

impl<'a> Parser<'a> {
    fn from(spec: ParserSpec<'a>) -> Parser<'a> {
        Parser {
            prog: spec.prog,
            usage: spec.usage,
            help: spec.help,
            suggest_on_error: spec.suggest_on_error,
            options: spec.options,
            actions: spec
                .actions
                .into_iter()
                .map(|a| Action {
                    kind: a.kind,
                    dest: a.dest,
                    options: a.options,
                    nargs: Nargs::from(&a.nargs),
                    ty: a.ty,
                    choices: a.choices.map(|c| c.iter().map(Val::from_spec).collect()),
                    default: Val::from_spec(&a.default),
                    constant: Val::from_spec(&a.constant),
                    required: a.required,
                    name: a.name,
                })
                .collect(),
        }
    }

    fn lookup(&self, option: &str) -> Option<usize> {
        self.options
            .iter()
            .find(|(o, _)| *o == option)
            .map(|&(_, a)| a)
    }
}

/// What parsing the command line ends in.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// `-h`/`--help`: the text for stdout, exit 0.
    Help(String),
    /// An argparse error: the text for stderr, exit 2.
    Error(String),
    Parsed(Parsed),
}

/// A successful parse: argparse's namespace and the canonical argument list.
#[derive(Debug, PartialEq)]
pub struct Parsed {
    /// `vars(args)`: dest and value, in no particular order.
    pub namespace: Vec<(String, Val)>,
    /// The subcommand, the positionals given (definition order), then each optional given (definition order,
    /// full option string; `--opt value` as two tokens, a store_true flag bare, an append option once per
    /// value given, a repeated store option once with its last value). int values are `str(int(x))`, float
    /// values `repr(float(x))`.
    pub canonical: Vec<String>,
}

impl Parsed {
    pub fn typed(&self) -> Value {
        Value::Object(
            self.namespace
                .iter()
                .map(|(k, v)| (k.clone(), v.typed()))
                .collect(),
        )
    }
}

/// An `ArgumentError`: the action's name (None for parser-level errors) and the message.
struct ArgumentError {
    name: Option<String>,
    message: String,
}

impl ArgumentError {
    fn text(&self) -> String {
        match &self.name {
            Some(name) => format!("argument {name}: {}", self.message),
            None => self.message.clone(),
        }
    }
}

/// `sys.exit()` from inside parsing: which parser, and help or an error.
enum Stop {
    Help(usize),
    Error(usize, String),
}

enum Flow {
    Argument(ArgumentError),
    Stop(Stop),
}

impl From<ArgumentError> for Flow {
    fn from(error: ArgumentError) -> Flow {
        Flow::Argument(error)
    }
}

/// One `take_action`: the action, its converted value and how many strings it consumed.
struct Taken {
    action: usize,
    value: Val,
    strings: usize,
}

/// One parser's `_parse_known_args` result.
struct Known {
    namespace: Vec<(String, Val)>,
    extras: Vec<String>,
    taken: Vec<Taken>,
    /// The subcommand chosen and what its parser took.
    sub: Option<(usize, Vec<Taken>)>,
}

fn set(namespace: &mut Vec<(String, Val)>, dest: &str, value: Val) {
    match namespace.iter_mut().find(|(k, _)| k == dest) {
        Some(entry) => entry.1 = value,
        None => namespace.push((dest.to_string(), value)),
    }
}

fn get<'n>(namespace: &'n [(String, Val)], dest: &str) -> Option<&'n Val> {
    namespace.iter().find(|(k, _)| k == dest).map(|(_, v)| v)
}

/// A regex atom of argparse's nargs patterns: one of the classes A, O, `-` repeated min..=max times.
#[derive(Clone, Copy)]
struct Atom {
    class: &'static [u8],
    min: usize,
    max: usize,
    group: usize,
}

const MANY: usize = usize::MAX;

/// `_get_nargs_pattern` as atoms, all in capture group `group`.
fn nargs_atoms(nargs: Nargs, optional: bool, group: usize, out: &mut Vec<Atom>) {
    let atom = |class, min, max| Atom {
        class,
        min,
        max,
        group,
    };
    let dashes = atom(b"-", 0, MANY);
    match (nargs, optional) {
        (Nargs::One, true) => out.push(atom(b"A", 1, 1)),
        (Nargs::One, false) => out.extend([dashes, atom(b"A", 1, 1), dashes]),
        (Nargs::Optional, true) => out.push(atom(b"A", 0, 1)),
        (Nargs::Optional, false) => out.extend([dashes, atom(b"A", 0, 1), dashes]),
        (Nargs::ZeroOrMore, true) => out.push(atom(b"A", 0, MANY)),
        (Nargs::ZeroOrMore, false) => out.extend([dashes, atom(b"A-", 0, MANY)]),
        (Nargs::OneOrMore, true) => out.extend([atom(b"A", 1, 1), atom(b"A", 0, MANY)]),
        (Nargs::OneOrMore, false) => out.extend([dashes, atom(b"A", 1, 1), atom(b"A-", 0, MANY)]),
        (Nargs::Remainder, _) => out.push(atom(b"AO-", 0, MANY)),
        (Nargs::Parser, true) => out.extend([atom(b"A", 1, 1), atom(b"AO", 0, MANY)]),
        (Nargs::Parser, false) => out.extend([dashes, atom(b"A", 1, 1), atom(b"-AO", 0, MANY)]),
        (Nargs::Exactly(n), true) => out.push(atom(b"AO", n, n)),
        (Nargs::Exactly(n), false) => {
            for _ in 0..n {
                out.extend([dashes, atom(b"A", 1, 1)]);
            }
            out.push(dashes);
        }
    }
}

/// `re.match(pattern, text)` for a sequence of greedy atoms (backtracking as Python's engine does): the
/// length each group matched and where the match ends.
fn match_atoms(atoms: &[Atom], groups: usize, text: &[u8]) -> Option<(Vec<usize>, usize)> {
    fn go(atoms: &[Atom], at: usize, text: &[u8], counts: &mut [usize]) -> Option<usize> {
        let Some(atom) = atoms.first() else {
            return Some(at);
        };
        let mut most = 0;
        while most < atom.max && at + most < text.len() && atom.class.contains(&text[at + most]) {
            most += 1;
        }
        if most < atom.min {
            return None;
        }
        let mut n = most;
        loop {
            counts[0] = n;
            if let Some(end) = go(&atoms[1..], at + n, text, &mut counts[1..]) {
                return Some(end);
            }
            if n == atom.min {
                return None;
            }
            n -= 1;
        }
    }
    let mut counts = vec![0; atoms.len()];
    let end = go(atoms, 0, text, &mut counts)?;
    let mut lengths = vec![0; groups];
    for (atom, n) in atoms.iter().zip(counts) {
        lengths[atom.group] += n;
    }
    Some((lengths, end))
}

/// The parsers by index, 0: main.py; 1..: the subcommands in cli.py's order (read when first used).
struct Engine {
    top: Parser<'static>,
    names: Vec<String>,
    commands: Vec<OnceCell<Parser<'static>>>,
}

impl Engine {
    fn load() -> Engine {
        let top = load_top();
        let names: Vec<String> = top
            .actions
            .iter()
            .find(|a| a.kind == Kind::Parsers)
            .and_then(|a| a.choices.as_ref())
            .expect("main.py's subcommands")
            .iter()
            .map(Val::text)
            .collect();
        let commands = names.iter().map(|_| OnceCell::new()).collect();
        Engine {
            top,
            names,
            commands,
        }
    }

    fn parser(&self, p: usize) -> &Parser<'static> {
        match p {
            0 => &self.top,
            _ => self.commands[p - 1].get_or_init(|| load_command(&self.names[p - 1])),
        }
    }

    fn parse(&self, args: &[&str]) -> Outcome {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        match self.parse_args(&args) {
            Ok(parsed) => Outcome::Parsed(parsed),
            Err(Stop::Help(p)) => Outcome::Help(self.parser(p).help.clone()),
            Err(Stop::Error(p, message)) => Outcome::Error(self.error_text(p, &message)),
        }
    }

    /// `parser.error(message)`: usage, then `<prog>: error: <message>`.
    fn error_text(&self, p: usize, message: &str) -> String {
        let parser = self.parser(p);
        format!("{}{}: error: {message}\n", parser.usage, parser.prog)
    }

    /// `parse_args`: unrecognized arguments are an error of main.py's parser.
    fn parse_args(&self, args: &[String]) -> Result<Parsed, Stop> {
        let known = self.parse_known(0, args)?;
        if !known.extras.is_empty() {
            let message = format!("unrecognized arguments: {}", known.extras.join(" "));
            return Err(Stop::Error(0, message));
        }
        let (sub, taken) = known.sub.expect("the subcommand is required");
        Ok(Parsed {
            canonical: self.canonical(sub, &taken),
            namespace: known.namespace,
        })
    }

    fn canonical(&self, sub: usize, taken: &[Taken]) -> Vec<String> {
        let parser = self.parser(sub);
        let mut out = vec![self.names[sub - 1].clone()];
        fn tokens(value: &Val, out: &mut Vec<String>) {
            match value {
                Val::List(items) => items.iter().for_each(|v| tokens(v, out)),
                other => out.push(other.text()),
            }
        }
        for (i, action) in parser.actions.iter().enumerate() {
            if action.options.is_empty() && action.kind != Kind::Help {
                for t in taken.iter().filter(|t| t.action == i && t.strings > 0) {
                    tokens(&t.value, &mut out);
                }
            }
        }
        for (i, action) in parser.actions.iter().enumerate() {
            if action.options.is_empty() || action.kind == Kind::Help {
                continue;
            }
            let given: Vec<&Taken> = taken.iter().filter(|t| t.action == i).collect();
            let Some(last) = given.last() else {
                continue;
            };
            let option = action
                .options
                .iter()
                .find(|o| o.starts_with("--"))
                .unwrap_or(&action.options[0])
                .to_string();
            match action.kind {
                Kind::StoreTrue => out.push(option),
                Kind::Append => {
                    for t in &given {
                        out.push(option.clone());
                        tokens(&t.value, &mut out);
                    }
                }
                _ => {
                    out.push(option);
                    tokens(&last.value, &mut out);
                }
            }
        }
        out
    }

    /// `_parse_known_args2` of parser `p`: an ArgumentError becomes that parser's error.
    fn parse_known(&self, p: usize, args: &[String]) -> Result<Known, Stop> {
        let mut state = State {
            engine: self,
            p,
            parser: self.parser(p),
            args,
            pattern: Vec::new(),
            option_tuples: Vec::new(),
            namespace: Vec::new(),
            extras: Vec::new(),
            unrecognized: Vec::new(),
            seen: BTreeSet::new(),
            positionals: Vec::new(),
            taken: Vec::new(),
            sub: None,
        };
        for action in &state.parser.actions {
            if let Some(dest) = action.dest {
                if action.default != Val::Suppress && get(&state.namespace, dest).is_none() {
                    set(&mut state.namespace, dest, action.default.clone());
                }
            }
        }
        match state.run() {
            Ok(()) => {}
            Err(Flow::Stop(stop)) => return Err(stop),
            Err(Flow::Argument(error)) => return Err(Stop::Error(p, error.text())),
        }
        let mut extras = state.extras;
        extras.extend(state.unrecognized);
        Ok(Known {
            namespace: state.namespace,
            extras,
            taken: state.taken,
            sub: state.sub,
        })
    }
}

/// An option string match: (action, option string, separator, explicit argument).
#[derive(Clone)]
struct OptionTuple {
    action: Option<usize>,
    option: String,
    sep: Option<String>,
    explicit: Option<String>,
}

struct State<'e> {
    engine: &'e Engine,
    p: usize,
    parser: &'e Parser<'static>,
    args: &'e [String],
    pattern: Vec<u8>,
    option_tuples: Vec<Option<Vec<OptionTuple>>>,
    namespace: Vec<(String, Val)>,
    extras: Vec<String>,
    /// A subparser's extras (`_UNRECOGNIZED_ARGS_ATTR`), after this parser's own.
    unrecognized: Vec<String>,
    seen: BTreeSet<usize>,
    positionals: Vec<usize>,
    taken: Vec<Taken>,
    sub: Option<(usize, Vec<Taken>)>,
}

fn chars(text: &str) -> Vec<char> {
    text.chars().collect()
}

impl State<'_> {
    fn run(&mut self) -> Result<(), Flow> {
        let args = self.args;
        // 'O' an option, 'A' an argument, '-' the first `--` (everything after it is an argument).
        let mut after_dashes = false;
        for arg in args {
            let tuples = if after_dashes || arg == "--" {
                None
            } else {
                self.parse_optional(arg)?
            };
            self.pattern
                .push(match (&tuples, after_dashes || arg != "--") {
                    (Some(_), _) => b'O',
                    (None, true) => b'A',
                    (None, false) => b'-',
                });
            after_dashes |= arg == "--";
            self.option_tuples.push(tuples);
        }
        self.positionals = (0..self.parser.actions.len())
            .filter(|&i| self.parser.actions[i].options.is_empty())
            .collect();
        let max_option = self.option_tuples.iter().rposition(Option::is_some);
        let mut start = 0;
        while let Some(max) = max_option.filter(|&max| start <= max) {
            let mut next = start;
            while next <= max && self.option_tuples[next].is_none() {
                next += 1;
            }
            if start != next {
                let end = self.consume_positionals(start)?;
                if end > start {
                    start = end;
                    continue;
                }
                start = end;
            }
            if self.option_tuples[start].is_none() {
                self.extras.extend(args[start..next].iter().cloned());
                start = next;
            }
            start = self.consume_optional(start)?;
        }
        let stop = self.consume_positionals(start)?;
        self.extras.extend(args[stop..].iter().cloned());
        // Required actions, and string defaults converted by the action's type.
        let mut required = Vec::new();
        for (i, action) in self.parser.actions.iter().enumerate() {
            if self.seen.contains(&i) {
                continue;
            }
            if action.required {
                required.push(action.name.unwrap_or("None"));
            } else if let (Val::Str(default), Some(dest)) = (&action.default, action.dest) {
                if get(&self.namespace, dest).is_some() {
                    let value = self.get_value(i, default)?;
                    set(&mut self.namespace, dest, value);
                }
            }
        }
        if !required.is_empty() {
            return Err(ArgumentError {
                name: None,
                message: format!(
                    "the following arguments are required: {}",
                    required.join(", ")
                ),
            }
            .into());
        }
        Ok(())
    }

    fn error(&self, action: Option<usize>, message: String) -> Flow {
        Flow::Argument(ArgumentError {
            name: action.and_then(|a| self.parser.actions[a].name.map(str::to_string)),
            message,
        })
    }

    /// `_parse_optional`: None for a positional, otherwise the option tuples (action None: unknown).
    fn parse_optional(&self, arg: &str) -> Result<Option<Vec<OptionTuple>>, Flow> {
        let tuple = |action, option: &str, sep: Option<&str>, explicit: Option<&str>| OptionTuple {
            action,
            option: option.to_string(),
            sep: sep.map(str::to_string),
            explicit: explicit.map(str::to_string),
        };
        if !arg.starts_with('-') {
            return Ok(None);
        }
        if let Some(action) = self.parser.lookup(arg) {
            return Ok(Some(vec![tuple(Some(action), arg, None, None)]));
        }
        let text = chars(arg);
        if text.len() == 1 {
            return Ok(None);
        }
        if let Some((option, explicit)) = arg.split_once('=') {
            if let Some(action) = self.parser.lookup(option) {
                return Ok(Some(vec![tuple(
                    Some(action),
                    option,
                    Some("="),
                    Some(explicit),
                )]));
            }
        }
        let tuples = self.option_tuples_for(arg, &text);
        if !tuples.is_empty() {
            return Ok(Some(tuples));
        }
        // `-\.?\d` (the parser has no option that looks like a negative number).
        let digit = |c: Option<&char>| c.is_some_and(|&c| python::decimal(c).is_some());
        if digit(text.get(1)) || (text[1] == '.' && digit(text.get(2))) {
            return Ok(None);
        }
        if arg.contains(' ') {
            return Ok(None);
        }
        Ok(Some(vec![tuple(None, arg, None, None)]))
    }

    /// `_get_option_tuples` (allow_abbrev).
    fn option_tuples_for(&self, arg: &str, text: &[char]) -> Vec<OptionTuple> {
        let (prefix, sep, explicit) = match arg.split_once('=') {
            Some((prefix, explicit)) => (prefix, Some("="), Some(explicit)),
            None => (arg, None, None),
        };
        let mut found = Vec::new();
        if text[1] == '-' {
            for &(option, action) in &self.parser.options {
                if option.starts_with(prefix) {
                    found.push(OptionTuple {
                        action: Some(action),
                        option: option.to_string(),
                        sep: sep.map(str::to_string),
                        explicit: explicit.map(str::to_string),
                    });
                }
            }
        } else {
            let short: String = text[..2].iter().collect();
            let short_explicit: String = text[2..].iter().collect();
            for &(option, action) in &self.parser.options {
                if option == short {
                    found.push(OptionTuple {
                        action: Some(action),
                        option: option.to_string(),
                        sep: Some(String::new()),
                        explicit: Some(short_explicit.clone()),
                    });
                } else if option.starts_with(prefix) {
                    found.push(OptionTuple {
                        action: Some(action),
                        option: option.to_string(),
                        sep: sep.map(str::to_string),
                        explicit: explicit.map(str::to_string),
                    });
                }
            }
        }
        found
    }

    fn match_argument(&self, action: usize, pattern: &[u8]) -> Result<usize, Flow> {
        let a = &self.parser.actions[action];
        let mut atoms = Vec::new();
        nargs_atoms(a.nargs, !a.options.is_empty(), 0, &mut atoms);
        if let Some((lengths, _)) = match_atoms(&atoms, 1, pattern) {
            return Ok(lengths[0]);
        }
        let message = match a.nargs {
            Nargs::One => "expected one argument".to_string(),
            Nargs::Optional => "expected at most one argument".to_string(),
            Nargs::OneOrMore => "expected at least one argument".to_string(),
            Nargs::Exactly(1) => "expected 1 argument".to_string(),
            Nargs::Exactly(n) => format!("expected {n} arguments"),
            Nargs::ZeroOrMore => "expected * arguments".to_string(),
            Nargs::Remainder => "expected ... arguments".to_string(),
            Nargs::Parser => "expected A... arguments".to_string(),
        };
        Err(self.error(Some(action), message))
    }

    /// `_match_arguments_partial` of the remaining positionals.
    fn match_partial(&self, pattern: &[u8]) -> Vec<usize> {
        for i in (1..=self.positionals.len()).rev() {
            let mut atoms = Vec::new();
            for (group, &action) in self.positionals[..i].iter().enumerate() {
                let a = &self.parser.actions[action];
                nargs_atoms(a.nargs, false, group, &mut atoms);
            }
            if let Some((mut lengths, end)) = match_atoms(&atoms, i, pattern) {
                if end < pattern.len() && pattern[end] == b'O' {
                    while lengths.last() == Some(&0) {
                        lengths.pop();
                    }
                }
                return lengths;
            }
        }
        Vec::new()
    }

    fn consume_positionals(&mut self, start: usize) -> Result<usize, Flow> {
        let counts = self.match_partial(&self.pattern[start..]);
        let mut start = start;
        for (k, &count) in counts.iter().enumerate() {
            let action = self.positionals[k];
            let mut strings: Vec<String> = self.args[start..start + count].to_vec();
            match self.parser.actions[action].nargs {
                Nargs::Parser => {
                    if self.pattern[start] == b'-' {
                        strings.remove(0);
                    }
                }
                Nargs::Remainder => {}
                _ => {
                    if self.pattern[start..start + count].contains(&b'-') {
                        let dashes = strings.iter().position(|s| s == "--").expect("--");
                        strings.remove(dashes);
                    }
                }
            }
            start += count;
            self.take_action(action, strings)?;
        }
        self.positionals.drain(..counts.len());
        Ok(start)
    }

    fn consume_optional(&mut self, start: usize) -> Result<usize, Flow> {
        let tuples = self.option_tuples[start].clone().expect("option");
        if tuples.len() > 1 {
            let matches: Vec<&str> = tuples.iter().map(|t| t.option.as_str()).collect();
            return Err(ArgumentError {
                name: None,
                message: format!(
                    "ambiguous option: {} could match {}",
                    self.args[start],
                    matches.join(", ")
                ),
            }
            .into());
        }
        let OptionTuple {
            mut action,
            mut option,
            mut sep,
            mut explicit,
        } = tuples.into_iter().next().expect("tuple");
        let mut actions: Vec<(usize, Vec<String>, String)> = Vec::new();
        let stop;
        loop {
            let Some(a) = action else {
                self.extras.push(self.args[start].clone());
                return Ok(start + 1);
            };
            if let Some(argument) = explicit.clone() {
                let count = self.match_argument(a, b"A")?;
                let second = option.chars().nth(1);
                if count == 0 && second != Some('-') && !argument.is_empty() {
                    if sep.as_deref().is_some_and(|s| !s.is_empty()) || argument.starts_with('-') {
                        return Err(self.error(
                            Some(a),
                            format!("ignored explicit argument {}", python::repr(&argument)),
                        ));
                    }
                    actions.push((a, Vec::new(), option.clone()));
                    let prefix = option.chars().next().expect("prefix");
                    let mut rest = argument.chars();
                    let first = rest.next().expect("argument");
                    let rest = rest.as_str();
                    option = format!("{prefix}{first}");
                    if let Some(next) = self.parser.lookup(&option) {
                        action = Some(next);
                        if rest.is_empty() {
                            (sep, explicit) = (None, None);
                        } else if let Some(value) = rest.strip_prefix('=') {
                            (sep, explicit) = (Some("=".into()), Some(value.into()));
                        } else {
                            (sep, explicit) = (Some(String::new()), Some(rest.into()));
                        }
                    } else {
                        self.extras.push(format!("{prefix}{argument}"));
                        stop = start + 1;
                        break;
                    }
                } else if count == 1 {
                    stop = start + 1;
                    actions.push((a, vec![argument], option));
                    break;
                } else {
                    return Err(self.error(
                        Some(a),
                        format!("ignored explicit argument {}", python::repr(&argument)),
                    ));
                }
            } else {
                let from = start + 1;
                let count = self.match_argument(a, &self.pattern[from..])?;
                stop = from + count;
                actions.push((a, self.args[from..stop].to_vec(), option));
                break;
            }
        }
        for (a, strings, _) in actions {
            self.take_action(a, strings)?;
        }
        Ok(stop)
    }

    /// `_get_value`: the action's type applied to one string.
    fn get_value(&self, action: usize, text: &str) -> Result<Val, Flow> {
        let (value, name) = match self.parser.actions[action].ty {
            None => return Ok(Val::Str(text.to_string())),
            Some(Type::Int) => (python::int(text).map(Val::Int), "int"),
            Some(Type::Float) => (python::float(text).map(Val::Float), "float"),
        };
        value.ok_or_else(|| {
            self.error(
                Some(action),
                format!("invalid {name} value: {}", python::repr(text)),
            )
        })
    }

    /// `_check_value`: the choices, with 3.15's suggestion of the closest one.
    fn check_value(&self, action: usize, value: &Val) -> Result<(), Flow> {
        let Some(choices) = &self.parser.actions[action].choices else {
            return Ok(());
        };
        if choices.iter().any(|c| c.equals(value)) {
            return Ok(());
        }
        let listed: Vec<String> = choices.iter().map(|c| python::repr(&c.text())).collect();
        let listed = listed.join(", ");
        let shown = python::repr(&value.text());
        if let Val::Str(word) = value {
            let names: Option<Vec<&str>> = choices
                .iter()
                .map(|c| match c {
                    Val::Str(s) => Some(s.as_str()),
                    _ => None,
                })
                .collect();
            if let Some(closest) = names
                .filter(|_| self.parser.suggest_on_error)
                .and_then(|names| python::close_match(word, &names))
            {
                return Err(self.error(
                    Some(action),
                    format!(
                        "invalid choice: {shown}, maybe you meant {}? (choose from {listed})",
                        python::repr(closest)
                    ),
                ));
            }
        }
        Err(self.error(
            Some(action),
            format!("invalid choice: {shown} (choose from {listed})"),
        ))
    }

    /// `_get_values`.
    fn get_values(&self, action: usize, strings: &[String]) -> Result<Val, Flow> {
        let a = &self.parser.actions[action];
        Ok(match (a.nargs, strings) {
            (Nargs::Optional, []) => {
                let value = if a.options.is_empty() {
                    &a.default
                } else {
                    &a.constant
                };
                match value {
                    Val::Str(s) => self.get_value(action, s)?,
                    other => other.clone(),
                }
            }
            (Nargs::ZeroOrMore, []) if a.options.is_empty() => match &a.default {
                Val::None => Val::List(Vec::new()),
                other => other.clone(),
            },
            (Nargs::One | Nargs::Optional, [one]) => {
                let value = self.get_value(action, one)?;
                self.check_value(action, &value)?;
                value
            }
            (Nargs::Remainder, _) => Val::List(
                strings
                    .iter()
                    .map(|s| self.get_value(action, s))
                    .collect::<Result<_, _>>()?,
            ),
            (Nargs::Parser, _) => {
                let values: Vec<Val> = strings
                    .iter()
                    .map(|s| self.get_value(action, s))
                    .collect::<Result<_, _>>()?;
                self.check_value(action, &values[0])?;
                Val::List(values)
            }
            _ => {
                let values: Vec<Val> = strings
                    .iter()
                    .map(|s| self.get_value(action, s))
                    .collect::<Result<_, _>>()?;
                for v in &values {
                    self.check_value(action, v)?;
                }
                Val::List(values)
            }
        })
    }

    fn take_action(&mut self, action: usize, strings: Vec<String>) -> Result<(), Flow> {
        self.seen.insert(action);
        let value = self.get_values(action, &strings)?;
        if value == Val::Suppress {
            return Ok(());
        }
        let a = &self.parser.actions[action];
        let dest = a.dest.unwrap_or_default();
        match a.kind {
            Kind::Help => return Err(Flow::Stop(Stop::Help(self.p))),
            Kind::Store => set(&mut self.namespace, dest, value.clone()),
            Kind::StoreTrue => set(&mut self.namespace, dest, a.constant.clone()),
            Kind::Append => {
                let mut items = match get(&self.namespace, dest) {
                    Some(Val::List(items)) => items.clone(),
                    _ => Vec::new(),
                };
                items.push(value.clone());
                set(&mut self.namespace, dest, Val::List(items));
            }
            Kind::Parsers => {
                let Val::List(values) = &value else {
                    unreachable!("parser values")
                };
                let name = values[0].text();
                set(&mut self.namespace, dest, Val::Str(name.clone()));
                let index = self
                    .engine
                    .names
                    .iter()
                    .position(|n| *n == name)
                    .expect("a checked choice")
                    + 1;
                let rest: Vec<String> = values[1..].iter().map(Val::text).collect();
                let known = self.engine.parse_known(index, &rest).map_err(Flow::Stop)?;
                for (key, v) in known.namespace {
                    set(&mut self.namespace, &key, v);
                }
                self.unrecognized.extend(known.extras);
                self.sub = Some((index, known.taken));
            }
        }
        self.taken.push(Taken {
            action,
            value,
            strings: strings.len(),
        });
        Ok(())
    }
}

/// Parse `main.py`'s arguments as cli.py's parser does.
pub fn parse(args: &[&str]) -> Outcome {
    Engine::load().parse(args)
}

/// `parser.error(message)` of main.py's parser (cli.py's own refusals after parsing, e.g. `status`
/// `--interval`): the text for stderr; exit 2.
#[allow(dead_code)]
pub fn error_text(message: &str) -> String {
    Engine::load().error_text(0, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(args: &[&str]) -> Parsed {
        match parse(args) {
            Outcome::Parsed(p) => p,
            other => panic!("{args:?}: {other:?}"),
        }
    }

    fn error(args: &[&str]) -> String {
        match parse(args) {
            Outcome::Error(e) => e,
            other => panic!("{args:?}: {other:?}"),
        }
    }

    fn value<'p>(p: &'p Parsed, dest: &str) -> &'p Val {
        get(&p.namespace, dest).expect(dest)
    }

    #[test]
    fn canonical_forms() {
        for (args, canonical) in [
            (
                &["status", "--reborn", "--pss"][..],
                &["status", "--pss", "--reborn"][..],
            ),
            (
                &["status", "--re", "cpu", "--pid=7", "--pid", " 0_8"],
                &["status", "cpu", "--pid", "7", "--pid", "8", "--reborn"],
            ),
            (
                &["status", "--interval", "1e-1", "--interval", "inf"],
                &["status", "--interval", "inf"],
            ),
            (&["capabilities", "--js"], &["capabilities", "--json"]),
            (&["boot-stage", "--", "-x"], &["boot-stage", "-x"]),
            (&["rtc-alarm", "-5"], &["rtc-alarm", "-5"]),
            (
                &["network-check", "--seconds", "10", "--peer=h"],
                &["network-check", "--peer", "h", "--seconds", "10"],
            ),
            (
                &["update", "check", "--url=-x"],
                &["update", "check", "--url", "-x"],
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
        ] {
            assert_eq!(parsed(args).canonical, canonical, "{args:?}");
        }
    }

    #[test]
    fn namespace_values() {
        let p = parsed(&["status"]);
        assert_eq!(value(&p, "command"), &Val::Str("status".into()));
        assert_eq!(value(&p, "section"), &Val::None);
        assert_eq!(value(&p, "pid"), &Val::List(vec![]));
        assert_eq!(value(&p, "interval"), &Val::Int("0".into()));
        assert_eq!(get(&p.namespace, "help"), None);
        let p = parsed(&["collect", "--workload", "x", "--pid", "1", "--pid", "2"]);
        assert_eq!(value(&p, "workload"), &Val::Str("x".into()));
        assert_eq!(
            value(&p, "pid"),
            &Val::List(vec![Val::Int("1".into()), Val::Int("2".into())])
        );
        let p = parsed(&["shutdown", "reboot"]);
        assert_eq!(value(&p, "reason"), &Val::Str("user".into()));
    }

    #[test]
    fn errors_are_argparses() {
        assert_eq!(
            error(&[]),
            format!(
                "{}main.py: error: the following arguments are required: command\n",
                Engine::load().parser(0).usage
            )
        );
        let e = error(&["sleep", "stats"]);
        assert!(
            e.starts_with("usage: main.py sleep [-h] {request,status,prepare}\n"),
            "{e}"
        );
        assert!(e.ends_with("main.py sleep: error: argument action: invalid choice: 'stats', maybe you meant 'status'? (choose from 'request', 'status', 'prepare')\n"), "{e}");
        assert!(error(&["internal-media", "setup", "--confirm"]).ends_with(
            "error: ambiguous option: --confirm could match --confirm-serial, --confirm-shown\n"
        ));
        assert!(error(&["status", "--pss=1"])
            .ends_with("error: argument --pss: ignored explicit argument '1'\n"));
        assert!(
            error(&["status", "--pid"]).ends_with("error: argument --pid: expected one argument\n")
        );
        assert!(error(&["status", "--pid", "x"])
            .ends_with("error: argument --pid: invalid int value: 'x'\n"));
        assert!(error(&["status", "--interval", "1_"])
            .ends_with("error: argument --interval: invalid float value: '1_'\n"));
        assert!(error(&["bench-library", "--tracks", "5"]).ends_with(
            "error: argument --tracks: invalid choice: '5' (choose from '1000', '10000', '20000')\n"
        ));
        assert!(error(&["time", "x"]).starts_with("usage: main.py [-h]\n"));
        assert!(error(&["time", "x"]).ends_with("main.py: error: unrecognized arguments: x\n"));
        assert!(error(&["network-check"])
            .ends_with("error: the following arguments are required: --peer\n"));
        assert!(error(&["boot-stage", "-x"])
            .ends_with("main.py boot-stage: error: the following arguments are required: stage\n"));
    }

    #[test]
    fn help_anywhere() {
        let engine = Engine::load();
        let status = engine.names.iter().position(|n| *n == "status").unwrap() + 1;
        for args in [
            &["status", "--pss", "-h"][..],
            &["status", "--he"],
            &["status", "-hx"],
            &["status", "cpu", "--help", "--pid", "x"],
        ] {
            assert_eq!(
                parse(args),
                Outcome::Help(engine.parser(status).help.clone()),
                "{args:?}"
            );
        }
        assert_eq!(
            parse(&["-h", "nonsense"]),
            Outcome::Help(engine.parser(0).help.clone())
        );
        assert!(matches!(parse(&["status", "-h=x"]), Outcome::Error(_)));
        assert!(matches!(
            parse(&["status", "--pid", "x", "-h"]),
            Outcome::Error(_)
        ));
    }

    #[test]
    fn every_subcommand_line_loads() {
        let engine = Engine::load();
        assert_eq!(engine.names.len(), SPEC.lines().count() - 3);
        for (i, name) in engine.names.iter().enumerate() {
            assert_eq!(engine.parser(i + 1).prog, format!("main.py {name}"));
        }
    }

    #[test]
    fn nargs_patterns_backtrack_like_re() {
        let mut atoms = Vec::new();
        nargs_atoms(Nargs::One, false, 0, &mut atoms);
        nargs_atoms(Nargs::Optional, false, 1, &mut atoms);
        assert_eq!(match_atoms(&atoms, 2, b"A-A"), Some((vec![2, 1], 3)));
        assert_eq!(match_atoms(&atoms, 2, b"AO"), Some((vec![1, 0], 1)));
        assert_eq!(match_atoms(&atoms, 2, b"O"), None);
    }
}
