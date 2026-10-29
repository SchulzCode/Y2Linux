//! The Python operations report.py and maintenance.py apply to parsed JSON, with their exceptions: a
//! record of an unexpected shape raises where Python raises (an uncaught `TypeError`, `KeyError` or
//! `AttributeError` is main.py's traceback, `Failure::Crash`), after the same changes.
// SPDX-License-Identifier: GPL-2.0-only
use super::pyjson::Py;
use crate::reply::Failure;

/// `type(value).__name__`.
pub fn type_name(value: &Py) -> &'static str {
    match value {
        Py::None => "NoneType",
        Py::Bool(_) => "bool",
        Py::Int(_) => "int",
        Py::Float(_) => "float",
        Py::Str(_) => "str",
        Py::List(_) => "list",
        Py::Dict(_) => "dict",
    }
}

/// `value.<attribute>` of a value that is no dict (`AttributeError`).
pub fn no_attribute(value: &Py, attribute: &str) -> Failure {
    Failure::attribute_error(format!(
        "'{}' object has no attribute '{attribute}'",
        type_name(value)
    ))
}

/// `value.get(key)` (`value.get(key, default)` is this or the default): a dict's entry, an
/// `AttributeError` for anything else.
pub fn get<'a>(value: &'a Py, key: &str) -> Result<Option<&'a Py>, Failure> {
    match value {
        Py::Dict(_) => Ok(value.get(key)),
        other => Err(no_attribute(other, "get")),
    }
}

/// `value[key]` with a str key.
pub fn index<'a>(value: &'a Py, key: &str) -> Result<&'a Py, Failure> {
    match value {
        Py::Dict(_) => value.get(key).ok_or_else(|| Failure::key_error(key)),
        Py::Str(_) => Err(Failure::type_error(
            "string indices must be integers, not 'str'",
        )),
        Py::List(_) => Err(Failure::type_error(
            "list indices must be integers or slices, not str",
        )),
        other => Err(Failure::type_error(format!(
            "'{}' object is not subscriptable",
            type_name(other)
        ))),
    }
}

/// `item in container` for a str item.
pub fn contains(container: &Py, item: &str) -> Result<bool, Failure> {
    match container {
        Py::List(items) => Ok(items.iter().any(|v| v.py_eq(&Py::str(item)))),
        Py::Dict(_) => Ok(container.get(item).is_some()),
        Py::Str(text) => Ok(text.contains(item)),
        other => Err(Failure::type_error(format!(
            "argument of type '{}' is not a container or iterable",
            type_name(other)
        ))),
    }
}

/// `for item in value`: a list's items, a str's characters, a dict's keys.
pub fn iterate(value: &Py) -> Result<Vec<Py>, Failure> {
    match value {
        Py::List(items) => Ok(items.clone()),
        Py::Str(text) => Ok(text.chars().map(|c| Py::Str(c.to_string())).collect()),
        Py::Dict(items) => Ok(items.iter().map(|(k, _)| Py::Str(k.clone())).collect()),
        other => Err(Failure::type_error(format!(
            "'{}' object is not iterable",
            type_name(other)
        ))),
    }
}

/// A number's value for an ordering comparison `left <op> right` (Python compares int, float and
/// bool with each other and refuses every other pair).
pub fn ordered(left: &Py, op: &str, right: &Py) -> Result<(f64, f64), Failure> {
    match (left.number(), right.number()) {
        (Some(a), Some(b)) => Ok((a, b)),
        _ => Err(Failure::type_error(format!(
            "'{op}' not supported between instances of '{}' and '{}'",
            type_name(left),
            type_name(right)
        ))),
    }
}

/// `value + n` for an int `n`.
pub fn add_int(value: &Py, n: i64) -> Result<Py, Failure> {
    match value {
        Py::Bool(b) => Ok(Py::int(i64::from(*b) + n)),
        Py::Int(digits) => Ok(match digits.parse::<i128>() {
            Ok(v) => Py::Int((v + i128::from(n)).to_string()),
            Err(_) => Py::Int(digits.clone()),
        }),
        Py::Float(f) => Ok(Py::Float(f + n as f64)),
        Py::Str(_) => Err(Failure::type_error(
            "can only concatenate str (not \"int\") to str",
        )),
        Py::List(_) => Err(Failure::type_error(
            "can only concatenate list (not \"int\") to list",
        )),
        other => Err(Failure::type_error(format!(
            "unsupported operand type(s) for +: '{}' and 'int'",
            type_name(other)
        ))),
    }
}

/// `value + items` for a list `items`.
pub fn add_list(value: &Py, items: Vec<Py>) -> Result<Py, Failure> {
    match value {
        Py::List(first) => Ok(Py::List(first.iter().cloned().chain(items).collect())),
        Py::Str(_) => Err(Failure::type_error(
            "can only concatenate str (not \"list\") to str",
        )),
        other => Err(Failure::type_error(format!(
            "unsupported operand type(s) for +: '{}' and 'list'",
            type_name(other)
        ))),
    }
}

