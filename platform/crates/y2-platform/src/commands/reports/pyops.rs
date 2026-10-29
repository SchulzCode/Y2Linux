//! The operations report.py and maintenance.py apply to parsed JSON records, taking the shapes Python
//! takes (`in` on a str is a substring test, a dict iterates its keys, a bool adds as 0/1): a record of
//! another shape is refused with the caller's reason where Python died with a traceback.
// SPDX-License-Identifier: GPL-2.0-only
use super::pyjson::Py;
use crate::reply::Failure;

/// A value of another shape than the operation takes (where Python raised a TypeError, KeyError or
/// AttributeError): the caller refuses with the name of the record it read (`or_refuse`).
#[derive(Debug, PartialEq)]
pub struct Shape;

pub trait OrRefuse<T> {
    /// `Failure::value(reason)` for a value of the wrong shape.
    fn or_refuse(self, reason: &'static str) -> Result<T, Failure>;
}

impl<T> OrRefuse<T> for Result<T, Shape> {
    fn or_refuse(self, reason: &'static str) -> Result<T, Failure> {
        self.map_err(|Shape| Failure::value(reason))
    }
}

/// `value.get(key)` (`value.get(key, default)` is this or the default): a dict's entry; anything
/// else has no `get`.
pub fn get<'a>(value: &'a Py, key: &str) -> Result<Option<&'a Py>, Shape> {
    match value {
        Py::Dict(_) => Ok(value.get(key)),
        _ => Err(Shape),
    }
}

/// `value[key]` with a str key: a dict's entry that is there.
pub fn index<'a>(value: &'a Py, key: &str) -> Result<&'a Py, Shape> {
    value.get(key).ok_or(Shape)
}

/// `item in container` for a str item: a list's equal item, a dict's key, a str's substring.
pub fn contains(container: &Py, item: &str) -> Result<bool, Shape> {
    match container {
        Py::List(items) => Ok(items.iter().any(|v| v.py_eq(&Py::str(item)))),
        Py::Dict(_) => Ok(container.get(item).is_some()),
        Py::Str(text) => Ok(text.contains(item)),
        _ => Err(Shape),
    }
}

/// `for item in value`: a list's items, a str's characters, a dict's keys.
pub fn iterate(value: &Py) -> Result<Vec<Py>, Shape> {
    match value {
        Py::List(items) => Ok(items.clone()),
        Py::Str(text) => Ok(text.chars().map(|c| Py::Str(c.to_string())).collect()),
        Py::Dict(items) => Ok(items.iter().map(|(k, _)| Py::Str(k.clone())).collect()),
        _ => Err(Shape),
    }
}

/// Two values an ordering comparison takes: numbers (int, float and bool compare with each other).
pub fn ordered(left: &Py, right: &Py) -> Result<(f64, f64), Shape> {
    left.number().zip(right.number()).ok_or(Shape)
}

/// `value + n` for an int `n`: an int stays an int (of any size), a float a float, a bool adds as 0/1.
pub fn add_int(value: &Py, n: i64) -> Result<Py, Shape> {
    match value {
        Py::Bool(b) => Ok(Py::int(i64::from(*b) + n)),
        Py::Int(digits) => Ok(match digits.parse::<i128>() {
            Ok(v) => Py::Int((v + i128::from(n)).to_string()),
            Err(_) => Py::Int(digits.clone()),
        }),
        Py::Float(f) => Ok(Py::Float(f + n as f64)),
        _ => Err(Shape),
    }
}

/// `value + items` for a list `items`.
pub fn add_list(value: &Py, items: Vec<Py>) -> Result<Py, Shape> {
    match value {
        Py::List(first) => Ok(Py::List(first.iter().cloned().chain(items).collect())),
        _ => Err(Shape),
    }
}

/// `dict(value)` of a parsed JSON value (`dict(x or {})`: a falsy value is `{}`): a dict as it is, a
/// list of two-item pairs (str or list) as its pairs. A pair of another length is Python's
/// `ValueError` (its text is the caller's answer); anything else, and a pair whose key is no str
/// (Reborn writes a dict), is refused as `reason`.
pub fn to_dict(value: &Py, reason: &'static str) -> Result<Py, Failure> {
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
                        _ => return Err(Failure::value(reason)),
                    },
                    Py::Dict(pair) if pair.len() != 2 => return Err(length(pair.len(), at)),
                    Py::Dict(pair) => out.set(&pair[0].0, Py::Str(pair[1].0.clone())),
                    _ => return Err(Failure::value(reason)),
                }
            }
            Ok(out)
        }
        _ => Err(Failure::value(reason)),
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

    #[test]
    fn operations_take_what_python_takes() {
        let p = |t: &str| parse(t).unwrap();
        assert_eq!(get(&p("[]"), "x"), Err(Shape));
        assert_eq!(get(&p(r#"{"x": 1}"#), "x"), Ok(Some(&Py::int(1))));
        assert_eq!(index(&p("{}"), "path"), Err(Shape));
        assert_eq!(index(&p(r#""ab""#), "path"), Err(Shape));
        assert_eq!(contains(&p(r#""a,b""#), "a"), Ok(true));
        assert_eq!(contains(&p("5"), "a"), Err(Shape));
        assert_eq!(ordered(&p(r#""x""#), &Py::int(5)), Err(Shape));
        assert_eq!(ordered(&p("true"), &Py::Float(0.5)), Ok((1.0, 0.5)));
        assert_eq!(add_int(&p("true"), 1), Ok(Py::Int("2".into())));
        assert_eq!(
            add_int(&p("123456789012345678901234567890"), 1),
            Ok(Py::Int("123456789012345678901234567891".into()))
        );
        assert_eq!(add_int(&p("null"), 1), Err(Shape));
        assert_eq!(add_list(&p(r#""x""#), vec![]), Err(Shape));
        let answer = |r: Result<Py, Failure>| r.map_err(|e| e.message());
        assert_eq!(
            answer(to_dict(&p(r#""ab""#), "bad")),
            Err("dictionary update sequence element #0 has length 1; 2 is required".into())
        );
        assert_eq!(
            answer(to_dict(&p(r#"["ab"]"#), "bad")),
            Ok(Py::dict().with("a", Py::str("b")))
        );
        assert_eq!(answer(to_dict(&p("5"), "bad")), Err("bad".into()));
        assert_eq!(answer(to_dict(&p("[[1, 2]]"), "bad")), Err("bad".into()));
        assert_eq!(
            json_error(b"{").message(),
            "Expecting property name enclosed in double quotes: line 1 column 2 (char 1)"
        );
    }
}
