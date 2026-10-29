//! IPv4 addresses, prefixes and networks from udhcpc's variables, parsed as Python 3.15's
//! `ipaddress` does (the forms it takes and the exact texts of its errors, which dhcp-event prints).
// SPDX-License-Identifier: GPL-2.0-only
use crate::reply::Failure;

fn address_error(text: String) -> Failure {
    Failure::raised("AddressValueError", text)
}

/// `%r` in ipaddress's messages.
fn repr(text: &str) -> String {
    crate::argparse::python::repr(text)
}

/// `str.isascii() and str.isdigit()`.
fn ascii_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// `_BaseV4._parse_octet`: the ValueError's text.
fn parse_octet(octet: &str) -> Result<u8, String> {
    if octet.is_empty() {
        return Err("Empty octet not permitted".into());
    }
    if !ascii_digits(octet) {
        return Err(format!("Only decimal digits permitted in {}", repr(octet)));
    }
    if octet.len() > 3 {
        return Err(format!("At most 3 characters permitted in {}", repr(octet)));
    }
    if octet != "0" && octet.starts_with('0') {
        return Err(format!(
            "Leading zeros are not permitted in {}",
            repr(octet)
        ));
    }
    // At most three ASCII digits (checked above).
    let value = octet
        .bytes()
        .fold(0u32, |value, digit| value * 10 + u32::from(digit - b'0'));
    u8::try_from(value).map_err(|_| format!("Octet {value} (> 255) not permitted"))
}

/// `_BaseV4._ip_int_from_string`: AddressValueError.
fn ip_int(text: &str) -> Result<u32, Failure> {
    if text.is_empty() {
        return Err(address_error("Address cannot be empty".into()));
    }
    let parts: Vec<&str> = text.split('.').collect();
    if parts.len() != 4 {
        return Err(address_error(format!(
            "Expected 4 octets in {}",
            repr(text)
        )));
    }
    let mut octets = [0u8; 4];
    for (octet, part) in octets.iter_mut().zip(parts) {
        *octet = parse_octet(part)
            .map_err(|error| address_error(format!("{error} in {}", repr(text))))?;
    }
    Ok(u32::from_be_bytes(octets))
}

/// `ipaddress.IPv4Address(text)` of a str.
pub(super) fn ipv4(text: &str) -> Result<[u8; 4], Failure> {
    if text.contains('/') {
        return Err(address_error(format!("Unexpected '/' in {}", repr(text))));
    }
    ip_int(text).map(u32::to_be_bytes)
}

pub(super) fn dotted(octets: [u8; 4]) -> String {
    format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3])
}

/// `_BaseNetwork._prefix_from_ip_int`: a netmask's length, None when it mixes zeroes and ones.
fn prefix_from_int(mask: u32) -> Option<u32> {
    let zeroes = if mask == 0 { 32 } else { mask.trailing_zeros() };
    let length = 32 - zeroes;
    let ones = u64::from(mask) >> zeroes;
    (ones == (1u64 << length) - 1).then_some(length)
}

/// `_BaseV4._make_netmask(text)` of a str: a length (ASCII digits, `0..=32`), else a netmask, else a
/// hostmask (`0.0.0.255` is /24); anything else is a NetmaskValueError naming the text.
fn prefix(text: &str) -> Result<u32, Failure> {
    if ascii_digits(text) {
        // Out of range (any length of digits): the dotted forms below refuse it too.
        if let Some(length) = text.parse::<u64>().ok().filter(|l| *l <= 32) {
            return Ok(length as u32);
        }
    }
    ip_int(text)
        .ok()
        .and_then(|mask| prefix_from_int(mask).or_else(|| prefix_from_int(!mask)))
        .ok_or_else(|| {
            Failure::raised(
                "NetmaskValueError",
                format!("{} is not a valid netmask", repr(text)),
            )
        })
}

