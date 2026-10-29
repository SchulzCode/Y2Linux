//! The kind of an IP address the scrubber keeps in its token (private, link, public), as Python's
//! ipaddress module names it (3.13+); loopback, unspecified and the players' USB network are kept.
// SPDX-License-Identifier: GPL-2.0-only
use super::*;

/// ipaddress's IPv4 properties: (loopback, unspecified, private, link-local).
fn ipv4_props(value: u32) -> (bool, bool, bool, bool) {
    let within = |network: u32, prefix: u32| prefix == 0 || (value ^ network) >> (32 - prefix) == 0;
    let net = |a: u8, b: u8, c: u8, d: u8| u32::from_be_bytes([a, b, c, d]);
    // ipaddress._private_networks (Python 3.13+), minus its exceptions.
    let private = [
        (net(0, 0, 0, 0), 8),
        (net(10, 0, 0, 0), 8),
        (net(127, 0, 0, 0), 8),
        (net(169, 254, 0, 0), 16),
        (net(172, 16, 0, 0), 12),
        (net(192, 0, 0, 0), 24),
        (net(192, 0, 0, 170), 31),
        (net(192, 0, 2, 0), 24),
        (net(192, 168, 0, 0), 16),
        (net(198, 18, 0, 0), 15),
        (net(198, 51, 100, 0), 24),
        (net(203, 0, 113, 0), 24),
        (net(240, 0, 0, 0), 4),
        (net(255, 255, 255, 255), 32),
    ]
    .iter()
    .any(|&(n, p)| within(n, p))
        && !(value == net(192, 0, 0, 9) || value == net(192, 0, 0, 10));
    (
        within(net(127, 0, 0, 0), 8),
        value == 0,
        private,
        within(net(169, 254, 0, 0), 16),
    )
}

/// `ipaddress.IPv4Address(text)` and the kind `_ipv4` names: `None` when kept as it is.
pub fn ipv4_kind(text: &str) -> Option<&'static str> {
    let mut octets = [0u8; 4];
    let parts: Vec<&str> = text.split('.').collect();
    if parts.len() != 4 {
        return None;
    }
    for (octet, part) in octets.iter_mut().zip(&parts) {
        if part.is_empty()
            || part.len() > 3
            || !part.bytes().all(|b| b.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return None;
        }
        *octet = part.parse().ok()?;
    }
    let value = u32::from_be_bytes(octets);
    let (loopback, unspecified, private, link) = ipv4_props(value);
    if loopback || unspecified || value == u32::MAX {
        return None;
    }
    if value >> 8 == u32::from_be_bytes([0, 10, 42, 0]) {
        return None; // the fixed USB network of every Y2Linux player
    }
    Some(if private {
        "private"
    } else if link {
        "link"
    } else {
        "public"
    })
}

/// `ipaddress.IPv6Address(text)` for the forms the pattern finds (hex groups and colons).
pub fn parse_ipv6(text: &str) -> Option<u128> {
    let parts: Vec<&str> = text.split(':').collect();
    if parts.len() < 3 || parts.len() > 9 {
        return None;
    }
    let mut skip = None;
    for (i, part) in parts.iter().enumerate().take(parts.len() - 1).skip(1) {
        if part.is_empty() {
            if skip.is_some() {
                return None;
            }
            skip = Some(i);
        }
    }
    let (hi, lo) = match skip {
        Some(index) => {
            let mut hi = index;
            let mut lo = parts.len() - index - 1;
            if parts[0].is_empty() {
                hi -= 1;
                if hi > 0 {
                    return None;
                }
            }
            if parts[parts.len() - 1].is_empty() {
                lo -= 1;
                if lo > 0 {
                    return None;
                }
            }
            if 8 < hi + lo + 1 {
                return None;
            }
            (hi, lo)
        }
        None => {
            if parts.len() != 8 || parts[0].is_empty() || parts[7].is_empty() {
                return None;
            }
            (8, 0)
        }
    };
    let hextet = |part: &str| -> Option<u128> {
        if part.is_empty() || part.len() > 4 || !part.bytes().all(hex) {
            return None;
        }
        u128::from_str_radix(part, 16).ok()
    };
    let mut value: u128 = 0;
    for part in &parts[..hi] {
        value = (value << 16) | hextet(part)?;
    }
    value = value.checked_shl(16 * (8 - hi) as u32).unwrap_or(0);
    let mut low: u128 = 0;
    for part in &parts[parts.len() - lo..] {
        low = (low << 16) | hextet(part)?;
    }
    Some(value | low)
}

/// `_ipv6`'s answer: `None` for a kept address (loopback, unspecified), else its kind: link-local,
/// else private (ipaddress, Python 3.13+), else public. An IPv4-mapped address has its IPv4 address's
/// properties.
pub fn ipv6_kind(address: u128) -> Option<&'static str> {
    let within =
        |network: u128, prefix: u32| prefix == 0 || (address ^ network) >> (128 - prefix) == 0;
    let net = |text: &str| parse_ipv6(text).expect("constant");
    if within(net("::ffff:0:0"), 96) {
        let (loopback, unspecified, private, link) = ipv4_props(address as u32);
        if loopback || unspecified {
            return None;
        }
        return Some(if link {
            "link"
        } else if private {
            "private"
        } else {
            "public"
        });
    }
    if address == 1 || address == 0 {
        return None;
    }
    if within(net("fe80::"), 10) {
        return Some("link");
    }
    let private = [
        ("::1", 128),
        ("::", 128),
        ("64:ff9b:1::", 48),
        ("100::", 64),
        ("2001::", 23),
        ("2001:db8::", 32),
        ("2002::", 16),
        ("3fff::", 20),
        ("fc00::", 7),
        ("fe80::", 10),
    ]
    .iter()
    .any(|&(n, p)| within(net(n), p));
    let exception = [
        ("2001:1::1", 128),
        ("2001:1::2", 128),
        ("2001:3::", 32),
        ("2001:4:112::", 48),
        ("2001:20::", 28),
        ("2001:30::", 28),
    ]
    .iter()
    .any(|&(n, p)| within(net(n), p));
    Some(if private && !exception {
        "private"
    } else {
        "public"
    })
}
