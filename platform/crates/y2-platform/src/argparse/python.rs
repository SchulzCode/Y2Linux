//! The Python builtins argparse calls while parsing: `int()` and `float()` of a str (the types cli.py names),
//! `repr()` of a str (in error messages), re `\d` (argparse's negative-number test) and
//! `difflib.get_close_matches(word, choices, 1)` (3.15's "maybe you meant"). Unicode properties are the
//! player's Python's (unicode.rs, generated).
// SPDX-License-Identifier: GPL-2.0-only
use super::unicode::{DECIMAL_ZEROS, NONPRINTABLE, SPACE};

/// `unicodedata.decimal(c)`: the digit of a Unicode decimal digit (Nd).
pub fn decimal(c: char) -> Option<u32> {
    let c = c as u32;
    let i = DECIMAL_ZEROS.partition_point(|&zero| zero <= c);
    let zero = *DECIMAL_ZEROS.get(i.checked_sub(1)?)?;
    (c - zero < 10).then_some(c - zero)
}

/// `str.isspace()` of one character.
fn is_space(c: char) -> bool {
    SPACE.binary_search(&(c as u32)).is_ok()
}

/// `str.isprintable()` of one character.
fn is_printable(c: char) -> bool {
    let c = c as u32;
    let i = NONPRINTABLE.partition_point(|&(first, _)| first <= c);
    i == 0 || NONPRINTABLE[i - 1].1 < c
}

/// `repr(text)`.
pub fn repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for c in text.chars() {
        let code = c as u32;
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            _ if code < 0x20 || code == 0x7f => out.push_str(&format!("\\x{code:02x}")),
            _ if code < 0x7f || is_printable(c) => out.push(c),
            _ if code <= 0xff => out.push_str(&format!("\\x{code:02x}")),
            _ if code <= 0xffff => out.push_str(&format!("\\u{code:04x}")),
            _ => out.push_str(&format!("\\U{code:08x}")),
        }
    }
    out.push(quote);
    out
}

/// What `int()` and `float()` parse (`_PyUnicode_TransformDecimalAndSpaceToASCII`): ASCII as it is, other
/// whitespace as a space, other decimal digits as their ASCII digit; anything else makes the text invalid.
fn ascii(text: &str) -> Option<Vec<u8>> {
    text.chars()
        .map(|c| match c as u32 {
            code if code < 127 => Some(code as u8),
            _ if is_space(c) => Some(b' '),
            _ => decimal(c).map(|d| b'0' + d as u8),
        })
        .collect()
}

/// `Py_ISSPACE`: the whitespace `PyLong_FromString` and `float()` strip (ASCII only).
fn ascii_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

fn strip(mut bytes: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = bytes {
        if !ascii_space(*first) {
            break;
        }
        bytes = rest;
    }
    while let [rest @ .., last] = bytes {
        if !ascii_space(*last) {
            break;
        }
        bytes = rest;
    }
    bytes
}

/// Python's limit on the digits of an int parsed from text (`sys.get_int_max_str_digits()`, default).
const INT_MAX_STR_DIGITS: usize = 4300;

/// `str(int(text))`, or None where `int(text)` raises ValueError.
pub fn int(text: &str) -> Option<String> {
    let bytes = ascii(text)?;
    let bytes = strip(&bytes);
    let (negative, body) = match bytes.split_first() {
        Some((b'-', rest)) => (true, rest),
        Some((b'+', rest)) => (false, rest),
        _ => (false, bytes),
    };
    // Digits with single underscores between them.
    let mut digits = String::with_capacity(body.len());
    let mut previous = b'_';
    for &b in body {
        match b {
            b'0'..=b'9' => digits.push(b as char),
            b'_' if previous != b'_' => {}
            _ => return None,
        }
        previous = b;
    }
    if digits.is_empty() || previous == b'_' || digits.len() > INT_MAX_STR_DIGITS {
        return None;
    }
    let value = digits.trim_start_matches('0');
    Some(match (value.is_empty(), negative) {
        (true, _) => "0".into(),
        (false, true) => format!("-{value}"),
        (false, false) => value.into(),
    })
}

