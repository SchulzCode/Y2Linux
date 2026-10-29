//! The diagnostic export's allowlists (diagnostic_fields.py and diagnostics.py's sets): the keys it
//! keeps, the kernel status lines it reads as `field=value` pairs and their fields, and the string
//! values that are public words. Each is a file of one entry per line in sorted (code point) order, so
//! a lookup is a binary search; the tests check the order.
// SPDX-License-Identifier: GPL-2.0-only
use std::sync::OnceLock;

type List = OnceLock<Vec<&'static str>>;

fn lines(cell: &'static List, text: &'static str) -> &'static [&'static str] {
    cell.get_or_init(|| text.lines().collect())
}

/// The keys a projected record keeps (any other key is dropped).
pub fn fields() -> &'static [&'static str] {
    static LIST: List = OnceLock::new();
    lines(&LIST, include_str!("fields/fields.txt"))
}

/// The keys whose string value is a kernel status line of `field=value` pairs.
pub fn kernel_status_keys() -> &'static [&'static str] {
    static LIST: List = OnceLock::new();
    lines(&LIST, include_str!("fields/kernel-status-keys.txt"))
}

/// The fields of a kernel status line a projection keeps.
pub fn kernel_fields() -> &'static [&'static str] {
    static LIST: List = OnceLock::new();
    lines(&LIST, include_str!("fields/kernel-fields.txt"))
}

/// String values that are public words (states, codecs, modes, reasons), kept as they are.
pub fn public_words() -> &'static [&'static str] {
    static LIST: List = OnceLock::new();
    lines(&LIST, include_str!("fields/public-words.txt"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_are_sorted_unique_and_complete() {
        for (list, length) in [
            (fields(), 652),
            (kernel_status_keys(), 14),
            (kernel_fields(), 177),
            (public_words(), 204),
        ] {
            assert_eq!(list.len(), length);
            assert!(list.windows(2).all(|w| w[0] < w[1]), "{list:?}");
            assert!(list.iter().all(|w| !w.is_empty() && w.trim() == *w));
        }
        assert!(public_words().contains(&"Internal Storage"));
        assert!(fields().binary_search(&"wifi_bytes_per_second").is_ok());
    }
}
