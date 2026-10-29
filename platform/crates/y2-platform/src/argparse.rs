//! What is left of the argparse emulation: the Python builtins some commands still reproduce
//! (`python`) and `error_text` for cli.py's refusals after parsing. The command line itself is cli.rs.
// SPDX-License-Identifier: GPL-2.0-only
pub(crate) mod python;
#[rustfmt::skip]
mod unicode;

/// An argument error of y2-platform itself found after parsing (`status --interval`): the text for
/// stderr; exit 2.
pub fn error_text(message: &str) -> String {
    crate::cli::error_text(None, message)
}

#[cfg(test)]
mod tests {
    use super::unicode::{DECIMAL_ZEROS, NONPRINTABLE, SPACE};

    #[test]
    fn the_unicode_tables_are_sorted_ranges() {
        // python.rs looks them up by binary search.
        assert!(NONPRINTABLE.iter().all(|(a, b)| a <= b));
        assert!(NONPRINTABLE.windows(2).all(|w| w[0].1 + 1 < w[1].0));
        assert!(SPACE.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(DECIMAL_ZEROS[0], 0x30);
        assert!(DECIMAL_ZEROS.windows(2).all(|w| w[1] - w[0] >= 10));
    }
}