/// `float(text)`, or None where it raises ValueError.
pub fn float(text: &str) -> Option<f64> {
    let bytes = ascii(text)?;
    // _Py_string_to_number_with_underscores: an underscore only between two digits.
    let mut plain = Vec::with_capacity(bytes.len());
    let mut previous = 0u8;
    for &b in &bytes {
        if b == b'_' {
            if !previous.is_ascii_digit() {
                return None;
            }
        } else {
            if previous == b'_' && !b.is_ascii_digit() {
                return None;
            }
            plain.push(b);
        }
        previous = b;
    }
    if previous == b'_' {
        return None;
    }
    let body = strip(&plain);
    let (negative, unsigned) = match body.split_first() {
        Some((b'-', rest)) => (true, rest),
        Some((b'+', rest)) => (false, rest),
        _ => (false, body),
    };
    let word = unsigned.to_ascii_lowercase();
    let value = if word == b"inf" || word == b"infinity" {
        f64::INFINITY
    } else if word == b"nan" {
        f64::NAN
    } else {
        // digits [. digits] | . digits, then an optional exponent with digits.
        let digits = |from: usize| {
            unsigned[from..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count()
        };
        let mut at = digits(0);
        let mut mantissa = at;
        if unsigned.get(at) == Some(&b'.') {
            let fraction = digits(at + 1);
            mantissa += fraction;
            at += 1 + fraction;
        }
        if mantissa == 0 {
            return None;
        }
        if matches!(unsigned.get(at), Some(b'e' | b'E')) {
            let sign = usize::from(matches!(unsigned.get(at + 1), Some(b'+' | b'-')));
            let exponent = digits(at + 1 + sign);
            if exponent == 0 {
                return None;
            }
            at += 1 + sign + exponent;
        }
        if at != unsigned.len() {
            return None;
        }
        std::str::from_utf8(unsigned).ok()?.parse::<f64>().ok()?
    };
    Some(if negative { -value } else { value })
}

/// `repr(value)` of a float.
pub fn float_repr(value: f64) -> String {
    if value.is_nan() {
        "nan".into()
    } else if value.is_infinite() {
        if value > 0.0 { "inf" } else { "-inf" }.into()
    } else {
        y2p_core::json::float_repr(value)
    }
}

/// The matched length of `difflib.SequenceMatcher(None, a, b)` (autojunk on, as get_close_matches uses).
fn matched(a: &[char], b: &[char]) -> usize {
    use std::collections::HashMap;
    let mut b2j: HashMap<char, Vec<usize>> = HashMap::new();
    for (j, &c) in b.iter().enumerate() {
        b2j.entry(c).or_default().push(j);
    }
    if b.len() >= 200 {
        // Popular elements (more than 1% + 1) are not matched as anchors.
        let limit = b.len() / 100 + 1;
        b2j.retain(|_, indices| indices.len() <= limit);
    }
    let longest = |alo: usize, ahi: usize, blo: usize, bhi: usize| {
        let (mut besti, mut bestj, mut best) = (alo, blo, 0usize);
        let mut j2len: HashMap<usize, usize> = HashMap::new();
        for (i, c) in a.iter().enumerate().take(ahi).skip(alo) {
            let mut next: HashMap<usize, usize> = HashMap::new();
            for &j in b2j.get(c).map(Vec::as_slice).unwrap_or_default() {
                if j < blo {
                    continue;
                }
                if j >= bhi {
                    break;
                }
                let k = j
                    .checked_sub(1)
                    .and_then(|p| j2len.get(&p))
                    .copied()
                    .unwrap_or(0)
                    + 1;
                next.insert(j, k);
                if k > best {
                    (besti, bestj, best) = (i + 1 - k, j + 1 - k, k);
                }
            }
            j2len = next;
        }
        // No junk function: extend over equal elements on both sides (the popular ones included).
        while besti > alo && bestj > blo && a[besti - 1] == b[bestj - 1] {
            (besti, bestj, best) = (besti - 1, bestj - 1, best + 1);
        }
        while besti + best < ahi && bestj + best < bhi && a[besti + best] == b[bestj + best] {
            best += 1;
        }
        (besti, bestj, best)
    };
    let mut total = 0;
    let mut queue = vec![(0, a.len(), 0, b.len())];
    while let Some((alo, ahi, blo, bhi)) = queue.pop() {
        let (i, j, k) = longest(alo, ahi, blo, bhi);
        if k > 0 {
            total += k;
            if alo < i && blo < j {
                queue.push((alo, i, blo, j));
            }
            if i + k < ahi && j + k < bhi {
                queue.push((i + k, ahi, j + k, bhi));
            }
        }
    }
    total
}

/// `difflib.get_close_matches(word, possibilities, n=1, cutoff=0.6)`: the best match with a ratio of at
/// least 0.6, the greater string on equal ratios (`heapq.nlargest` of `(ratio, x)`).
pub fn close_match<'a>(word: &str, possibilities: &[&'a str]) -> Option<&'a str> {
    let b: Vec<char> = word.chars().collect();
    let mut best: Option<(f64, &str)> = None;
    for &x in possibilities {
        let a: Vec<char> = x.chars().collect();
        let length = a.len() + b.len();
        let ratio = if length == 0 {
            1.0
        } else {
            2.0 * matched(&a, &b) as f64 / length as f64
        };
        // real_quick_ratio() and quick_ratio() are upper bounds of ratio(): only ratio() decides.
        if ratio >= 0.6 && best.is_none_or(|(score, y)| (ratio, x) > (score, y)) {
            best = Some((ratio, x));
        }
    }
    best.map(|(_, x)| x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_is_pythons() {
        for (text, value) in [
            ("5", Some("5")),
            (" -0_7\n", Some("-7")),
            ("-0", Some("0")),
            ("007", Some("7")),
            ("+12", Some("12")),
            ("\u{a0}5\u{3000}", Some("5")),
            ("\u{663}", Some("3")),
            ("\u{ff11}\u{ff12}", Some("12")),
            ("\u{663}_\u{663}", Some("33")),
            ("\u{2028}1", Some("1")),
            ("\x0b5\x0c", Some("5")),
            ("99999999999999999999999", Some("99999999999999999999999")),
            ("\x1c5", None),
            ("1__0", None),
            ("_1", None),
            ("1_", None),
            ("+_1", None),
            ("", None),
            (" ", None),
            ("+", None),
            ("1 2", None),
            ("0x10", None),
            ("1e3", None),
            ("1.0", None),
            ("\u{2212}5", None),
        ] {
            assert_eq!(int(text).as_deref(), value, "{text:?}");
        }
        assert!(int(&"1".repeat(4300)).is_some());
        assert!(int(&"1".repeat(4301)).is_none());
        assert!(int(&"0".repeat(4301)).is_none());
    }

    #[test]
    fn float_is_pythons() {
        for (text, value) in [
            ("1.5", Some("1.5")),
            (".5", Some("0.5")),
            ("5.", Some("5.0")),
            ("1E+05", Some("100000.0")),
            ("+.5e-3", Some("0.0005")),
            ("-Infinity", Some("-inf")),
            ("+nAn", Some("nan")),
            ("-nan", Some("nan")),
            ("1_000.5", Some("1000.5")),
            ("1e1_0", Some("10000000000.0")),
            ("\u{661}.\u{665}", Some("1.5")),
            ("1e400", Some("inf")),
            ("1e-400", Some("0.0")),
            ("-0", Some("-0.0")),
            (" 1.5\u{3000}", Some("1.5")),
            (".5_5", Some("0.55")),
            ("1.e5", Some("100000.0")),
            (".", None),
            ("e5", None),
            ("1e", None),
            ("1e+", None),
            ("infinity_", None),
            ("in f", None),
            ("1__0", None),
            ("1_.5", None),
            ("1._5", None),
            ("1_e5", None),
            ("0x10", None),
            ("\x1c1.5", None),
            ("infi", None),
            ("++1", None),
            ("1.2.3", None),
            ("1e5.5", None),
            ("_1.0", None),
            ("1.0_", None),
            (".e5", None),
            ("1.5j", None),
        ] {
            assert_eq!(float(text).map(float_repr).as_deref(), value, "{text:?}");
        }
    }

    #[test]
    fn repr_is_pythons() {
        assert_eq!(repr("a"), "'a'");
        assert_eq!(repr("it's"), "\"it's\"");
        assert_eq!(repr("'\""), "'\\'\"'");
        assert_eq!(repr("\t\n\r\\\x01\x7f"), "'\\t\\n\\r\\\\\\x01\\x7f'");
        assert_eq!(
            repr("\u{a0}\u{e9}\u{200b}\u{2028}"),
            "'\\xa0\u{e9}\\u200b\\u2028'"
        );
        assert_eq!(repr("\u{e0001}\u{1f600}"), "'\\U000e0001\u{1f600}'");
    }

    #[test]
    fn close_match_is_difflibs() {
        let choices = ["status", "health", "stats"];
        assert_eq!(close_match("stauts", &choices), Some("stats"));
        assert_eq!(close_match("helth", &choices), Some("health"));
        assert_eq!(close_match("zzz", &choices), None);
        assert_eq!(close_match("", &["", "a"]), Some(""));
    }
}