/// `dict(value)` of a parsed JSON value (`dict(x or {})`: a falsy value is `{}`): a dict as it is, a
/// list of two-item pairs (str or list) as its pairs; the `ValueError` of a pair of another length,
/// the `TypeError` of anything else. A pair whose key is no str is outside what the port reads
/// (Reborn writes a dict): a `TypeError` here.
pub fn to_dict(value: &Py) -> Result<Py, Failure> {
    let length = |n: usize, at: usize| {
        Failure::value(format!(
            "dictionary update sequence element #{at} has length {n}; 2 is required"
        ))
    };
    match value {
        Py::Dict(_) => Ok(value.clone()),
        v if !v.truthy() => Ok(Py::dict()),
        Py::Str(text) => Err(length(text.chars().next().map_or(0, |_| 1), 0)),
        Py::List(items) => {
            let mut out = Py::dict();
            for (at, item) in items.iter().enumerate() {
                match item {
                    Py::Str(pair) => {
                        let chars: Vec<char> = pair.chars().collect();
                        if chars.len() != 2 {
                            return Err(length(chars.len(), at));
                        }
                        out.set(&chars[0].to_string(), Py::Str(chars[1].to_string()));
                    }
                    Py::List(pair) if pair.len() != 2 => return Err(length(pair.len(), at)),
                    Py::List(pair) => match &pair[0] {
                        Py::Str(key) => out.set(key, pair[1].clone()),
                        key @ (Py::List(_) | Py::Dict(_)) => {
                            return Err(Failure::type_error(format!(
                                "unhashable type: '{}'",
                                type_name(key)
                            )))
                        }
                        key => {
                            return Err(Failure::type_error(format!(
                                "a settings key of type '{}' is not read",
                                type_name(key)
                            )))
                        }
                    },
                    Py::Dict(pair) if pair.len() != 2 => return Err(length(pair.len(), at)),
                    Py::Dict(pair) => out.set(&pair[0].0, Py::Str(pair[1].0.clone())),
                    _ => {
                        return Err(Failure::type_error(format!(
                            "cannot convert dictionary update sequence element #{at} to a sequence"
                        )))
                    }
                }
            }
            Ok(out)
        }
        other => Err(Failure::type_error(format!(
            "'{}' object is not iterable",
            type_name(other)
        ))),
    }
}

/// `json.loads(raw)` that failed: Python's `JSONDecodeError` or `UnicodeDecodeError` (both
/// `ValueError`s) with CPython's text.
pub fn json_error(raw: &[u8]) -> Failure {
    match crate::commands::suspend::pyjson::loads(raw) {
        Err(error) => Failure::raised(error.class, error.text),
        Ok(_) => Failure::raised(
            "JSONDecodeError",
            "Expecting value: line 1 column 1 (char 0)",
        ),
    }
}

/// `json.loads(raw)`: the value, or Python's exception.
pub fn loads(raw: &[u8]) -> Result<Py, Failure> {
    super::pyjson::loads_bytes(raw).ok_or_else(|| json_error(raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::reports::pyjson::loads as parse;

    fn crash(result: Result<impl std::fmt::Debug, Failure>) -> String {
        match result {
            Err(Failure::Crash { class, message }) => format!("{class}: {message}"),
            Err(other) => format!("caught: {}", other.message()),
            Ok(value) => format!("ok: {value:?}"),
        }
    }

    #[test]
    fn exceptions_read_as_pythons() {
        // python3 -c for each expression (3.14).
        let p = |t: &str| parse(t).unwrap();
        assert_eq!(
            crash(get(&p("[]"), "x")),
            "AttributeError: 'list' object has no attribute 'get'"
        );
        assert_eq!(crash(index(&p("{}"), "path")), "KeyError: 'path'");
        assert_eq!(
            crash(index(&p("\"ab\""), "path")),
            "TypeError: string indices must be integers, not 'str'"
        );
        assert_eq!(crash(contains(&p("\"a,b\""), "a")), "ok: true");
        assert_eq!(
            crash(contains(&p("5"), "a")),
            "TypeError: argument of type 'int' is not a container or iterable"
        );
        assert_eq!(
            crash(ordered(&p("\"x\""), ">=", &Py::int(5))),
            "TypeError: '>=' not supported between instances of 'str' and 'int'"
        );
        assert_eq!(crash(add_int(&p("true"), 1)), "ok: Int(\"2\")");
        assert_eq!(
            crash(add_int(&p("null"), 1)),
            "TypeError: unsupported operand type(s) for +: 'NoneType' and 'int'"
        );
        assert_eq!(
            crash(add_list(&p("\"x\""), vec![])),
            "TypeError: can only concatenate str (not \"list\") to str"
        );
        assert_eq!(
            crash(to_dict(&p("\"ab\""))),
            "caught: dictionary update sequence element #0 has length 1; 2 is required"
        );
        assert_eq!(
            crash(to_dict(&p("[\"ab\"]"))),
            "ok: Dict([(\"a\", Str(\"b\"))])"
        );
        assert_eq!(
            crash(to_dict(&p("5"))),
            "TypeError: 'int' object is not iterable"
        );
        assert_eq!(
            json_error(b"{").message(),
            "Expecting property name enclosed in double quotes: line 1 column 2 (char 1)"
        );
    }
}