/// `_split_optional_netmask`: at most one '/'.
fn split_prefix(text: &str) -> Result<(&str, Option<&str>), Failure> {
    let mut parts = text.split('/');
    let address = parts.next().unwrap_or_default();
    let mask = parts.next();
    if parts.next().is_some() {
        return Err(address_error(format!(
            "Only one '/' permitted in {}",
            repr(text)
        )));
    }
    Ok((address, mask))
}

/// `ipaddress.IPv4Interface(text)`: the prefix length.
pub(super) fn interface(text: &str) -> Result<u32, Failure> {
    let (address, mask) = split_prefix(text)?;
    ipv4(address)?;
    mask.map_or(Ok(32), prefix)
}

/// `str(ipaddress.IPv4Network(text, strict=False))`.
pub(super) fn network(text: &str) -> Result<String, Failure> {
    let (address, mask) = split_prefix(text)?;
    let address = ipv4(address)?;
    let length = mask.map_or(Ok(32), prefix)?;
    let mask = if length == 0 {
        0
    } else {
        u32::MAX << (32 - length)
    };
    Ok(format!(
        "{}/{length}",
        dotted((u32::from_be_bytes(address) & mask).to_be_bytes())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_parse_as_ipaddress_does() {
        assert_eq!(dotted(ipv4("192.168.1.40").unwrap()), "192.168.1.40");
        fn error<T: std::fmt::Debug>(result: Result<T, Failure>) -> String {
            match result.unwrap_err() {
                Failure::Os { class, text } => format!("{class}: {text}"),
                other => panic!("{other:?}"),
            }
        }
        for (bad, text) in [
            (
                "01.2.3.4",
                "Leading zeros are not permitted in '01' in '01.2.3.4'",
            ),
            ("1.2.3", "Expected 4 octets in '1.2.3'"),
            (
                "256.1.1.1",
                "Octet 256 (> 255) not permitted in '256.1.1.1'",
            ),
            ("", "Address cannot be empty"),
            ("1.2.3.4.5", "Expected 4 octets in '1.2.3.4.5'"),
            (
                " 1.2.3.4",
                "Only decimal digits permitted in ' 1' in ' 1.2.3.4'",
            ),
            ("1..2.3", "Empty octet not permitted in '1..2.3'"),
            (
                "1.2.3.1234",
                "At most 3 characters permitted in '1234' in '1.2.3.1234'",
            ),
            (
                "1.2.3.\u{663}",
                "Only decimal digits permitted in '\u{663}' in '1.2.3.\u{663}'",
            ),
            ("1.2.3.4/5", "Unexpected '/' in '1.2.3.4/5'"),
        ] {
            assert_eq!(error(ipv4(bad)), format!("AddressValueError: {text}"));
        }
        assert_eq!(network("10.1.2.3/8").unwrap(), "10.0.0.0/8");
        assert_eq!(network("0.0.0.0/0").unwrap(), "0.0.0.0/0");
        assert_eq!(network("192.168.5.1").unwrap(), "192.168.5.1/32");
        assert_eq!(network("10.1.2.3/0.255.255.255").unwrap(), "10.0.0.0/8");
        assert_eq!(prefix("255.255.255.0").unwrap(), 24);
        assert_eq!(prefix("255.255.255.255").unwrap(), 32);
        assert_eq!(prefix("0.0.0.0").unwrap(), 0);
        assert_eq!(prefix("0.0.0.255").unwrap(), 24);
        assert_eq!(prefix("024").unwrap(), 24);
        for bad in ["255.0.255.0", "33", "", "+8", "99999999999999999999", "x"] {
            assert_eq!(
                error(prefix(bad)),
                format!("NetmaskValueError: '{bad}' is not a valid netmask")
            );
        }
        assert_eq!(
            error(interface("1.2.3.4/1/2")),
            "AddressValueError: Only one '/' permitted in '1.2.3.4/1/2'"
        );
        assert_eq!(
            error(network("1.2.3.4/")),
            "NetmaskValueError: '' is not a valid netmask"
        );
        assert_eq!(network("10.1.2.3/255.0.0.0").unwrap(), "10.0.0.0/8");
    }
}
